use std::{fs, io::Cursor, path::Path};

use image::{DynamicImage, ImageReader, Limits};
use lopdf::{Document, ObjectId};
use thiserror::Error;

use crate::{
    image_to_pdf::{ImageToPdfError, add_color_image_xobject},
    pdf_overlay::{append_page_content_with_reset, install_xobject},
    pdf_page_geometry::{PageForm, page_form},
};

const MAX_IMAGE_DIMENSION: u32 = 50_000;

#[derive(Debug, Clone, Copy)]
pub struct ImageOverlayOptions {
    pub x: f32,
    pub y: f32,
    pub every_page: bool,
}

#[derive(Debug, Error)]
pub enum ImageOverlayError {
    #[error("could not read PDF '{filename}': {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("input PDF '{filename}' has no pages")]
    EmptyPdf { filename: String },
    #[error("could not read overlay image: {0}")]
    ReadImage(#[from] std::io::Error),
    #[error("could not determine the overlay image format: {0}")]
    GuessImageFormat(std::io::Error),
    #[error("could not decode the overlay image: {0}")]
    DecodeImage(#[from] image::ImageError),
    #[error("SVG overlay is not valid UTF-8")]
    InvalidSvgEncoding,
    #[error("SVG overlay contains a disallowed external resource or document declaration")]
    UnsafeSvg,
    #[error("could not parse the SVG overlay: {0}")]
    ParseSvg(String),
    #[error("could not convert the SVG overlay to PDF: {0}")]
    ConvertSvg(String),
    #[error("the converted SVG has no page")]
    EmptySvg,
    #[error("could not add the overlay image to the PDF: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("overlay target page has an invalid MediaBox")]
    InvalidPageBox,
    #[error("could not encode the raster overlay: {0}")]
    RasterPdf(#[from] ImageToPdfError),
    #[error("could not write the overlaid PDF: {0}")]
    WritePdf(std::io::Error),
}

#[derive(Debug, Clone, Copy)]
struct OverlayObject {
    id: ObjectId,
    width: f32,
    height: f32,
}

/// Draws one raster or vector image at its intrinsic size on the first or all PDF pages.
///
/// SVG input is kept as vector PDF content. External SVG resource loading and XML document
/// declarations are rejected before parsing.
///
/// # Errors
///
/// Returns [`ImageOverlayError`] when either input is invalid or unsafe, the PDF has no pages,
/// or the output cannot be written.
pub fn overlay_image_to_file(
    input_path: &Path,
    input_filename: &str,
    image_path: &Path,
    options: ImageOverlayOptions,
    output_path: &Path,
) -> Result<(), ImageOverlayError> {
    let mut document = Document::load(input_path).map_err(|source| ImageOverlayError::ReadPdf {
        filename: input_filename.to_owned(),
        source,
    })?;
    document.renumber_objects_with(1);
    let page_ids = document.get_pages().into_values().collect::<Vec<_>>();
    if page_ids.is_empty() {
        return Err(ImageOverlayError::EmptyPdf {
            filename: input_filename.to_owned(),
        });
    }

    let image_bytes = fs::read(image_path)?;
    let overlay = if is_svg(&image_bytes) {
        let PageForm { id, width, height } = import_svg_form(&mut document, &image_bytes)?;
        OverlayObject { id, width, height }
    } else {
        raster_xobject(&mut document, &image_bytes)?
    };
    let page_count = if options.every_page {
        page_ids.len()
    } else {
        1
    };
    for (index, page_id) in page_ids.into_iter().take(page_count).enumerate() {
        apply_overlay(&mut document, page_id, overlay, options, index)?;
    }

    document.renumber_objects();
    document.compress();
    document
        .save(output_path)
        .map_err(ImageOverlayError::WritePdf)?;
    Ok(())
}

fn raster_xobject(
    document: &mut Document,
    bytes: &[u8],
) -> Result<OverlayObject, ImageOverlayError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(ImageOverlayError::GuessImageFormat)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    reader.limits(limits);
    let image = reader.decode()?;
    raster_image_xobject(document, &image)
}

fn raster_image_xobject(
    document: &mut Document,
    image: &DynamicImage,
) -> Result<OverlayObject, ImageOverlayError> {
    let (id, width, height) = add_color_image_xobject(document, image)?;
    Ok(OverlayObject { id, width, height })
}

pub(crate) fn import_svg_form(
    document: &mut Document,
    bytes: &[u8],
) -> Result<PageForm, ImageOverlayError> {
    let pdf = svg_to_pdf_bytes(bytes)?;
    let mut source = Document::load_mem(&pdf)?;
    source.renumber_objects_with(document.max_id.saturating_add(1));
    let source_page = source
        .get_pages()
        .into_values()
        .next()
        .ok_or(ImageOverlayError::EmptySvg)?;
    let form = page_form(&mut source, source_page)?;
    document.objects.extend(source.objects);
    document.max_id = document.max_id.max(source.max_id);
    Ok(form)
}

pub(crate) fn svg_to_pdf_bytes(bytes: &[u8]) -> Result<Vec<u8>, ImageOverlayError> {
    let svg = std::str::from_utf8(bytes).map_err(|_| ImageOverlayError::InvalidSvgEncoding)?;
    if unsafe_svg(svg) {
        return Err(ImageOverlayError::UnsafeSvg);
    }
    let mut options = svg2pdf::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = match svg2pdf::usvg::Tree::from_str(svg, &options) {
        Ok(tree) => tree,
        Err(original_error) => {
            let defaulted = with_default_svg_dimensions(svg);
            if defaulted == svg {
                return Err(ImageOverlayError::ParseSvg(original_error.to_string()));
            }
            svg2pdf::usvg::Tree::from_str(&defaulted, &options)
                .map_err(|error| ImageOverlayError::ParseSvg(error.to_string()))?
        }
    };
    svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .map_err(|error| ImageOverlayError::ConvertSvg(error.to_string()))
}

pub(crate) fn svg_to_pdf_bytes_with_a4_default(bytes: &[u8]) -> Result<Vec<u8>, ImageOverlayError> {
    let svg = std::str::from_utf8(bytes).map_err(|_| ImageOverlayError::InvalidSvgEncoding)?;
    let defaulted = with_default_svg_dimensions(svg);
    svg_to_pdf_bytes(defaulted.as_bytes())
}

fn apply_overlay(
    document: &mut Document,
    page_id: ObjectId,
    overlay: OverlayObject,
    options: ImageOverlayOptions,
    index: usize,
) -> Result<(), lopdf::Error> {
    let resource_name = format!("OverlayImage{index}");
    install_xobject(document, page_id, resource_name.as_bytes(), overlay.id)?;
    let content = format!(
        "q\n{} 0 0 {} {} {} cm\n/{resource_name} Do\nQ\n",
        pdf_number(overlay.width),
        pdf_number(overlay.height),
        pdf_number(options.x),
        pdf_number(options.y),
    );
    append_page_content_with_reset(document, page_id, content.as_bytes())
}

fn is_svg(bytes: &[u8]) -> bool {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_ascii_lowercase();
    prefix.contains("<svg") || (prefix.contains("<?xml") && prefix.contains("svg"))
}

fn unsafe_svg(svg: &str) -> bool {
    let lower = svg.to_ascii_lowercase();
    let compact = lower
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    lower.contains("<!doctype")
        || lower.contains("<!entity")
        || lower.contains("@import")
        || ["http://", "https://", "file://", "ftp://"]
            .iter()
            .any(|scheme| {
                compact.contains(&format!("href=\"{scheme}"))
                    || compact.contains(&format!("href='{scheme}"))
                    || compact.contains(&format!("url({scheme}"))
                    || compact.contains(&format!("url(\"{scheme}"))
                    || compact.contains(&format!("url('{scheme}"))
            })
}

fn with_default_svg_dimensions(svg: &str) -> String {
    let lower = svg.to_ascii_lowercase();
    let Some(start) = lower.find("<svg") else {
        return svg.to_owned();
    };
    let Some(relative_end) = lower[start..].find('>') else {
        return svg.to_owned();
    };
    let end = start + relative_end;
    let tag = &lower[start..end];
    let has_width = has_xml_attribute(tag, "width");
    let has_height = has_xml_attribute(tag, "height");
    if has_width && has_height {
        return svg.to_owned();
    }
    let mut attributes = String::new();
    if !has_width {
        attributes.push_str(" width=\"595\"");
    }
    if !has_height {
        attributes.push_str(" height=\"842\"");
    }
    format!("{}{}{}", &svg[..start + 4], attributes, &svg[start + 4..])
}

fn has_xml_attribute(tag: &str, name: &str) -> bool {
    let bytes = tag.as_bytes();
    let name_bytes = name.as_bytes();
    let mut start = 0;
    while let Some(relative) = tag[start..].find(name) {
        let index = start + relative;
        let before_is_boundary = index == 0
            || bytes
                .get(index - 1)
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'<');
        let mut after = index + name_bytes.len();
        while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        if before_is_boundary && bytes.get(after) == Some(&b'=') {
            return true;
        }
        start = index + name_bytes.len();
    }
    false
}

fn pdf_number(value: f32) -> String {
    let mut output = format!("{value:.5}");
    while output.contains('.') && output.ends_with('0') {
        output.pop();
    }
    if output.ends_with('.') {
        output.pop();
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lopdf::Document;

    use crate::pdf_overlay::test_support::{
        BALANCED_CONTENT, UNBALANCED_CONTENT, assert_matrix_eq, ctm_at_xobject, single_page_pdf,
    };

    use super::{
        ImageOverlayOptions, is_svg, overlay_image_to_file, unsafe_svg, with_default_svg_dimensions,
    };

    /// The overlaid image must land at the requested user-space coordinates at
    /// its intrinsic size, whatever the page's own stream left on the graphics
    /// state stack.
    #[test]
    fn overlaid_images_ignore_an_unbalanced_page_stream() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let overlay_image = directory.path().join("overlay.png");
        image::RgbaImage::from_pixel(40, 25, image::Rgba([0, 0, 255, 255])).save(&overlay_image)?;
        let options = ImageOverlayOptions {
            x: 100.0,
            y: 200.0,
            every_page: false,
        };

        let mut placements = Vec::new();
        for (name, content) in [
            ("balanced", BALANCED_CONTENT),
            ("unbalanced", UNBALANCED_CONTENT),
        ] {
            let input = directory.path().join(format!("{name}.pdf"));
            let output = directory.path().join(format!("{name}-overlaid.pdf"));
            fs::write(&input, single_page_pdf(content))?;
            overlay_image_to_file(&input, "input.pdf", &overlay_image, options, &output)?;
            let document = Document::load(&output)?;
            let page_id = *document.get_pages().values().next().ok_or("no page")?;
            placements.push(
                ctm_at_xobject(&document, page_id, "OverlayImage0")
                    .ok_or("the overlay is not drawn")?,
            );
        }

        assert_matrix_eq(placements[1], placements[0]);
        assert_matrix_eq(placements[1], [40.0, 0.0, 0.0, 25.0, 100.0, 200.0]);
        Ok(())
    }

    #[test]
    fn detects_svg_from_content_instead_of_filename() {
        assert!(is_svg(
            b"  <?xml version='1.0'?><svg width='1' height='1'/>"
        ));
        assert!(is_svg(b"<svg xmlns='http://www.w3.org/2000/svg'/>"));
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn rejects_external_svg_resources_and_xml_entities() {
        assert!(unsafe_svg("<svg><image href='file:///secret'/></svg>"));
        assert!(unsafe_svg("<!DOCTYPE svg [<!ENTITY x SYSTEM 'x'>]><svg/>"));
        assert!(!unsafe_svg(
            "<svg><image href='data:image/png;base64,AA=='/></svg>"
        ));
    }

    #[test]
    fn supplies_a4_dimensions_only_when_svg_dimensions_are_missing() {
        let svg = "<svg viewBox='0 0 10 10'><path d='M0 0'/></svg>";
        let defaulted = with_default_svg_dimensions(svg);
        assert!(defaulted.contains("width=\"595\" height=\"842\""));
        assert_eq!(
            with_default_svg_dimensions("<svg width = '10' height='20'></svg>"),
            "<svg width = '10' height='20'></svg>"
        );
    }
}
