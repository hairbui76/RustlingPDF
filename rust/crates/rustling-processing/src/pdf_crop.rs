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

/// Maximum resource-scope chain depth, and maximum content streams walked, while
/// working out which patterns and shadings surviving content still paints with.
/// Both bound the work an adversarial file can force. Exhausting either abandons
/// pruning for that page and keeps every entry: retaining a secret is bad, but
/// pruning something still painted corrupts the document, so the bound fails
/// towards the recoverable side.
const MAX_RESOURCE_SCOPE_DEPTH: usize = 32;
const MAX_WALKED_CONTENT_STREAMS: usize = 4096;

/// Drops `/Pattern` and `/Shading` resource entries that no surviving mark paints
/// with.
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
        let Some(referenced) = referenced_pattern_and_shading(document, page_id, &resources) else {
            continue;
        };
        retain_referenced_resources(document, &mut resources, b"Pattern", &referenced.patterns);
        retain_referenced_resources(document, &mut resources, b"Shading", &referenced.shadings);
        document
            .get_dictionary_mut(page_id)?
            .set("Resources", Object::Dictionary(resources));
    }
    Ok(())
}

/// Identifies one resource entry of the page's `/Pattern` or `/Shading` dictionary.
///
/// Keying on the resolved object — not on the bare resource name — is what makes
/// this safe across scopes. Names are scope-local: a page and a Form `XObject` it
/// paints routinely both call their own, different patterns `/P0`, so a name-keyed
/// keep-set would let a reference made inside the form retain the page's unrelated
/// entry, which is exactly the leak this pruning exists to close.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ResourceRef {
    Indirect(ObjectId),
    /// A directly embedded entry in the page's own resource dictionary. Such an
    /// object has no id to key on, and nothing outside the page's scope can name
    /// it, so the page-local name identifies it unambiguously.
    PageDirect(Vec<u8>),
}

#[derive(Default)]
struct ReferencedResources {
    patterns: HashSet<ResourceRef>,
    shadings: HashSet<ResourceRef>,
}

/// A resource-resolution scope: dictionaries innermost first, the page's own
/// resources always last.
///
/// A Form `XObject` without its own `/Resources` inherits the enclosing scope
/// rather than resolving against nothing — that is legal per ISO 32000-1 §8.10.1,
/// and treating such a form as having no resources both loses the chain (its `Do`
/// targets stop resolving) and silently prunes patterns its content still paints
/// with. A form that *does* carry `/Resources` has them searched first, with the
/// enclosing scope behind: strictly self-contained forms are unaffected, and a
/// form whose dictionary is missing an entry still resolves the way real viewers
/// resolve it instead of losing a live reference.
type ResourceScope = Vec<Dictionary>;

/// Collects every `/Pattern` and `/Shading` entry of the page's own resources that
/// surviving content still paints with: the page's streams, the Form `XObjects` they
/// invoke (recursively, honouring resource inheritance), and the content of any
/// pattern that is itself still referenced.
///
/// Returns `None` when the walk exceeds its bounds, which the caller reads as
/// "keep everything".
fn referenced_pattern_and_shading(
    document: &Document,
    page_id: ObjectId,
    page_resources: &Dictionary,
) -> Option<ReferencedResources> {
    let mut found = ReferencedResources::default();
    let mut visited: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<(Vec<u8>, ResourceScope)> = VecDeque::new();
    let mut budget = MAX_WALKED_CONTENT_STREAMS;
    queue.push_back((
        document.get_page_content(page_id),
        vec![page_resources.clone()],
    ));

    while let Some((content, scope)) = queue.pop_front() {
        budget = budget.checked_sub(1)?;
        let Ok(content) = Content::decode(&content) else {
            continue;
        };
        for operation in &content.operations {
            match operation.operator.as_str() {
                "sh" => {
                    if let Some(name) = operand_name(operation.operands.first())
                        && let Some((key, _)) = resolve_resource(document, &scope, b"Shading", name)
                    {
                        found.shadings.insert(key);
                    }
                }
                // Both `scn` and `SCN` name a pattern in their LAST operand; an
                // uncolored pattern is preceded by its colour components.
                "scn" | "SCN" => {
                    if let Some(name) = operand_name(operation.operands.last())
                        && let Some((key, value)) =
                            resolve_resource(document, &scope, b"Pattern", name)
                    {
                        found.patterns.insert(key);
                        // A surviving pattern's own content can paint with further
                        // patterns and shadings; follow it so the keep-set is
                        // transitively closed.
                        enqueue_stream_content(
                            document,
                            &scope,
                            &value,
                            None,
                            &mut visited,
                            &mut queue,
                        )?;
                    }
                }
                "Do" => {
                    if let Some(name) = operand_name(operation.operands.first())
                        && let Some((_, value)) =
                            resolve_resource(document, &scope, b"XObject", name)
                    {
                        enqueue_stream_content(
                            document,
                            &scope,
                            &value,
                            Some(b"Form"),
                            &mut visited,
                            &mut queue,
                        )?;
                    }
                }
                _ => {}
            }
        }
    }
    Some(found)
}

fn operand_name(operand: Option<&Object>) -> Option<&[u8]> {
    operand.and_then(|operand| operand.as_name().ok())
}

/// Resolves `name` in `category` through the scope chain, innermost first, and
/// reports which object it landed on.
fn resolve_resource(
    document: &Document,
    scope: &ResourceScope,
    category: &[u8],
    name: &[u8],
) -> Option<(ResourceRef, Object)> {
    for (depth, resources) in scope.iter().enumerate() {
        let Some(entries) = resources
            .get(category)
            .ok()
            .and_then(|entries| resource_dictionary(document, entries))
        else {
            continue;
        };
        let Ok(value) = entries.get(name) else {
            continue;
        };
        let key = match value {
            Object::Reference(object_id) => ResourceRef::Indirect(*object_id),
            // Only the page's own dictionary is ever pruned, so a directly
            // embedded entry found in a nested scope is a different object and
            // keeps nothing on the page alive.
            _ if depth + 1 == scope.len() => ResourceRef::PageDirect(name.to_vec()),
            _ => return None,
        };
        return Some((key, value.clone()));
    }
    None
}

/// Queues a referenced stream's content for scanning, under the scope its own
/// `/Resources` establish — or, when it has none, under the enclosing scope it
/// inherits.
fn enqueue_stream_content(
    document: &Document,
    scope: &ResourceScope,
    value: &Object,
    required_subtype: Option<&[u8]>,
    visited: &mut HashSet<ObjectId>,
    queue: &mut VecDeque<(Vec<u8>, ResourceScope)>,
) -> Option<()> {
    let Object::Reference(object_id) = value else {
        // A pattern or form must be an indirect stream object; anything else has
        // no content to follow.
        return Some(());
    };
    if !visited.insert(*object_id) {
        return Some(());
    }
    let Ok(stream) = document.get_object(*object_id).and_then(Object::as_stream) else {
        return Some(());
    };
    if let Some(subtype) = required_subtype
        && !stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|actual| actual == subtype)
    {
        return Some(());
    }
    let Ok(content) = stream.decompressed_content() else {
        return Some(());
    };
    let child = match stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|resources| resource_dictionary(document, resources))
    {
        Some(own) => {
            if scope.len() >= MAX_RESOURCE_SCOPE_DEPTH {
                return None;
            }
            let mut chain = Vec::with_capacity(scope.len().saturating_add(1));
            chain.push(own);
            chain.extend(scope.iter().cloned());
            chain
        }
        None => scope.clone(),
    };
    queue.push_back((content, child));
    Some(())
}

/// Rewrites `resources[category]` inline, keeping only the entries `keep` names.
///
/// The category dictionary is inlined rather than mutated through its reference so
/// a dictionary shared with an untouched page is never altered in place.
fn retain_referenced_resources(
    document: &Document,
    resources: &mut Dictionary,
    category: &[u8],
    keep: &HashSet<ResourceRef>,
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
        let key = match value {
            Object::Reference(object_id) => ResourceRef::Indirect(*object_id),
            _ => ResourceRef::PageDirect(name.clone()),
        };
        if keep.contains(&key) {
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
