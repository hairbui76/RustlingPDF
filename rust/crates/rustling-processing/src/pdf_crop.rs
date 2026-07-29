use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    path::Path,
};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content, dictionary};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::{
    pdf_page_geometry::{PageForm, page_form, replace_page_tree},
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
    #[error(
        "could not establish which patterns and shadings the cropped content still uses, so \
         out-of-crop data cannot be removed safely ({details}); retry with \
         removeDataOutsideCrop=false to crop without the removal guarantee"
    )]
    ResourceAnalysis { details: String },
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
/// Bounds on the resource-reachability walk.
///
/// These exist to stop pathological nesting, not to ration ordinary work: the
/// walk visits each (stream, scope chain) pair once and carries scopes by
/// identity rather than by value, so its cost is linear in the document. Real
/// files sit orders of magnitude below both. Exceeding either is reported as an
/// error and no file is produced — see [`prune_unreferenced_pattern_and_shading`]
/// for why the walk never fails open.
const MAX_RESOURCE_SCOPE_DEPTH: usize = 256;
const MAX_WALKED_SCOPED_STREAMS: usize = 100_000;

/// Pins the property that makes the resource walk cheap: a scope is carried by
/// **identity**, never by value.
///
/// An earlier revision carried `Vec<Dictionary>` and cloned the page's whole
/// `/Resources` — including its N-entry `/XObject` sub-dictionary — once per
/// inheriting form. That is quadratic in both time and memory: a 766 KB upload
/// with 4000 forms took 15.9 s and 1.8 GB of resident memory, all of it while
/// holding the global `PDFium` lock. If this assertion ever fails because a
/// dictionary was put back into the scope type, that regression is back.
const _: () = assert!(size_of::<ResourcesId>() <= 2 * size_of::<u32>() + size_of::<usize>());

/// The `/Pattern` and `/Shading` resource categories this pruning considers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ResourceCategory {
    Pattern,
    Shading,
}

impl ResourceCategory {
    const ALL: [Self; 2] = [Self::Pattern, Self::Shading];

    fn key(self) -> &'static [u8] {
        match self {
            Self::Pattern => b"Pattern",
            Self::Shading => b"Shading",
        }
    }
}

/// Identifies one `/Resources` dictionary.
///
/// A `/Resources` value that is an indirect reference may be **shared** — a Form
/// `XObject` pointing at the very dictionary its page uses is spec-legal and real
/// generators emit it. Identifying such a dictionary by its object id, and
/// filtering that one object once, is what keeps every holder of it consistent.
/// Writing a filtered *copy* onto the page instead leaves the form still pointing
/// at the unfiltered original, which keeps the secret reachable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ResourcesId {
    Shared(ObjectId),
    /// Written directly inside this page or form object, so unshared.
    InlineIn(ObjectId),
}

/// Identifies one `/Pattern` or `/Shading` sub-dictionary, which can itself be
/// shared by object id independently of the `/Resources` dictionaries holding it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CategoryId {
    Shared(ObjectId),
    InlineIn(ResourcesId, ResourceCategory),
}

/// One resource entry that surviving content still paints with.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum LiveResource {
    /// The entry is an indirect reference. The object it names identifies it no
    /// matter which dictionary or scope reaches it, so a page and a form that
    /// both call their own different patterns `/P0` cannot be confused.
    Object(ObjectId),
    /// The entry is written directly inside a category dictionary, so it is
    /// unshared and that dictionary plus the name identify it.
    Inline(CategoryId, Vec<u8>),
}

/// Drops `/Pattern` and `/Shading` resource entries that no surviving mark paints
/// with.
///
/// `PDFium` rebuilds `/Font`, `/ExtGState`, and `/XObject` when it regenerates a
/// page — which is why a removed image or Form `XObject` really does leave the
/// file — but it leaves `/Pattern` and `/Shading` exactly as it found them. Since
/// [`page_form`] copies the page's `/Resources` verbatim into the rebuilt page's
/// Form `XObject`, an out-of-crop mark painted with a tiling pattern or a shading
/// would keep its whole subtree reachable, and `prune_objects` would rightly
/// preserve it.
///
/// The live set is collected across **every** page first and only then applied, so
/// an entry any page still paints survives, and each dictionary — including one
/// shared between a page and a form — is filtered exactly once, in place.
///
/// # Errors
///
/// Returns [`CropError::ResourceAnalysis`] when the walk cannot establish what is
/// live: an undecodable content stream, or nesting beyond the bounds above. The
/// walk never falls back to keeping everything, for the same reason this endpoint
/// returns `501` instead of silently cropping without removal when `PDFium` is
/// missing — answering a request to delete data with a file that still contains
/// it, and a `200` that looks identical to a clean one, is the one outcome the
/// caller cannot detect or recover from. A caller that would rather have the file
/// than the guarantee can retry with `removeDataOutsideCrop=false`.
fn prune_unreferenced_pattern_and_shading(document: &mut Document) -> Result<(), CropError> {
    let mut walk = ResourceWalk::new(document);
    for page_id in document.get_pages().into_values() {
        walk.walk_page(page_id)?;
    }
    let ResourceWalk {
        live, dictionaries, ..
    } = walk;
    for resources_id in dictionaries {
        for category in ResourceCategory::ALL {
            retain_live_entries(document, resources_id, category, &live);
        }
    }
    Ok(())
}

/// Reachability walk over the content that survived removal.
struct ResourceWalk<'a> {
    document: &'a Document,
    live: HashSet<LiveResource>,
    /// Every `/Resources` dictionary reached, and therefore eligible for filtering.
    dictionaries: BTreeSet<ResourcesId>,
    /// Streams already walked, keyed by scope chain as well as identity: the same
    /// form reached under a different chain can resolve its names to different
    /// entries, so identity alone would miss references.
    visited: HashSet<(ObjectId, Vec<ResourcesId>)>,
    /// Pending work. The walk is iterative on purpose: a Form `XObject` that
    /// inherits its enclosing scope does not grow the scope chain, so recursing
    /// per link would be bounded only by how many forms the file contains — a
    /// 6000-link chain in a ~500 KB upload overflowed the thread stack and
    /// aborted the process.
    queue: VecDeque<(Vec<u8>, Vec<ResourcesId>)>,
    budget: usize,
}

impl<'a> ResourceWalk<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            live: HashSet::new(),
            dictionaries: BTreeSet::new(),
            visited: HashSet::new(),
            queue: VecDeque::new(),
            budget: MAX_WALKED_SCOPED_STREAMS,
        }
    }

    fn walk_page(&mut self, page_id: ObjectId) -> Result<(), CropError> {
        let Some(resources_id) = page_resources_id(self.document, page_id) else {
            return Ok(());
        };
        let content = self.document.get_page_content(page_id);
        self.queue.push_back((content, vec![resources_id]));
        while let Some((content, chain)) = self.queue.pop_front() {
            self.scan(&content, &chain)?;
        }
        Ok(())
    }

    fn scan(&mut self, content: &[u8], chain: &[ResourcesId]) -> Result<(), CropError> {
        self.budget = self
            .budget
            .checked_sub(1)
            .ok_or_else(|| CropError::ResourceAnalysis {
                details: format!(
                    "more than {MAX_WALKED_SCOPED_STREAMS} nested content streams reference \
                     patterns or shadings"
                ),
            })?;
        self.dictionaries.extend(chain.iter().copied());
        let content = Content::decode(content).map_err(|source| CropError::ResourceAnalysis {
            details: format!("a content stream could not be decoded: {source}"),
        })?;
        for operation in &content.operations {
            match operation.operator.as_str() {
                "sh" => {
                    if let Some(name) = operand_name(operation.operands.first()) {
                        self.record(chain, ResourceCategory::Shading, name);
                    }
                }
                // Both `scn` and `SCN` name a pattern in their LAST operand; an
                // uncolored pattern is preceded by its colour components.
                "scn" | "SCN" => {
                    if let Some(name) = operand_name(operation.operands.last())
                        && let Some(value) = self.record(chain, ResourceCategory::Pattern, name)
                    {
                        // A surviving pattern's own content can paint with further
                        // patterns and shadings; follow it.
                        self.enqueue(chain, &value, None)?;
                    }
                }
                "Do" => {
                    if let Some(name) = operand_name(operation.operands.first())
                        && let Some(value) = resolve_xobject(self.document, chain, name)
                    {
                        self.enqueue(chain, &value, Some(b"Form"))?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Resolves `name` through the scope chain and marks what it lands on live.
    fn record(
        &mut self,
        chain: &[ResourcesId],
        category: ResourceCategory,
        name: &[u8],
    ) -> Option<Object> {
        let (live, value) = resolve_resource(self.document, chain, category, name)?;
        self.live.insert(live);
        Some(value)
    }

    /// Queues a referenced stream under the scope its own `/Resources` establish —
    /// or, when it has none, under the enclosing scope it inherits (ISO 32000-1
    /// §8.10.1).
    fn enqueue(
        &mut self,
        chain: &[ResourcesId],
        value: &Object,
        required_subtype: Option<&[u8]>,
    ) -> Result<(), CropError> {
        // A pattern or form is an indirect stream object; anything else has no
        // content to follow.
        let Object::Reference(object_id) = value else {
            return Ok(());
        };
        let Ok(stream) = self
            .document
            .get_object(*object_id)
            .and_then(Object::as_stream)
        else {
            return Ok(());
        };
        if let Some(subtype) = required_subtype
            && !stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|actual| actual == subtype)
        {
            return Ok(());
        }
        let child = match stream.dict.get(b"Resources") {
            Ok(resources) => {
                if chain.len() >= MAX_RESOURCE_SCOPE_DEPTH {
                    return Err(CropError::ResourceAnalysis {
                        details: format!(
                            "resource scopes nest more than {MAX_RESOURCE_SCOPE_DEPTH} deep"
                        ),
                    });
                }
                let mut child = Vec::with_capacity(chain.len().saturating_add(1));
                child.push(resources_id(*object_id, resources));
                child.extend_from_slice(chain);
                child
            }
            // No `/Resources` of its own: inherit the enclosing scope rather than
            // resolving against nothing, which would lose both its own `Do`
            // targets and the patterns its content paints with.
            Err(_) => chain.to_vec(),
        };
        if !self.visited.insert((*object_id, child.clone())) {
            return Ok(());
        }
        let content =
            stream
                .decompressed_content()
                .map_err(|source| CropError::ResourceAnalysis {
                    details: format!("a content stream could not be decompressed: {source}"),
                })?;
        self.queue.push_back((content, child));
        Ok(())
    }
}

fn operand_name(operand: Option<&Object>) -> Option<&[u8]> {
    operand.and_then(|operand| operand.as_name().ok())
}

fn resources_id(owner: ObjectId, resources: &Object) -> ResourcesId {
    match resources {
        Object::Reference(object_id) => ResourcesId::Shared(*object_id),
        _ => ResourcesId::InlineIn(owner),
    }
}

/// The `/Resources` a page uses, following `/Parent` inheritance to whichever
/// node actually carries them.
fn page_resources_id(document: &Document, page_id: ObjectId) -> Option<ResourcesId> {
    let mut object_id = page_id;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(object_id) {
            return None;
        }
        let dictionary = document.get_dictionary(object_id).ok()?;
        if let Ok(resources) = dictionary.get(b"Resources") {
            return Some(resources_id(object_id, resources));
        }
        object_id = dictionary.get(b"Parent").ok()?.as_reference().ok()?;
    }
}

fn resources_dictionary(document: &Document, id: ResourcesId) -> Option<&Dictionary> {
    match id {
        ResourcesId::Shared(object_id) => document.get_dictionary(object_id).ok(),
        ResourcesId::InlineIn(owner) => owner_resources(document, owner),
    }
}

fn owner_resources(document: &Document, owner: ObjectId) -> Option<&Dictionary> {
    let resources = match document.get_object(owner).ok()? {
        Object::Dictionary(dictionary) => dictionary.get(b"Resources").ok()?,
        Object::Stream(stream) => stream.dict.get(b"Resources").ok()?,
        _ => return None,
    };
    resources.as_dict().ok()
}

fn owner_resources_mut(document: &mut Document, owner: ObjectId) -> Option<&mut Dictionary> {
    let resources = match document.objects.get_mut(&owner)? {
        Object::Dictionary(dictionary) => dictionary.get_mut(b"Resources").ok()?,
        Object::Stream(stream) => stream.dict.get_mut(b"Resources").ok()?,
        _ => return None,
    };
    resources.as_dict_mut().ok()
}

/// Locates a category sub-dictionary within a `/Resources` dictionary.
fn category_dictionary(
    document: &Document,
    resources_id: ResourcesId,
    category: ResourceCategory,
) -> Option<(CategoryId, &Dictionary)> {
    let resources = resources_dictionary(document, resources_id)?;
    match resources.get(category.key()).ok()? {
        Object::Reference(object_id) => Some((
            CategoryId::Shared(*object_id),
            document.get_dictionary(*object_id).ok()?,
        )),
        Object::Dictionary(entries) => {
            Some((CategoryId::InlineIn(resources_id, category), entries))
        }
        _ => None,
    }
}

/// Resolves `name` through the scope chain, innermost first, and reports which
/// entry it landed on.
fn resolve_resource(
    document: &Document,
    chain: &[ResourcesId],
    category: ResourceCategory,
    name: &[u8],
) -> Option<(LiveResource, Object)> {
    for resources_id in chain {
        let Some((category_id, entries)) = category_dictionary(document, *resources_id, category)
        else {
            continue;
        };
        let Ok(value) = entries.get(name) else {
            continue;
        };
        let live = match value {
            Object::Reference(object_id) => LiveResource::Object(*object_id),
            _ => LiveResource::Inline(category_id, name.to_vec()),
        };
        return Some((live, value.clone()));
    }
    None
}

fn resolve_xobject(document: &Document, chain: &[ResourcesId], name: &[u8]) -> Option<Object> {
    for resources_id in chain {
        let Some(resources) = resources_dictionary(document, *resources_id) else {
            continue;
        };
        let Some(xobjects) = resources
            .get(b"XObject")
            .ok()
            .and_then(|xobjects| match xobjects {
                Object::Reference(object_id) => document.get_dictionary(*object_id).ok(),
                Object::Dictionary(dictionary) => Some(dictionary),
                _ => None,
            })
        else {
            continue;
        };
        if let Ok(value) = xobjects.get(name) {
            return Some(value.clone());
        }
    }
    None
}

/// Filters one category dictionary in place, keeping only live entries.
///
/// Filtering the dictionary the holders actually point at — rather than writing a
/// filtered copy onto one of them — is what makes a `/Resources` dictionary shared
/// between a page and a form come out consistent.
fn retain_live_entries(
    document: &mut Document,
    resources_id: ResourcesId,
    category: ResourceCategory,
    live: &HashSet<LiveResource>,
) {
    let Some((category_id, entries)) = category_dictionary(document, resources_id, category) else {
        return;
    };
    let mut retained = Dictionary::new();
    for (name, value) in entries {
        let key = match value {
            Object::Reference(object_id) => LiveResource::Object(*object_id),
            _ => LiveResource::Inline(category_id, name.clone()),
        };
        if live.contains(&key) {
            retained.set(name.clone(), value.clone());
        }
    }
    if retained.len() == entries.len() {
        return;
    }
    match category_id {
        CategoryId::Shared(object_id) => {
            if let Some(Object::Dictionary(existing)) = document.objects.get_mut(&object_id) {
                *existing = retained;
            }
        }
        CategoryId::InlineIn(resources_id, category) => {
            let resources = match resources_id {
                ResourcesId::Shared(object_id) => document
                    .objects
                    .get_mut(&object_id)
                    .and_then(|object| object.as_dict_mut().ok()),
                ResourcesId::InlineIn(owner) => owner_resources_mut(document, owner),
            };
            if let Some(resources) = resources {
                if retained.is_empty() {
                    resources.remove(category.key());
                } else {
                    resources.set(category.key().to_vec(), Object::Dictionary(retained));
                }
            }
        }
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
