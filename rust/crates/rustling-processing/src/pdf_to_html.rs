//! PDF → HTML conversion with an optional Poppler compatibility path.
//!
//! An installed `pdftohtml` remains preferred so existing operators retain its
//! complex-mode output. When the executable cannot be found, a bounded native
//! renderer uses the shared `PDFium` text geometry, heading/column inference, and
//! image extractor to produce a flat ZIP containing HTML, CSS, and PNG assets.
//!
//! Content is positioned in exactly one coordinate space — unrotated PDF user space,
//! the space `PDFium` reports text and image geometry in. A page's intrinsic `/Rotate`
//! is applied once, as a CSS transform on the page canvas, never by mixing rotated
//! page extents with unrotated content offsets.

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, ErrorKind, Read},
    path::{Component, Path},
    process::Command,
};

use tempfile::TempDir;
use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    ghostscript::exit_status,
    pdf_markdown::{heading_prefix, reading_order_groups},
    pdfium_backend::{
        ExtractImageFormat, ExtractedPageImage, MarkdownPageGeometry, MarkdownTextLine,
        PdfiumExtractImageLimits, PdfiumExtractImagesAttempt, PdfiumExtractImagesError,
        PdfiumMarkdownAttempt, PdfiumTextError, try_extract_markdown_lines,
        try_extract_page_images_to_zip,
    },
};

const PDFTOHTML_COMMAND_ENV: &str = "RUSTLING_PROCESSING_PDFTOHTML_COMMAND";
const MAX_OUTPUT_FILES: usize = 100_000;
const MAX_OUTPUT_BYTES: u64 = 200 * 1024 * 1024;
const MAX_HTML_BYTES: usize = 64 * 1024 * 1024;
const MAX_NATIVE_TEXT_BYTES: usize = 32 * 1024 * 1024;
const MAX_NATIVE_PAGES: usize = 10_000;
const MAX_NATIVE_IMAGES: usize = 10_000;
const MAX_NATIVE_IMAGE_PIXELS: u64 = 25_000_000;
const MAX_NATIVE_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PAGE_DIMENSION_POINTS: f32 = 20_000.0;
const DEFAULT_PAGE_WIDTH_POINTS: f32 = 612.0;
const DEFAULT_PAGE_HEIGHT_POINTS: f32 = 792.0;

const NATIVE_CSS: &str = r"html {
  background: #eceff3;
  color: #111;
  font-family: Arial, Helvetica, sans-serif;
}
body {
  margin: 0;
  padding: 24px;
}
.document {
  display: flex;
  flex-direction: column;
  gap: 24px;
  align-items: center;
}
.page {
  position: relative;
  overflow: hidden;
  box-sizing: border-box;
  flex: none;
  background: white;
  box-shadow: 0 2px 10px rgb(0 0 0 / 18%);
}
.page-canvas {
  position: absolute;
  top: 0;
  left: 0;
  transform-origin: 0 0;
}
.text-run {
  position: absolute;
  z-index: 2;
  box-sizing: border-box;
  margin: 0;
  padding: 0;
  color: #111;
  font-family: Arial, Helvetica, sans-serif;
  line-height: 1.05;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.embedded-image {
  position: absolute;
  z-index: 1;
  object-fit: fill;
}
";

/// Renderer chosen for a successful conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PdfToHtmlBackend {
    ExternalPdftohtml,
    NativePdfium,
}

#[derive(Debug, Error)]
pub enum PdfToHtmlError {
    #[error("pdftohtml with '{command}' failed with status {status}: {details}")]
    PdftohtmlFailed {
        command: String,
        status: String,
        details: String,
    },
    #[error("could not start pdftohtml command '{command}': {source}")]
    PdftohtmlStart {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("PDFium is required for native PDF-to-HTML conversion: {details}")]
    NativeUnavailable {
        explicitly_configured: bool,
        details: String,
    },
    #[error("could not extract native PDF text: {0}")]
    NativeText(#[from] PdfiumTextError),
    #[error("could not extract native PDF images: {0}")]
    NativeImages(#[from] PdfiumExtractImagesError),
    #[error("the PDF has no pages")]
    NoPages,
    #[error("PDF-to-HTML output contains more than {MAX_OUTPUT_FILES} files")]
    TooManyOutputs,
    #[error("PDF-to-HTML output exceeds the {MAX_OUTPUT_BYTES}-byte safety limit")]
    OutputTooLarge,
    #[error("native HTML exceeds the {MAX_HTML_BYTES}-byte safety limit")]
    HtmlTooLarge,
    #[error("native PDF text exceeds the {MAX_NATIVE_TEXT_BYTES}-byte safety limit")]
    TextTooLarge,
    #[error("native PDF page or text geometry exceeds the supported safety limits")]
    DocumentTooLarge,
    #[error("PDF-to-HTML produced an unsafe output filename: {0}")]
    UnsafeOutputName(String),
    #[error("pdftohtml produced no output")]
    NoOutput,
    #[error("could not prepare the conversion workspace: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not build the output archive: {0}")]
    Zip(#[from] zip::result::ZipError),
}

impl PdfToHtmlError {
    /// Returns true when the upload itself is what the endpoint rejected.
    ///
    /// [`Self::TooManyOutputs`] and [`Self::OutputTooLarge`] are deliberately excluded:
    /// they are raised by `package_outputs`, which is shared by both renderers, so they
    /// also fire for a legitimate large Poppler conversion. They express how much output
    /// this service is willing to materialise — a server-side policy — so they map to a
    /// 5xx rather than blaming the client for a cap it cannot see.
    #[must_use]
    pub fn is_client_input_error(&self) -> bool {
        match self {
            Self::NativeText(
                PdfiumTextError::ReadPdf { .. }
                | PdfiumTextError::ReadPage { .. }
                | PdfiumTextError::ReadText { .. }
                | PdfiumTextError::PageIndex
                | PdfiumTextError::InvalidPage { .. },
            )
            | Self::NativeImages(
                PdfiumExtractImagesError::ReadPdf { .. }
                | PdfiumExtractImagesError::ReadPage { .. }
                | PdfiumExtractImagesError::DecodeImage { .. }
                | PdfiumExtractImagesError::EncodeImage { .. }
                | PdfiumExtractImagesError::TooManyImages { .. }
                | PdfiumExtractImagesError::UnsafeImageDimensions { .. }
                | PdfiumExtractImagesError::EncodedImagesTooLarge { .. },
            )
            | Self::NoPages
            | Self::HtmlTooLarge
            | Self::TextTooLarge
            | Self::DocumentTooLarge => true,
            Self::TooManyOutputs
            | Self::OutputTooLarge
            | Self::PdftohtmlFailed { .. }
            | Self::PdftohtmlStart { .. }
            | Self::NativeUnavailable { .. }
            | Self::NativeText(PdfiumTextError::RuntimePoisoned)
            | Self::NativeImages(
                PdfiumExtractImagesError::RuntimePoisoned
                | PdfiumExtractImagesError::Io(_)
                | PdfiumExtractImagesError::Zip(_),
            )
            | Self::UnsafeOutputName(_)
            | Self::NoOutput
            | Self::Io(_)
            | Self::Zip(_) => false,
        }
    }
}

enum ExternalAttempt {
    Converted,
    Unavailable,
}

/// Converts a PDF to a ZIP of HTML, CSS, and extracted images.
///
/// Poppler complex-mode output is preferred when `pdftohtml` can be started.
/// A missing executable falls back to the native `PDFium` renderer.
///
/// # Errors
///
/// Returns [`PdfToHtmlError`] when the selected renderer cannot read the PDF,
/// conversion fails, safety limits are exceeded, or no output is produced.
pub fn convert_pdf_to_html(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<PdfToHtmlBackend, PdfToHtmlError> {
    convert_pdf_to_html_with_commands(input_path, filename, output_path, &pdftohtml_commands())
}

pub(crate) fn convert_pdf_to_html_with_commands(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
    commands: &[String],
) -> Result<PdfToHtmlBackend, PdfToHtmlError> {
    let base_name = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("document");
    let work_dir = TempDir::new()?;
    let arguments = [
        OsString::from("-c"),
        input_path.as_os_str().to_owned(),
        OsString::from(base_name),
    ];

    match run_pdftohtml(commands, &arguments, work_dir.path())? {
        ExternalAttempt::Converted => {
            tracing::info!(
                backend = "pdftohtml",
                "PDF-to-HTML conversion selected external Poppler"
            );
            package_outputs(work_dir.path(), output_path)?;
            Ok(PdfToHtmlBackend::ExternalPdftohtml)
        }
        ExternalAttempt::Unavailable => {
            tracing::debug!(
                backend = "native-pdfium",
                "pdftohtml was not found; falling back to native PDF-to-HTML"
            );
            render_native(input_path, filename, base_name, work_dir.path())?;
            package_outputs(work_dir.path(), output_path)?;
            tracing::info!(
                backend = "native-pdfium",
                "PDF-to-HTML conversion selected native PDFium renderer"
            );
            Ok(PdfToHtmlBackend::NativePdfium)
        }
    }
}

fn render_native(
    input_path: &Path,
    filename: &str,
    base_name: &str,
    work_dir: &Path,
) -> Result<(), PdfToHtmlError> {
    let (lines, pages, median_font_size, median_line_height, line_limit_reached) =
        match try_extract_markdown_lines(input_path, filename)? {
            PdfiumMarkdownAttempt::Extracted {
                lines,
                pages,
                median_font_size,
                median_line_height,
                line_limit_reached,
            } => (
                lines,
                pages,
                median_font_size,
                median_line_height,
                line_limit_reached,
            ),
            PdfiumMarkdownAttempt::Unavailable {
                explicitly_configured,
                details,
            } => {
                return Err(PdfToHtmlError::NativeUnavailable {
                    explicitly_configured,
                    details,
                });
            }
        };
    // The shared text primitive has no page cap; the native renderer owns its own so a
    // huge document is refused outright instead of rendered with missing pages.
    if line_limit_reached || pages.len() > MAX_NATIVE_PAGES {
        return Err(PdfToHtmlError::DocumentTooLarge);
    }
    if pages.is_empty() {
        return Err(PdfToHtmlError::NoPages);
    }
    let text_bytes = lines
        .iter()
        .try_fold(0_usize, |total, line| total.checked_add(line.text.len()))
        .ok_or(PdfToHtmlError::TextTooLarge)?;
    if text_bytes > MAX_NATIVE_TEXT_BYTES {
        return Err(PdfToHtmlError::TextTooLarge);
    }

    let safe_base_name = safe_base_name(base_name);
    let intermediate_dir = work_dir.join(".native-intermediate");
    fs::create_dir(&intermediate_dir)?;
    let image_archive = intermediate_dir.join("page-images.zip");
    let image_limits = PdfiumExtractImageLimits {
        images: MAX_NATIVE_IMAGES,
        pixels_per_image: MAX_NATIVE_IMAGE_PIXELS,
        encoded_bytes: MAX_NATIVE_IMAGE_BYTES,
    };
    let images = match try_extract_page_images_to_zip(
        input_path,
        filename,
        &safe_base_name,
        ExtractImageFormat::Png,
        "png",
        &image_archive,
        Some(image_limits),
    )? {
        PdfiumExtractImagesAttempt::Extracted { images } => images,
        PdfiumExtractImagesAttempt::Unavailable {
            explicitly_configured,
            details,
        } => {
            return Err(PdfToHtmlError::NativeUnavailable {
                explicitly_configured,
                details,
            });
        }
    };
    unpack_native_images(&image_archive, work_dir)?;

    let html = render_native_html(
        filename,
        &safe_base_name,
        &pages,
        &lines,
        median_font_size,
        median_line_height,
        &images,
    )?;
    fs::write(work_dir.join(format!("{safe_base_name}.html")), html)?;
    fs::write(work_dir.join(format!("{safe_base_name}.css")), NATIVE_CSS)?;
    Ok(())
}

fn render_native_html(
    filename: &str,
    base_name: &str,
    pages: &[MarkdownPageGeometry],
    lines: &[MarkdownTextLine],
    median_font_size: f32,
    median_line_height: f32,
    images: &[ExtractedPageImage],
) -> Result<String, PdfToHtmlError> {
    let mut html = String::with_capacity(lines.len().saturating_mul(160).min(MAX_HTML_BYTES));
    push_html(&mut html, "<!doctype html>\n<html lang=\"en\">\n<head>\n")?;
    push_html(&mut html, "<meta charset=\"utf-8\">\n")?;
    push_html(
        &mut html,
        "<meta name=\"generator\" content=\"RustlingPDF native PDF-to-HTML\">\n",
    )?;
    push_html(
        &mut html,
        &format!("<title>{}</title>\n", escape_html_text(filename)),
    )?;
    push_html(
        &mut html,
        &format!("<link rel=\"stylesheet\" href=\"{base_name}.css\">\n"),
    )?;
    push_html(
        &mut html,
        "</head>\n<body>\n<main class=\"document\" data-renderer=\"native-pdfium\">\n",
    )?;

    let mut line_index = 0_usize;
    for page in pages {
        let layout = page_layout(page);
        let PageLayout {
            canvas_width: page_width,
            canvas_height: page_height,
            ..
        } = layout;
        push_html(
            &mut html,
            &format!(
                "<section class=\"page\" id=\"page-{}\" data-page-number=\"{}\" data-rotation=\"{}\" style=\"width:{:.2}pt;height:{:.2}pt\">\n",
                page.page + 1,
                page.page + 1,
                layout.rotation_degrees,
                layout.display_width,
                layout.display_height
            ),
        )?;
        // Content coordinates stay in unrotated user space inside this canvas; the
        // page's intrinsic /Rotate is applied once, here, as a CSS transform.
        push_html(
            &mut html,
            &format!(
                "<div class=\"page-canvas\" style=\"width:{page_width:.2}pt;height:{page_height:.2}pt{}\">\n",
                layout.canvas_transform()
            ),
        )?;

        for image in images.iter().filter(|image| image.page == page.page) {
            let x = bounded_coordinate(image.x, page_width);
            let width = bounded_extent(image.width, page_width - x);
            let height = bounded_extent(image.height, page_height);
            let top = bounded_coordinate(page_height - image.y - height, page_height);
            push_html(
                &mut html,
                &format!(
                    "<img class=\"embedded-image\" src=\"{}\" alt=\"\" data-page=\"{}\" style=\"left:{x:.2}pt;top:{top:.2}pt;width:{width:.2}pt;height:{height:.2}pt\">\n",
                    image.filename,
                    page.page + 1
                ),
            )?;
        }

        let page_start = line_index;
        while line_index < lines.len() && lines[line_index].page == page.page {
            line_index += 1;
        }
        let mut page_lines: Vec<&MarkdownTextLine> = lines[page_start..line_index].iter().collect();
        // PDFium's segment order depends on the page's display orientation, so a rotated
        // page arrives bottom-to-top. Sort by descending user-space top edge to make DOM
        // order visual (and rotation-independent) before column grouping.
        page_lines.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
        for group in reading_order_groups(&page_lines) {
            for line in group {
                append_text_run(
                    &mut html,
                    line,
                    page_height,
                    page_width,
                    median_font_size,
                    median_line_height,
                )?;
            }
        }
        push_html(&mut html, "</div>\n</section>\n")?;
    }
    push_html(&mut html, "</main>\n</body>\n</html>\n")?;
    Ok(html)
}

fn append_text_run(
    html: &mut String,
    line: &MarkdownTextLine,
    page_height: f32,
    page_width: f32,
    median_font_size: f32,
    median_line_height: f32,
) -> Result<(), PdfToHtmlError> {
    let font_size = if line.dominant_font_size.is_finite() && line.dominant_font_size > 0.0 {
        line.dominant_font_size.clamp(1.0, 200.0)
    } else {
        median_font_size.clamp(1.0, 200.0)
    };
    let x = bounded_coordinate(line.x, page_width);
    let top = bounded_coordinate(page_height - line.y, page_height);
    let width = bounded_extent(line.width, page_width - x);
    let weight = if line.bold { 700 } else { 400 };
    let style = if line.italic { "italic" } else { "normal" };
    let tag = match heading_prefix(
        &line.text,
        line.dominant_font_size,
        line.height,
        median_font_size,
        median_line_height,
    ) {
        "# " => "h1",
        "## " => "h2",
        _ => "div",
    };
    push_html(
        html,
        &format!(
            "<{tag} class=\"text-run\" data-page=\"{}\" style=\"left:{x:.2}pt;top:{top:.2}pt;width:{width:.2}pt;font-size:{font_size:.2}pt;font-weight:{weight};font-style:{style}\">{}</{tag}>\n",
            line.page + 1,
            escape_html_text(&line.text)
        ),
    )
}

/// Resolved geometry for one rendered page.
///
/// `canvas_*` is the unrotated content box that text and image coordinates live in;
/// `display_*` is the laid-out box after the page's intrinsic `/Rotate`, so the flow
/// reserves the space a viewer would actually show.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PageLayout {
    canvas_width: f32,
    canvas_height: f32,
    display_width: f32,
    display_height: f32,
    rotation_degrees: u16,
}

impl PageLayout {
    /// Returns the CSS that maps the unrotated canvas onto the rotated display box.
    ///
    /// `transform-origin` is the canvas's top-left corner, so each rotation is followed
    /// by the translation that brings the rotated corner back into the display box.
    fn canvas_transform(&self) -> String {
        match self.rotation_degrees {
            90 => format!(
                ";transform:translate({:.2}pt,0pt) rotate(90deg)",
                self.canvas_height
            ),
            180 => format!(
                ";transform:translate({:.2}pt,{:.2}pt) rotate(180deg)",
                self.canvas_width, self.canvas_height
            ),
            270 => format!(
                ";transform:translate(0pt,{:.2}pt) rotate(270deg)",
                self.canvas_width
            ),
            _ => String::new(),
        }
    }
}

fn page_layout(page: &MarkdownPageGeometry) -> PageLayout {
    let canvas_width = if page.width.is_finite() && page.width > 0.0 {
        page.width.clamp(1.0, MAX_PAGE_DIMENSION_POINTS)
    } else {
        DEFAULT_PAGE_WIDTH_POINTS
    };
    let canvas_height = if page.height.is_finite() && page.height > 0.0 {
        page.height.clamp(1.0, MAX_PAGE_DIMENSION_POINTS)
    } else {
        DEFAULT_PAGE_HEIGHT_POINTS
    };
    let rotation_degrees = match page.rotation_degrees {
        90 => 90,
        180 => 180,
        270 => 270,
        _ => 0,
    };
    let (display_width, display_height) = if matches!(rotation_degrees, 90 | 270) {
        (canvas_height, canvas_width)
    } else {
        (canvas_width, canvas_height)
    };
    PageLayout {
        canvas_width,
        canvas_height,
        display_width,
        display_height,
        rotation_degrees,
    }
}

fn bounded_coordinate(value: f32, maximum: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, maximum)
    } else {
        0.0
    }
}

fn bounded_extent(value: f32, maximum: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value.clamp(0.1, maximum.max(0.1))
    } else {
        0.1
    }
}

fn escape_html_text(value: &str) -> String {
    ammonia::clean_text(value)
        .replace("&#32;", " ")
        .replace("&#9;", "\t")
        .replace("&#10;", "\n")
        .replace("&#12;", "\u{c}")
        .replace("&#13;", "\r")
        .replace("&#47;", "/")
        .replace("&#61;", "=")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&grave;", "`")
}

fn push_html(output: &mut String, value: &str) -> Result<(), PdfToHtmlError> {
    let length = output
        .len()
        .checked_add(value.len())
        .ok_or(PdfToHtmlError::HtmlTooLarge)?;
    if length > MAX_HTML_BYTES {
        return Err(PdfToHtmlError::HtmlTooLarge);
    }
    output.push_str(value);
    Ok(())
}

fn safe_base_name(value: &str) -> String {
    let result: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    if result.is_empty() || result == "." || result == ".." {
        "document".to_owned()
    } else {
        result
    }
}

fn unpack_native_images(archive_path: &Path, output_dir: &Path) -> Result<(), PdfToHtmlError> {
    let mut archive = ZipArchive::new(File::open(archive_path)?)?;
    if archive.len() > MAX_NATIVE_IMAGES {
        return Err(PdfToHtmlError::TooManyOutputs);
    }
    let mut total_bytes = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        if !is_safe_flat_name(&name) {
            return Err(PdfToHtmlError::UnsafeOutputName(name));
        }
        let remaining = MAX_NATIVE_IMAGE_BYTES
            .checked_sub(total_bytes)
            .ok_or(PdfToHtmlError::OutputTooLarge)?;
        let mut output = File::create(output_dir.join(&name))?;
        let copied = io::copy(&mut entry.take(remaining + 1), &mut output)?;
        if copied > remaining {
            return Err(PdfToHtmlError::OutputTooLarge);
        }
        total_bytes = total_bytes.saturating_add(copied);
    }
    Ok(())
}

fn is_safe_flat_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && Path::new(name).components().all(|component| {
            matches!(component, Component::Normal(_))
                && Path::new(name)
                    .file_name()
                    .is_some_and(|value| value == name)
        })
}

/// Archives the workspace for **either** renderer, bounding how much output the service
/// materialises. Both caps are server-side policy, not a client-input judgement; see
/// [`PdfToHtmlError::is_client_input_error`].
fn package_outputs(work_dir: &Path, output_path: &Path) -> Result<(), PdfToHtmlError> {
    package_outputs_within(work_dir, output_path, MAX_OUTPUT_FILES, MAX_OUTPUT_BYTES)
}

fn package_outputs_within(
    work_dir: &Path,
    output_path: &Path,
    max_files: usize,
    max_bytes: u64,
) -> Result<(), PdfToHtmlError> {
    let mut outputs = Vec::new();
    for entry in fs::read_dir(work_dir)? {
        let path = entry?.path();
        if path.is_file() {
            if outputs.len() >= max_files {
                return Err(PdfToHtmlError::TooManyOutputs);
            }
            outputs.push(path);
        }
    }
    if outputs.is_empty() {
        return Err(PdfToHtmlError::NoOutput);
    }
    outputs.sort();

    let file = File::create(output_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut total_bytes = 0_u64;
    for path in &outputs {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|name| is_safe_flat_name(name))
            .ok_or_else(|| PdfToHtmlError::UnsafeOutputName(path.display().to_string()))?;
        zip.start_file(name, options)?;
        let remaining = max_bytes
            .checked_sub(total_bytes)
            .ok_or(PdfToHtmlError::OutputTooLarge)?;
        let input = File::open(path)?;
        let copied = io::copy(&mut input.take(remaining + 1), &mut zip)?;
        if copied > remaining {
            return Err(PdfToHtmlError::OutputTooLarge);
        }
        total_bytes = total_bytes.saturating_add(copied);
    }
    zip.finish()?;
    Ok(())
}

fn run_pdftohtml(
    commands: &[String],
    arguments: &[OsString],
    work_dir: &Path,
) -> Result<ExternalAttempt, PdfToHtmlError> {
    for command in commands {
        match Command::new(command)
            .args(arguments)
            .current_dir(work_dir)
            .output()
        {
            Ok(output) if output.status.success() => return Ok(ExternalAttempt::Converted),
            Ok(output) => {
                return Err(PdfToHtmlError::PdftohtmlFailed {
                    command: command.clone(),
                    status: exit_status(output.status),
                    details: process_details(&output.stdout, &output.stderr),
                });
            }
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(PdfToHtmlError::PdftohtmlStart {
                    command: command.clone(),
                    source,
                });
            }
        }
    }
    Ok(ExternalAttempt::Unavailable)
}

fn pdftohtml_commands() -> Vec<String> {
    if let Ok(command) = crate::env_compat::var(PDFTOHTML_COMMAND_ENV)
        && !command.trim().is_empty()
    {
        return vec![command];
    }
    if cfg!(windows) {
        vec!["pdftohtml.exe".to_owned(), "pdftohtml".to_owned()]
    } else {
        vec!["pdftohtml".to_owned()]
    }
}

fn process_details(stdout: &[u8], stderr: &[u8]) -> String {
    let bytes = if stderr.is_empty() { stdout } else { stderr };
    let details = String::from_utf8_lossy(bytes);
    let mut characters = details.trim().chars();
    let result = characters.by_ref().take(2_048).collect::<String>();
    if characters.next().is_some() {
        format!("{result}…")
    } else if result.is_empty() {
        "no diagnostic output".to_owned()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Cursor, Read},
    };

    use lopdf::{Document, Object, Stream, dictionary};
    use tempfile::tempdir;
    use zip::ZipArchive;

    use super::{
        PdfToHtmlBackend, PdfToHtmlError, convert_pdf_to_html_with_commands, render_native_html,
    };
    use crate::pdfium_backend::{MarkdownPageGeometry, MarkdownTextLine};

    #[test]
    fn native_html_is_positioned_semantic_escaped_and_column_ordered()
    -> Result<(), Box<dyn std::error::Error>> {
        let pages = [MarkdownPageGeometry {
            page: 0,
            width: 500.0,
            height: 700.0,
            rotation_degrees: 0,
        }];
        let mut lines = vec![line("Title <safe>", 20.0, 20.0, 650.0)];
        lines.extend([
            positioned_line("Left one", 50.0, 100.0, 600.0),
            positioned_line("Right one", 300.0, 100.0, 590.0),
            positioned_line("Left three", 50.0, 100.0, 560.0),
            positioned_line("Right two", 300.0, 100.0, 570.0),
            positioned_line("Left two", 50.0, 100.0, 580.0),
            positioned_line("Right four", 300.0, 100.0, 530.0),
            positioned_line("Left four", 50.0, 100.0, 540.0),
            positioned_line("Right three", 300.0, 100.0, 550.0),
        ]);
        let html = render_native_html("unsafe.pdf", "unsafe", &pages, &lines, 10.0, 12.0, &[])?;
        assert!(html.contains("<h1 class=\"text-run\""));
        assert!(html.contains("Title &lt;safe&gt;"));
        assert!(html.contains("font-weight:700;font-style:italic"));
        assert!(html.contains("id=\"page-1\""));
        assert!(matches!(
            (html.find("Left four"), html.find("Right one")),
            (Some(left), Some(right)) if left < right
        ));
        Ok(())
    }

    /// Rotated pages must position text exactly like the unrotated page, because the
    /// renderer keeps content in unrotated user space and applies `/Rotate` once as a
    /// CSS transform. Before this fix the section used PDFium's rotation-aware extents
    /// while the runs used unrotated coordinates, so `page_height - line.y` went negative
    /// and every run clamped to `top:0.00pt`.
    #[test]
    fn rotated_pages_keep_positions_and_rotate_at_the_css_level()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = [
            positioned_line("Top line", 72.0, 200.0, 716.72),
            positioned_line("Second line", 72.0, 200.0, 670.15),
            positioned_line("Third line", 72.0, 200.0, 623.58),
        ];
        let baseline = render_rotated(0, &lines)?;
        assert!(baseline.contains("data-rotation=\"0\""));
        assert!(baseline.contains("style=\"width:612.00pt;height:792.00pt\""));
        // No transform on an unrotated page.
        assert!(!baseline.contains("rotate("));
        let baseline_tops = text_run_tops(&baseline);
        assert_eq!(baseline_tops.len(), 3);
        assert!(
            baseline_tops.iter().all(|top| *top > 1.0),
            "unrotated baseline must not collapse to the top edge: {baseline_tops:?}"
        );
        assert!(
            baseline_tops.windows(2).all(|pair| pair[0] < pair[1]),
            "runs must stay in top-to-bottom order: {baseline_tops:?}"
        );

        for (rotation, expected_section, expected_transform) in [
            (
                90_u16,
                "style=\"width:792.00pt;height:612.00pt\"",
                ";transform:translate(792.00pt,0pt) rotate(90deg)",
            ),
            (
                180,
                "style=\"width:612.00pt;height:792.00pt\"",
                ";transform:translate(612.00pt,792.00pt) rotate(180deg)",
            ),
            (
                270,
                "style=\"width:792.00pt;height:612.00pt\"",
                ";transform:translate(0pt,612.00pt) rotate(270deg)",
            ),
        ] {
            let html = render_rotated(rotation, &lines)?;
            assert!(
                html.contains(&format!("data-rotation=\"{rotation}\"")),
                "rotation {rotation} is not recorded on the page section"
            );
            assert!(
                html.contains(expected_section),
                "rotation {rotation} must lay out the rotated page box, got:\n{html}"
            );
            assert!(
                html.contains(expected_transform),
                "rotation {rotation} must carry its canvas transform, got:\n{html}"
            );
            // The canvas keeps the unrotated page box.
            assert!(html.contains("<div class=\"page-canvas\" style=\"width:612.00pt;height:792.00pt"));
            let tops = text_run_tops(&html);
            assert_eq!(
                tops, baseline_tops,
                "rotation {rotation} must not move runs inside the canvas"
            );
            assert!(
                tops.iter().all(|top| *top > 1.0),
                "rotation {rotation} collapsed runs to the top edge: {tops:?}"
            );
            assert!(
                matches!(
                    (html.find("Top line"), html.find("Third line")),
                    (Some(first), Some(last)) if first < last
                ),
                "rotation {rotation} must keep DOM reading order"
            );
        }
        Ok(())
    }

    fn render_rotated(
        rotation_degrees: u16,
        lines: &[MarkdownTextLine],
    ) -> Result<String, Box<dyn std::error::Error>> {
        let pages = [MarkdownPageGeometry {
            page: 0,
            width: 612.0,
            height: 792.0,
            rotation_degrees,
        }];
        Ok(render_native_html(
            "rotated.pdf",
            "rotated",
            &pages,
            lines,
            10.0,
            12.0,
            &[],
        )?)
    }

    /// Extracts every `top:` value from the emitted text runs, in DOM order.
    fn text_run_tops(html: &str) -> Vec<f32> {
        html.lines()
            .filter(|line| line.contains("class=\"text-run\""))
            .filter_map(|line| {
                let start = line.find(";top:")? + ";top:".len();
                let rest = &line[start..];
                let end = rest.find("pt")?;
                rest[..end].parse::<f32>().ok()
            })
            .collect()
    }

    #[test]
    fn missing_external_command_uses_native_zip_when_pdfium_is_available()
    -> Result<(), Box<dyn std::error::Error>> {
        if crate::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_none() {
            return Ok(());
        }
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        std::fs::write(&input, two_page_pdf()?)?;
        let output = directory.path().join("out.zip");
        let backend = convert_pdf_to_html_with_commands(
            &input,
            "source.pdf",
            &output,
            &["/definitely/missing/rustling-pdftohtml".to_owned()],
        )?;
        assert_eq!(backend, PdfToHtmlBackend::NativePdfium);
        let mut archive = ZipArchive::new(File::open(output)?)?;
        let mut html = String::new();
        archive.by_name("source.html")?.read_to_string(&mut html)?;
        assert!(html.contains("First page"));
        assert!(html.contains("Second page"));
        assert!(matches!(
            (html.find("First page"), html.find("Second page")),
            (Some(first), Some(second)) if first < second
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn available_external_command_keeps_precedence() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir()?;
        let command = directory.path().join("fake-pdftohtml");
        std::fs::write(
            &command,
            "#!/bin/sh\nprintf '<html>external marker</html>\\n' > \"$3.html\"\n",
        )?;
        let mut permissions = std::fs::metadata(&command)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&command, permissions)?;
        let input = directory.path().join("input.pdf");
        std::fs::write(&input, b"external path does not inspect this")?;
        let output = directory.path().join("out.zip");
        let backend = convert_pdf_to_html_with_commands(
            &input,
            "source.pdf",
            &output,
            &[command.to_string_lossy().into_owned()],
        )?;
        assert_eq!(backend, PdfToHtmlBackend::ExternalPdftohtml);
        let mut archive = ZipArchive::new(File::open(output)?)?;
        let mut html = String::new();
        archive.by_name("source.html")?.read_to_string(&mut html)?;
        assert!(html.contains("external marker"));
        Ok(())
    }

    /// `package_outputs` is shared by the external and native renderers, so its caps must
    /// not be reported as a client input error: a legitimate large Poppler conversion
    /// would then fail as `400 Bad Request` blaming the client for a server-side cap.
    #[test]
    fn shared_output_caps_are_server_side_conditions() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let work_dir = directory.path().join("work");
        std::fs::create_dir(&work_dir)?;
        std::fs::write(work_dir.join("a.html"), vec![b'a'; 4_096])?;
        std::fs::write(work_dir.join("b.html"), vec![b'b'; 4_096])?;

        let too_large = super::package_outputs_within(
            &work_dir,
            &directory.path().join("too-large.zip"),
            10,
            1_024,
        )
        .expect_err("an over-cap output must not be archived");
        assert!(matches!(too_large, PdfToHtmlError::OutputTooLarge));
        assert!(
            !too_large.is_client_input_error(),
            "an output-size cap is a server-side policy, not bad client input"
        );

        let too_many = super::package_outputs_within(
            &work_dir,
            &directory.path().join("too-many.zip"),
            1,
            1024 * 1024,
        )
        .expect_err("an over-count output must not be archived");
        assert!(matches!(too_many, PdfToHtmlError::TooManyOutputs));
        assert!(!too_many.is_client_input_error());

        // Input-shaped rejections stay client errors.
        assert!(PdfToHtmlError::DocumentTooLarge.is_client_input_error());
        assert!(PdfToHtmlError::TextTooLarge.is_client_input_error());
        assert!(PdfToHtmlError::HtmlTooLarge.is_client_input_error());

        // Under the caps the archive is still produced.
        super::package_outputs_within(&work_dir, &directory.path().join("ok.zip"), 10, 1024 * 1024)?;
        Ok(())
    }

    #[test]
    fn damaged_pdf_returns_an_error_without_panicking() -> Result<(), Box<dyn std::error::Error>> {
        if crate::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_none() {
            return Ok(());
        }
        let directory = tempdir()?;
        let input = directory.path().join("damaged.pdf");
        std::fs::write(&input, b"%PDF-1.7\nnot a valid cross-reference table")?;
        let output = directory.path().join("out.zip");
        let result = convert_pdf_to_html_with_commands(
            &input,
            "damaged.pdf",
            &output,
            &["/definitely/missing/rustling-pdftohtml".to_owned()],
        );
        assert!(matches!(
            result,
            Err(PdfToHtmlError::NativeText(_) | PdfToHtmlError::NoPages)
        ));
        Ok(())
    }

    fn line(text: &str, font_size: f32, x: f32, y: f32) -> MarkdownTextLine {
        MarkdownTextLine {
            page: 0,
            text: text.to_owned(),
            dominant_font_size: font_size,
            height: font_size,
            x,
            width: 450.0,
            y,
            paragraph_break_before: false,
            bold: true,
            italic: true,
        }
    }

    fn positioned_line(text: &str, x: f32, width: f32, y: f32) -> MarkdownTextLine {
        MarkdownTextLine {
            page: 0,
            text: text.to_owned(),
            dominant_font_size: 10.0,
            height: 12.0,
            x,
            width,
            y,
            paragraph_break_before: false,
            bold: false,
            italic: false,
        }
    }

    fn two_page_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let mut pages = Vec::new();
        for (text, y) in [("First page", 140), ("Second page", 120)] {
            let content = format!("BT /F1 12 Tf 10 {y} Td ({text}) Tj ET");
            let content_id = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            pages.push(document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                "Contents" => content_id,
            }));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => pages.into_iter().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => 2,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Cursor::new(Vec::new());
        document.save_to(&mut bytes)?;
        Ok(bytes.into_inner())
    }
}
