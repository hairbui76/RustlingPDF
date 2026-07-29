use std::{
    collections::{HashSet, VecDeque},
    path::Path,
};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content, dictionary};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::{
    pdf_page_geometry::{PageForm, inherited_value, page_form, replace_page_tree},
    pdfium_backend::{
        DetectedCropBounds, PdfiumAutoCropAttempt, PdfiumAutoCropError, PdfiumCropContentAttempt,
        PdfiumCropContentError, try_detect_auto_crop_bounds, try_remove_content_outside_crop,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct CropOptions {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub remove_data_outside_crop: bool,
    pub auto_crop: bool,
}

#[derive(Debug, Error)]
pub enum CropError {
    #[error("crop coordinates (x, y, width, height) are required when auto-crop is not enabled")]
    MissingCoordinates,
    #[error("crop coordinates must be finite numbers")]
    NonFiniteCoordinates,
    #[error("could not read PDF '{filename}': {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("PDF '{filename}' has no pages")]
    NoPages { filename: String },
    #[error("automatic crop returned a different number of page bounds than the source PDF")]
    PageCountMismatch,
    #[error("automatic crop requires PDFium: {details}")]
    PdfiumRuntime {
        explicitly_configured: bool,
        details: String,
    },
    #[error(transparent)]
    Pdfium(#[from] PdfiumAutoCropError),
    #[error("PDF structure error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("could not write cropped PDF: {0}")]
    WritePdf(std::io::Error),
    #[error("could not stage the PDF for out-of-crop content removal: {0}")]
    CropContentInput(std::io::Error),
    #[error(
        "removeDataOutsideCrop=true requires PDFium, which is not available on this system: \
         {details}"
    )]
    CropContentRuntime {
        explicitly_configured: bool,
        details: String,
    },
    #[error(transparent)]
    CropContent(#[from] PdfiumCropContentError),
}

/// Crops every page using explicit coordinates or PDFium-rendered content bounds.
///
/// With `remove_data_outside_crop` the out-of-crop content is physically discarded
/// before the pages are rebuilt; without it the pages are only clipped, so the
/// original marks stay in the file.
///
/// # Errors
///
/// Returns [`CropError`] when request coordinates are missing, the PDF cannot be
/// read or rebuilt, or `PDFium` — required for automatic detection and for
/// out-of-crop content removal — is unavailable or fails.
pub fn crop_pdf_to_file(
    input_path: &Path,
    filename: &str,
    options: CropOptions,
    output_path: &Path,
) -> Result<(), CropError> {
    if options.auto_crop {
        let bounds = match try_detect_auto_crop_bounds(input_path, filename)? {
            PdfiumAutoCropAttempt::Detected(bounds) => bounds,
            PdfiumAutoCropAttempt::Unavailable {
                explicitly_configured,
                details,
            } => {
                return Err(CropError::PdfiumRuntime {
                    explicitly_configured,
                    details,
                });
            }
        };
        return rebuild_cropped_pdf(input_path, filename, &bounds, output_path, false);
    }

    let bounds = explicit_bounds(options)?;
    if options.remove_data_outside_crop {
        let pruned = NamedTempFile::new_in(output_path.parent().unwrap_or_else(|| Path::new(".")))
            .map_err(CropError::CropContentInput)?;
        match try_remove_content_outside_crop(input_path, filename, bounds, pruned.path())? {
            PdfiumCropContentAttempt::Removed => {}
            PdfiumCropContentAttempt::Unavailable {
                explicitly_configured,
                details,
            } => {
                return Err(CropError::CropContentRuntime {
                    explicitly_configured,
                    details,
                });
            }
        }
        let page_count = page_count(pruned.path(), filename)?;
        return rebuild_cropped_pdf(
            pruned.path(),
            filename,
            &vec![bounds; page_count],
            output_path,
            true,
        );
    }
    let page_count = page_count(input_path, filename)?;
    rebuild_cropped_pdf(
        input_path,
        filename,
        &vec![bounds; page_count],
        output_path,
        false,
    )
}

fn explicit_bounds(options: CropOptions) -> Result<DetectedCropBounds, CropError> {
    let (Some(x), Some(y), Some(width), Some(height)) =
        (options.x, options.y, options.width, options.height)
    else {
        return Err(CropError::MissingCoordinates);
    };
    if ![x, y, width, height].into_iter().all(f32::is_finite) {
        return Err(CropError::NonFiniteCoordinates);
    }
    Ok(DetectedCropBounds {
        x,
        y,
        width,
        height,
    })
}

fn page_count(input_path: &Path, filename: &str) -> Result<usize, CropError> {
    let document = load_document(input_path, filename)?;
    let page_count = document.get_pages().len();
    if page_count == 0 {
        Err(CropError::NoPages {
            filename: filename.to_owned(),
        })
    } else {
        Ok(page_count)
    }
}

fn rebuild_cropped_pdf(
    input_path: &Path,
    filename: &str,
    bounds: &[DetectedCropBounds],
    output_path: &Path,
    prune_unreferenced_resources: bool,
) -> Result<(), CropError> {
    let mut document = load_document(input_path, filename)?;
    if prune_unreferenced_resources {
        prune_unreferenced_pattern_and_shading(&mut document)?;
    }
    let page_ids = document.get_pages().into_values().collect::<Vec<_>>();
    if page_ids.is_empty() {
        return Err(CropError::NoPages {
            filename: filename.to_owned(),
        });
    }
    if page_ids.len() != bounds.len() {
        return Err(CropError::PageCountMismatch);
    }
    let root_pages_id = document.catalog()?.get(b"Pages")?.as_reference()?;
    let forms = page_ids
        .into_iter()
        .map(|page_id| page_form(&mut document, page_id))
        .collect::<Result<Vec<_>, _>>()?;
    let pages = forms
        .into_iter()
        .zip(bounds)
        .map(|(form, bounds)| add_cropped_page(&mut document, root_pages_id, form, *bounds))
        .collect();
    replace_page_tree(&mut document, root_pages_id, pages)?;
    document.catalog_mut()?.remove(b"AcroForm");
    // Rebuilding the page tree orphans the original page objects, their annotations,
    // and — after out-of-crop content removal — PDFium's superseded content streams.
    // Unreferenced objects are still bytes in the saved file, so a crop that promised
    // to delete data would otherwise leave it recoverable. Prune before writing.
    document.prune_objects();
    document.renumber_objects();
    document.compress();
    document.save(output_path).map_err(CropError::WritePdf)?;
    Ok(())
}

/// Drops `/Pattern` and `/Shading` resource entries that no surviving mark names.
///
/// `PDFium` rebuilds `/Font`, `/ExtGState`, and `/XObject` when it regenerates a
/// page — which is why a removed image or Form `XObject` really does leave the file
/// — but it leaves `/Pattern` and `/Shading` exactly as it found them. Since
/// [`page_form`] copies the page's `/Resources` verbatim into the rebuilt page's
/// Form `XObject`, an out-of-crop mark painted with a tiling pattern or a shading
/// would keep its whole subtree reachable, and `prune_objects` would rightly
/// preserve it: the pattern's text, its images, and a shading's sampled-function
/// data would all still be extractable from a file whose caller asked for them to
/// be deleted.
///
/// Only entries no surviving content stream references are dropped, so this cannot
/// remove something still painted.
fn prune_unreferenced_pattern_and_shading(document: &mut Document) -> Result<(), CropError> {
    let page_ids = document.get_pages().into_values().collect::<Vec<_>>();
    for page_id in page_ids {
        let Ok(resources) = inherited_value(document, page_id, b"Resources") else {
            continue;
        };
        let Some(mut resources) = resource_dictionary(document, &resources) else {
            continue;
        };
        if resources.get(b"Pattern").is_err() && resources.get(b"Shading").is_err() {
            continue;
        }
        let referenced = referenced_pattern_and_shading_names(document, page_id, &resources);
        retain_named_resources(document, &mut resources, b"Pattern", &referenced.patterns);
        retain_named_resources(document, &mut resources, b"Shading", &referenced.shadings);
        document
            .get_dictionary_mut(page_id)?
            .set("Resources", Object::Dictionary(resources));
    }
    Ok(())
}

#[derive(Default)]
struct ReferencedNames {
    patterns: HashSet<Vec<u8>>,
    shadings: HashSet<Vec<u8>>,
}

/// Collects every `/Pattern` and `/Shading` resource name reachable from a page's
/// surviving content: the page's own streams, the Form `XObjects` they invoke
/// (recursively), and the content of any pattern that is itself still referenced.
fn referenced_pattern_and_shading_names(
    document: &Document,
    page_id: ObjectId,
    resources: &Dictionary,
) -> ReferencedNames {
    let mut found = ReferencedNames::default();
    let mut visited: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<(Vec<u8>, Dictionary)> = VecDeque::new();
    queue.push_back((document.get_page_content(page_id), resources.clone()));

    while let Some((content, scope)) = queue.pop_front() {
        let Ok(content) = Content::decode(&content) else {
            continue;
        };
        for operation in &content.operations {
            match operation.operator.as_str() {
                "sh" => {
                    if let Some(name) = operation
                        .operands
                        .first()
                        .and_then(|operand| operand.as_name().ok())
                    {
                        found.shadings.insert(name.to_vec());
                    }
                }
                // Both `scn` and `SCN` name a pattern in their LAST operand; an
                // uncolored pattern is preceded by its colour components.
                "scn" | "SCN" => {
                    if let Some(name) = operation
                        .operands
                        .last()
                        .and_then(|operand| operand.as_name().ok())
                    {
                        found.patterns.insert(name.to_vec());
                    }
                }
                "Do" => {
                    if let Some(name) = operation
                        .operands
                        .first()
                        .and_then(|operand| operand.as_name().ok())
                        && let Some((form_id, form_content, form_resources)) =
                            form_xobject_content(document, &scope, name)
                        && visited.insert(form_id)
                    {
                        queue.push_back((form_content, form_resources));
                    }
                }
                _ => {}
            }
        }
        // A pattern that survived can itself paint with further patterns or
        // shadings; follow those so the keep-set is transitively closed.
        for name in found.patterns.clone() {
            if let Some((pattern_id, pattern_content, pattern_resources)) =
                named_stream_content(document, &scope, b"Pattern", &name)
                && visited.insert(pattern_id)
            {
                queue.push_back((pattern_content, pattern_resources));
            }
        }
    }
    found
}

fn form_xobject_content(
    document: &Document,
    resources: &Dictionary,
    name: &[u8],
) -> Option<(ObjectId, Vec<u8>, Dictionary)> {
    let (object_id, content, dictionary, stream_resources) =
        named_stream(document, resources, b"XObject", name)?;
    dictionary
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_ok_and(|subtype| subtype == b"Form")
        .then_some((object_id, content, stream_resources))
}

fn named_stream_content(
    document: &Document,
    resources: &Dictionary,
    category: &[u8],
    name: &[u8],
) -> Option<(ObjectId, Vec<u8>, Dictionary)> {
    let (object_id, content, _, stream_resources) =
        named_stream(document, resources, category, name)?;
    Some((object_id, content, stream_resources))
}

fn named_stream(
    document: &Document,
    resources: &Dictionary,
    category: &[u8],
    name: &[u8],
) -> Option<(ObjectId, Vec<u8>, Dictionary, Dictionary)> {
    let category = resource_dictionary(document, resources.get(category).ok()?)?;
    let object_id = category.get(name).ok()?.as_reference().ok()?;
    let stream = document.get_object(object_id).ok()?.as_stream().ok()?;
    let content = stream.decompressed_content().ok()?;
    let stream_resources = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|nested| resource_dictionary(document, nested))
        .unwrap_or_default();
    Some((object_id, content, stream.dict.clone(), stream_resources))
}

/// Rewrites `resources[category]` inline, keeping only the named entries.
///
/// The category dictionary is inlined rather than mutated through its reference so
/// a dictionary shared with an untouched page is never altered in place.
fn retain_named_resources(
    document: &Document,
    resources: &mut Dictionary,
    category: &[u8],
    keep: &HashSet<Vec<u8>>,
) {
    let Some(existing) = resources
        .get(category)
        .ok()
        .and_then(|entries| resource_dictionary(document, entries))
    else {
        return;
    };
    let mut retained = Dictionary::new();
    for (name, value) in &existing {
        if keep.contains(name.as_slice()) {
            retained.set(name.clone(), value.clone());
        }
    }
    if retained.is_empty() {
        resources.remove(category);
    } else {
        resources.set(category.to_vec(), Object::Dictionary(retained));
    }
}

fn resource_dictionary(document: &Document, object: &Object) -> Option<Dictionary> {
    match object {
        Object::Reference(object_id) => document.get_dictionary(*object_id).ok().cloned(),
        Object::Dictionary(dictionary) => Some(dictionary.clone()),
        _ => None,
    }
}

fn add_cropped_page(
    document: &mut Document,
    parent_id: lopdf::ObjectId,
    form: PageForm,
    bounds: DetectedCropBounds,
) -> lopdf::ObjectId {
    let content = format!(
        "q {} {} {} {} re W n /Fm0 Do Q\n",
        bounds.x, bounds.y, bounds.width, bounds.height
    );
    let content_id = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => parent_id,
        "MediaBox" => vec![
            bounds.x.into(),
            bounds.y.into(),
            (bounds.x + bounds.width).into(),
            (bounds.y + bounds.height).into(),
        ],
        "Resources" => dictionary! {
            "XObject" => dictionary! { "Fm0" => form.id },
        },
        "Contents" => content_id,
    })
}

fn load_document(path: &Path, filename: &str) -> Result<Document, CropError> {
    Document::load(path).map_err(|source| CropError::ReadPdf {
        filename: filename.to_owned(),
        source,
    })
}
