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
        "this PDF's Form XObject graph expands without bound ({details}), and removing \
         out-of-crop content requires expanding it; retry with removeDataOutsideCrop=false \
         to crop without removal"
    )]
    UnboundedFormExpansion { details: String },
    #[error("the crop worker did not finish: {0}")]
    Worker(String),
}

/// Stack for the thread the crop runs on.
///
/// `lopdf` traverses a document recursively, so the depth it needs is a property
/// of the uploaded file. `tokio` gives its blocking threads 2 MiB, and a real
/// 932 KB document with 1,353 forms needs between 2 and 4 MiB — measured by
/// sweeping the stack size, deterministically, on this exact file. Overflowing a
/// stack **aborts the process**, so an unauthenticated request took the whole
/// service down and every concurrent caller with it. That is why this is not left
/// to the ambient stack.
///
/// Neither PDFium nor the resource walk is involved: PDFium completes this file on
/// its own, the walk is iterative by construction, and the rebuild's traversal of
/// PDFium's output is what recurses.
///
/// This is **containment, not a proof**. A document nested deeper still overflows,
/// just further out; only an iterative traversal in `lopdf` would make it
/// impossible. 32 MiB is 16x the depth that was crashing and far beyond anything
/// in a thousand-document corpus.
const CROP_STACK_BYTES: usize = 32 * 1024 * 1024;

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
    // Scoped so the worker can borrow the paths; see [`CROP_STACK_BYTES`] for why
    // it does not simply run on the caller's stack.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(CROP_STACK_BYTES)
            .spawn_scoped(scope, || {
                crop_pdf_to_file_inner(input_path, filename, options, output_path)
            })
            .map_err(|error| CropError::Worker(error.to_string()))?
            .join()
            .map_err(|_| CropError::Worker("the crop worker panicked".to_owned()))?
    })
}

fn crop_pdf_to_file_inner(
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
        // Before PDFium, not after: the expansion happens inside it, and by the
        // time it has started there is nothing left to refuse with.
        form_expansion_within_bounds(&load_document(input_path, filename)?)?;
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
        prune_dead_resource_declarations(&mut document);
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

/// The expanded `Do` invocations a document may cost before removal is refused.
///
/// `PDFium` expands every form invocation when it regenerates a page, and it
/// bounds neither the recursion nor the fan out. ISO 32000-1 §8.10.1 forbids a
/// form from invoking itself directly or indirectly, so a cycle is already an
/// invalid document — but PDFium follows one anyway, and an acyclic graph can be
/// just as expensive: measured on this unauthenticated endpoint, a **1,982-byte**
/// upload whose six forms each invoke all six reached 45 GB resident and never
/// returned, and a 557 KB document fanning 60 ways across 8 levels reached 84 GB.
///
/// This is *not* something the resource walk's bounds can reach. The cost is
/// spent inside PDFium, before the walk runs at all — verified by disabling the
/// walk entirely and watching the same 45 GB. The only place to stop it is on the
/// way in.
///
/// The graph follows **executed** `Do` operators, because that is what PDFium
/// expands. Declarations alone are far too coarse: a Form XObject sharing its
/// page's `/Resources` dictionary — spec-legal, and something real generators
/// emit — declares *itself*, so every such document would look like a cycle and
/// be refused.
///
/// Where the operators cannot be read, the form's declared children are used
/// instead. That is the safe direction and it closes the obvious evasion: a
/// crafted file cannot hide its recursion behind a construct the content parser
/// stops at, because failing to parse is exactly what triggers the conservative
/// reading.
///
/// `removeDataOutsideCrop=false` never invokes PDFium and stays available — it
/// answers both of the documents above in under a fifth of a second.
const MAX_FORM_EXPANSION: u64 = 1_000_000;

/// Refuses a document whose Form `XObject` graph would expand past
/// [`MAX_FORM_EXPANSION`].
///
/// # Errors
///
/// Returns [`CropError::UnboundedFormExpansion`] for a cyclic graph, or one whose
/// expanded invocation count exceeds the bound.
fn form_expansion_within_bounds(document: &Document) -> Result<(), CropError> {
    enum Step {
        Enter(ObjectId),
        Exit(ObjectId),
    }
    let refuse = |details: &str| CropError::UnboundedFormExpansion {
        details: details.to_owned(),
    };
    let mut cost: std::collections::HashMap<ObjectId, u64> = std::collections::HashMap::new();
    let mut on_path: HashSet<ObjectId> = HashSet::new();
    for page_id in document.get_pages().into_values() {
        let scope = page_resources_id(document, page_id);
        let roots = scope
            .and_then(|scope| {
                invoked_form_ids(document, &document.get_page_content(page_id), scope)
            })
            .unwrap_or_else(|| declared_form_ids(document, scope));
        let mut stack: Vec<Step> = roots.iter().rev().map(|id| Step::Enter(*id)).collect();
        while let Some(step) = stack.pop() {
            match step {
                Step::Enter(id) => {
                    if cost.contains_key(&id) {
                        continue;
                    }
                    if !on_path.insert(id) {
                        return Err(refuse(
                            "a Form XObject invokes itself, directly or indirectly",
                        ));
                    }
                    stack.push(Step::Exit(id));
                    for child in form_children(document, id) {
                        stack.push(Step::Enter(child));
                    }
                }
                Step::Exit(id) => {
                    on_path.remove(&id);
                    let mut expanded: u64 = 1;
                    for child in form_children(document, id) {
                        expanded = expanded.saturating_add(cost.get(&child).copied().unwrap_or(1));
                    }
                    if expanded >= MAX_FORM_EXPANSION {
                        return Err(refuse(
                            "one Form XObject expands to more than a million invocations",
                        ));
                    }
                    cost.insert(id, expanded);
                }
            }
        }
        let page_cost = roots.iter().fold(0_u64, |total, id| {
            total.saturating_add(cost.get(id).copied().unwrap_or(1))
        });
        if page_cost >= MAX_FORM_EXPANSION {
            return Err(refuse(
                "one page expands to more than a million form invocations",
            ));
        }
    }
    Ok(())
}

/// The Form `XObject` object ids a resource scope declares.
fn declared_form_ids(document: &Document, resources_id: Option<ResourcesId>) -> Vec<ObjectId> {
    let Some(resources_id) = resources_id else {
        return Vec::new();
    };
    let Some((_, entries)) = category_dictionary(document, resources_id, ResourceCategory::XObject)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(_, value)| form_object_id(document, value))
        .collect()
}

/// The forms a form actually invokes, falling back to what it declares when its
/// operators cannot be read.
fn form_children(document: &Document, form_id: ObjectId) -> Vec<ObjectId> {
    let Ok(Object::Stream(stream)) = document.get_object(form_id) else {
        return Vec::new();
    };
    let scope = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|resources| scope_id(document, Some(form_id), resources));
    let declared = declared_form_ids(document, scope);
    if declared.is_empty() {
        return Vec::new();
    }
    let Some(scope) = scope else {
        return declared;
    };
    let Ok(content) = stream.decompressed_content() else {
        return declared;
    };
    invoked_form_ids(document, &content, scope).unwrap_or(declared)
}

/// The forms a content stream's `Do` operators name, resolved in `scope`.
///
/// `None` means the stream could not be read closely enough to be sure — an
/// operator stream that does not parse in full, or a name that resolves through a
/// scope inherited from whoever invoked this stream, which is not knowable from
/// here. Callers fall back to the declared set.
fn invoked_form_ids(
    document: &Document,
    content: &[u8],
    scope: ResourcesId,
) -> Option<Vec<ObjectId>> {
    let decoded = Content::decode_strict(content).ok()?;
    let mut invoked = Vec::new();
    for operation in &decoded.operations {
        if operation.operator != "Do" {
            continue;
        }
        let name = operand_name(&operation.operands)?;
        let (_, value) = resolve_resource(document, &[scope], ResourceCategory::XObject, name)?;
        if let Some(object_id) = form_object_id(document, &value) {
            invoked.push(object_id);
        }
    }
    Some(invoked)
}

/// The object id of a value that is a Form `XObject`, following references.
fn form_object_id(document: &Document, value: &Object) -> Option<ObjectId> {
    let (object_id, object) = document.dereference(value).ok()?;
    let stream = object.as_stream().ok()?;
    if resolved_name(document, stream.dict.get(b"Subtype").ok()) != Some(b"Form".as_slice()) {
        return None;
    }
    object_id
}

/// Bounds on the resource-reachability walk.
///
/// The byte budget counts **decoded content bytes**, not streams. A stream reached
/// under two different scope chains is scanned twice, so a file that makes many
/// forms reachable under many scopes re-decodes the same bodies quadratically —
/// cost a stream counter cannot see. Counting scans let a 532 KB upload sit under
/// a 100 000-scan budget while burning 68 s and 57 MB; counting bytes prices that
/// shape correctly.
///
/// Both bounds exist to stop pathological shapes, not to ration ordinary work.
/// Exceeding either stops the walk short; what it did not read it does not prune
/// — see [`prune_dead_resource_declarations`] for why that is the safe direction.
const MAX_RESOURCE_SCOPE_DEPTH: usize = 256;
const MAX_SCANNED_CONTENT_BYTES: u64 = 256 * 1024 * 1024;

/// Caps on the (stream, scope-chain) pairs the walk will track.
///
/// The byte budget prices *scanning*; it does not price *remembering*. `visited`
/// and the queue grow one entry per pair, and a pair's memory cost is its chain
/// depth, so a handful of cross-declaring forms generates chains combinatorially
/// while charging almost no bytes. Measured on an unauthenticated endpoint: a
/// **1,982-byte** upload with six cross-declaring forms took the server down in
/// 26 s, and left unbounded it reached 84 GB resident. Bytes cannot see that
/// shape — only a cap on the pairs themselves can.
///
/// Both bounds are needed. The pair count alone would still allow
/// `MAX_SCOPED_STREAMS * MAX_RESOURCE_SCOPE_DEPTH` links; the link budget prices
/// depth, which is what both the memory and the per-chain set work are actually
/// spent on. Every path that touches a chain is charged against the link budget,
/// including the give-up paths that admit no pair and decode no byte — those were
/// the last unpriced work, and they alone kept a six-form clique busy for five
/// minutes while sitting inside every other bound.
///
/// Real documents sit orders of magnitude below both: the deepest scope nesting
/// measured across a 1,000-document corpus is single digits, and the widest fan
/// out is tens of thousands of forms at depth one.
const MAX_SCOPED_STREAMS: usize = 50_000;
const MAX_SCOPE_LINKS: usize = 500_000;

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

/// The resource categories this pruning governs.
///
/// All five are pruned together, and that is what makes the walk's rule true by
/// construction rather than by enumeration: an entry no executed path names is
/// removed from the dictionary that declares it, so the stream behind it is
/// orphaned and `prune_objects` deletes it. Nothing is then left in the output
/// that the walk did not traverse.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ResourceCategory {
    Pattern,
    Shading,
    XObject,
    ExtGState,
    Font,
}

impl ResourceCategory {
    const ALL: [Self; 5] = [
        Self::Pattern,
        Self::Shading,
        Self::XObject,
        Self::ExtGState,
        Self::Font,
    ];

    fn key(self) -> &'static [u8] {
        match self {
            Self::Pattern => b"Pattern",
            Self::Shading => b"Shading",
            Self::XObject => b"XObject",
            Self::ExtGState => b"ExtGState",
            Self::Font => b"Font",
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

/// Drops every resource entry that no executed path names, and with it the
/// streams behind them.
///
/// `PDFium` rebuilds a page's own `/Font`, `/ExtGState`, and `/XObject` when it
/// regenerates the page, but it never touches `/Pattern` or `/Shading`, and it
/// never rewrites a Form `XObject`'s or a pattern's **own** `/Resources` at all.
/// Since [`page_form`] copies the page's `/Resources` verbatim into the rebuilt
/// page, anything still declared stays reachable and `prune_objects` rightly
/// preserves it — including a tiling pattern's text and images, a shading's
/// sampled-function data, and whole never-invoked forms.
///
/// So liveness is decided by **execution**: an entry survives only if some path
/// the operators actually take names it. Everything else is removed from the
/// dictionary that declares it and falls out of the file. That is what makes the
/// walk's rule — *traverse every content stream that will still be in the output
/// file* — true by construction. An earlier revision instead traversed
/// declarations to avoid dangling names, which left dead streams in the file: they
/// kept out-of-crop secrets alive through a form's own `/Resources` (which nothing
/// rewrote), and cross-declaring forms re-scanned under many scopes turned a
/// 532 KB upload into 68 s of work under the global `PDFium` lock.
///
/// The live set is collected across **every** page before anything is applied, so
/// an entry any page still uses survives, and each dictionary — including one
/// shared between a page and a form — is filtered exactly once, in place.
///
/// # Removal is best effort, and errs towards keeping
///
/// Deciding liveness means parsing content streams, and a content stream can be
/// unreadable: `lopdf`'s content parser stops at constructs it does not accept,
/// and a stream's filters can fail to decode. The walk therefore uses
/// [`Content::decode_strict`], which reports a partial parse instead of returning
/// the prefix it managed to read, and where a stream cannot be read in full it
/// **preserves** every resource scope that stream could have resolved a name in.
/// Removal happens where the content can be read and the entry proven
/// unreferenced; everywhere else the declarations are left exactly as they were.
///
/// The direction is deliberate, and it is the opposite of what an earlier
/// revision chose. Over-deletion destroys the caller's document silently: judging
/// the resources of an unread stream dead, while the stream itself is copied to
/// the output verbatim and still names them, stripped `/Font` and `/XObject` from
/// every page of a real 1.1 MB file — 77% of the page's ink — under a `200` that
/// looks exactly like a clean one. Under-deletion leaves data outside the crop
/// box recoverable, which is bounded (`PDFium` has already deleted the marks
/// themselves; what survives is the resource declarations behind them),
/// inspectable in the returned file, and documented in `contracts/crop.md`.
/// Refusing such documents with `422` was tried and is worse still: a document
/// every viewer renders must still crop.
fn prune_dead_resource_declarations(document: &mut Document) {
    let mut walk = ResourceWalk::new(document);
    for page_id in document.get_pages().into_values() {
        if walk.abandoned {
            break;
        }
        walk.walk_page(page_id);
    }
    let ResourceWalk {
        live,
        dictionaries,
        preserved,
        mut protected,
        abandoned,
        ..
    } = walk;
    if abandoned {
        tracing::warn!(
            target: "rustling_processing::crop",
            event = "crop_resource_pruning_skipped",
            "kept every resource declaration: the reachability walk exceeded its work bound, so \
             what it did not read could not be proven unreferenced"
        );
        return;
    }
    // A preserved scope's category dictionaries must survive whole. Skipping the
    // scope is not enough on its own: a `/Pattern` or `/Font` sub-dictionary is
    // routinely an indirect object that a second, fully readable scope also points
    // at, and filtering it *there* would strip entries the unreadable stream still
    // names. Protection therefore travels with the object, not with the holder.
    for resources_id in &preserved {
        for category in ResourceCategory::ALL {
            if let Some((category_id, _)) = category_dictionary(document, *resources_id, category) {
                protected.insert(category_id);
            }
        }
    }
    if !protected.is_empty() {
        tracing::warn!(
            target: "rustling_processing::crop",
            event = "crop_resource_pruning_partial",
            preserved_scopes = preserved.len(),
            protected_dictionaries = protected.len(),
            "kept some resource declarations: content the parser could not read in full may \
             still name them, so removal was skipped for those scopes"
        );
    }
    for resources_id in dictionaries {
        for category in ResourceCategory::ALL {
            retain_live_entries(document, resources_id, category, &live, &protected);
        }
    }
}

/// One queued unit of pending work.
///
/// A page's joined content has no object of its own, so it is carried by value —
/// but only one page is in flight at a time. Everything else is carried as an
/// identity and decompressed when it is popped, which is what keeps the queue's
/// size proportional to the pair caps rather than to the decompressed size of
/// everything reachable.
enum Pending {
    PageContent(Vec<u8>),
    Stream(ObjectId),
}

/// Reachability walk over the content that survived removal.
///
/// Annotation appearance (`/AP`) streams are deliberately **not** walked. That is
/// safe only because [`add_cropped_page`] builds each page without an `/Annots`
/// entry, so no appearance stream is part of the surviving content and none can be
/// left naming a pruned resource. `rebuilt_pages_carry_no_annotations` in
/// `tests/crop_endpoint.rs` pins that invariant: if annotations are ever
/// preserved, that test fails and this walk must start following `/AP`.
struct ResourceWalk<'a> {
    document: &'a Document,
    live: HashSet<LiveResource>,
    /// Every `/Resources` dictionary reached, and therefore eligible for filtering.
    dictionaries: BTreeSet<ResourcesId>,
    /// Scopes some stream that could not be read in full might have resolved a
    /// name in. Nothing they declare is removed.
    preserved: BTreeSet<ResourcesId>,
    /// Category dictionaries protected by object, which is what makes preservation
    /// survive sharing: a protected scope's `/Pattern` sub-dictionary can be the
    /// same object a readable scope declares, and filtering it there would strip
    /// the protected scope's entries out from under it. Also carries the one scope
    /// shape that has no [`ResourcesId`] at all — a `/Resources` dictionary written
    /// inline inside a directly embedded object, which has no id to key on.
    protected: HashSet<CategoryId>,
    /// Streams already walked, keyed by scope chain as well as identity: the same
    /// form reached under a different chain can resolve its names to different
    /// entries, so identity alone would miss references.
    visited: HashSet<(ObjectId, Vec<ResourcesId>)>,
    /// Pending work. The walk is iterative on purpose: a Form `XObject` that
    /// inherits its enclosing scope does not grow the scope chain, so recursing
    /// per link would be bounded only by how many forms the file contains — a
    /// 6000-link chain in a ~500 KB upload overflowed the thread stack and
    /// aborted the process.
    ///
    /// It queues **identities**, not bodies: an earlier revision pushed a fully
    /// decompressed copy of every reachable stream and only charged the byte
    /// budget when it was popped again, so the queue itself was the amplifier.
    /// Decompressing at pop keeps the queue's cost proportional to the pair caps.
    queue: VecDeque<(Pending, Vec<ResourcesId>)>,
    budget: u64,
    /// Scope links held across `visited`, which is what the pair caps price.
    links: usize,
    /// The walk ran out of work budget, so what it never reached is unknown and
    /// nothing may be pruned anywhere.
    abandoned: bool,
}

impl<'a> ResourceWalk<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            live: HashSet::new(),
            dictionaries: BTreeSet::new(),
            preserved: BTreeSet::new(),
            protected: HashSet::new(),
            visited: HashSet::new(),
            queue: VecDeque::new(),
            budget: MAX_SCANNED_CONTENT_BYTES,
            links: 0,
            abandoned: false,
        }
    }

    fn walk_page(&mut self, page_id: ObjectId) {
        let Some(resources_id) = page_resources_id(self.document, page_id) else {
            return;
        };
        let content = self.document.get_page_content(page_id);
        self.queue
            .push_back((Pending::PageContent(content), vec![resources_id]));
        while let Some((pending, chain)) = self.queue.pop_front() {
            match pending {
                Pending::PageContent(content) => self.scan(&content, &chain),
                Pending::Stream(object_id) => {
                    // Decompressing here rather than at enqueue time is what keeps
                    // the queue cheap. Its bytes are unreachable if this fails, so
                    // what the stream paints with is unknowable.
                    let content = self
                        .document
                        .get_object(object_id)
                        .and_then(Object::as_stream)
                        .and_then(Stream::decompressed_content);
                    match content {
                        Ok(content) => self.scan(&content, &chain),
                        Err(_) => self.preserve(&chain),
                    }
                }
            }
            if self.abandoned {
                self.queue.clear();
                return;
            }
        }
    }

    /// Registers one (stream, scope-chain) pair, or reports that the walk has hit
    /// its bounds.
    ///
    /// Returns `false` when the pair was already seen, so the caller does no work
    /// twice. Sets `abandoned` — which discards the whole document's pruning —
    /// when either cap is exceeded, because past that point what the walk has not
    /// reached is unknown and nothing may be judged dead.
    fn admit(&mut self, object_id: ObjectId, chain: &[ResourcesId]) -> bool {
        if self.visited.len() >= MAX_SCOPED_STREAMS || self.links >= MAX_SCOPE_LINKS {
            self.abandoned = true;
            return false;
        }
        if !self.visited.insert((object_id, chain.to_vec())) {
            return false;
        }
        self.links = self.links.saturating_add(chain.len());
        true
    }

    /// Records that a stream resolving names against `chain` could not be read in
    /// full, so no scope it could have named into may be filtered.
    ///
    /// Charged against the link budget like everything else. It has to be: the
    /// give-up paths call this *without* admitting a pair or decoding a byte, so
    /// on a document that keeps hitting the depth bound it was the one piece of
    /// unbounded, unpriced work left. A six-form clique in a 1,982-byte upload
    /// spent five minutes here — inside the caps, because nothing counted it.
    fn preserve(&mut self, chain: &[ResourcesId]) {
        self.links = self.links.saturating_add(chain.len());
        if self.links >= MAX_SCOPE_LINKS {
            self.abandoned = true;
        }
        self.preserved.extend(chain.iter().copied());
    }

    fn scan(&mut self, content: &[u8], chain: &[ResourcesId]) {
        // Charged in bytes: the same stream reached under two scope chains is
        // scanned twice, so cost tracks decoded bytes, not stream count.
        let charge = u64::try_from(content.len()).unwrap_or(u64::MAX);
        let Some(budget) = self.budget.checked_sub(charge) else {
            self.abandoned = true;
            return;
        };
        self.budget = budget;
        self.dictionaries.extend(chain.iter().copied());
        // Strict, because the lenient decoder returns whatever prefix it managed to
        // parse and silently discards the rest (`parser::content` throws away the
        // unconsumed remainder). A `%` comment followed by a blank line ends the
        // parse — and `get_page_content` inserts exactly such a blank line between
        // the members of a `/Contents` array — so every operator after it becomes
        // invisible. Judging their resources dead while the stream is copied to the
        // output verbatim, still naming them, is how a real page lost 77% of its
        // ink. Strict decoding turns that class of silent truncation, not just this
        // one construct, into something this walk can see and decline to act on.
        let Ok(decoded) = Content::decode_strict(content) else {
            self.preserve(chain);
            return;
        };
        for operation in &decoded.operations {
            match operation.operator.as_str() {
                "sh" => {
                    if let Some(name) = operand_name(&operation.operands) {
                        self.record(chain, ResourceCategory::Shading, name);
                    }
                }
                // An uncolored pattern's name is preceded by its colour
                // components; `operand_name` takes the last name either way.
                "scn" | "SCN" => {
                    if let Some(name) = operand_name(&operation.operands)
                        && let Some(value) = self.record(chain, ResourceCategory::Pattern, name)
                    {
                        // A surviving pattern's own content can paint with further
                        // patterns and shadings; follow it.
                        self.enqueue(chain, &value, None);
                    }
                }
                // The entry is marked live whatever its subtype — an image `Do`
                // must keep its declaration too — but only a Form has content to
                // follow.
                "Do" => {
                    if let Some(name) = operand_name(&operation.operands)
                        && let Some(value) = self.record(chain, ResourceCategory::XObject, name)
                    {
                        self.enqueue(chain, &value, Some(b"Form"));
                    }
                }
                // A Type 3 font's glyph procedures are content streams too, and a
                // Type 3 font without its own `/Resources` resolves names against
                // the page (ISO 32000-1 §9.6.5). A glyph that paints with a
                // page-level pattern keeps it alive just as a form would.
                "Tf" => {
                    if let Some(name) = operand_name(&operation.operands)
                        && let Some(value) = self.record(chain, ResourceCategory::Font, name)
                    {
                        self.enqueue_type3_font(chain, &value);
                    }
                }
                // A graphics state can reach content two ways: `/SMask /G` names a
                // form XObject painted to derive the mask, and `/Font` selects a
                // font directly (ISO 32000-1 Table 58) — including a Type 3 font,
                // whose glyph procedures `Tf` would never see.
                "gs" => {
                    if let Some(name) = operand_name(&operation.operands)
                        && let Some(value) = self.record(chain, ResourceCategory::ExtGState, name)
                    {
                        self.enqueue_graphics_state(chain, &value);
                    }
                }
                _ => {}
            }
        }
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

    /// Queues every glyph procedure of a font object, if it is a Type 3 font.
    fn enqueue_type3_font(&mut self, chain: &[ResourcesId], font: &Object) {
        let document = self.document;
        let Some((font_id, font)) = dereference_dictionary(document, font) else {
            return;
        };
        if resolved_name(document, font.get(b"Subtype").ok()) != Some(b"Type3".as_slice()) {
            return;
        }
        let Some(child) = self.child_chain(chain, font_id, font.get(b"Resources").ok()) else {
            return;
        };
        let Some((_, procedures)) = font
            .get(b"CharProcs")
            .ok()
            .and_then(|procedures| dereference_dictionary(document, procedures))
        else {
            return;
        };
        for (_, procedure) in procedures {
            self.enqueue(&child, procedure, None);
        }
    }

    /// Queues the content a graphics state parameter dictionary reaches: the
    /// transparency group its soft mask paints, and the font it selects.
    fn enqueue_graphics_state(&mut self, chain: &[ResourcesId], state: &Object) {
        let document = self.document;
        let Some((_, state)) = dereference_dictionary(document, state) else {
            return;
        };
        if let Some((_, mask)) = state
            .get(b"SMask")
            .ok()
            .and_then(|mask| dereference_dictionary(document, mask))
            && let Ok(group) = mask.get(b"G")
        {
            let group = group.clone();
            self.enqueue(chain, &group, Some(b"Form"));
        }
        // `/Font` is `[font size]`: selecting a font without ever issuing `Tf`.
        // The array itself is routinely an indirect object, so it has to be
        // resolved rather than matched on.
        if let Some(font) = resolved_array(document, state.get(b"Font").ok())
            && let Some(font) = font.first()
        {
            let font = font.clone();
            self.enqueue_type3_font(chain, &font);
        }
    }

    /// The scope a nested stream resolves against: its own `/Resources` searched
    /// first, with the enclosing scope behind, or the enclosing scope unchanged
    /// when it declares none.
    ///
    /// Returns `None` when that scope cannot be established, having first
    /// preserved everything the stream behind it could have named.
    fn child_chain(
        &mut self,
        chain: &[ResourcesId],
        owner: Option<ObjectId>,
        resources: Option<&Object>,
    ) -> Option<Vec<ResourcesId>> {
        let Some(resources) = resources else {
            // Inherits the enclosing scope, so its identity does not matter.
            return Some(chain.to_vec());
        };
        let Some(id) = scope_id(self.document, owner, resources) else {
            // A `/Resources` dictionary written inline inside a directly embedded
            // object — a Type 3 font stored in an `/ExtGState`'s `/Font` array
            // rather than as an indirect object — has no id, so there is no
            // identity to key a scope on and no dictionary this walk could filter.
            // Preserve what it reaches instead of guessing.
            self.preserve(chain);
            self.protect_shared_categories(resources);
            return None;
        };
        if chain.len() >= MAX_RESOURCE_SCOPE_DEPTH {
            // Descending no further means this stream goes unread, so its own
            // scope is preserved alongside the chain behind it — otherwise a
            // shallower, readable holder of the same `/Resources` object would
            // filter out entries this stream still names.
            self.preserve(chain);
            self.preserved.insert(id);
            return None;
        }
        let mut child = Vec::with_capacity(chain.len().saturating_add(1));
        child.push(id);
        child.extend_from_slice(chain);
        Some(child)
    }

    /// Protects every category dictionary a scope reaches by indirect reference.
    ///
    /// Used for the scope shape that has no [`ResourcesId`]. The inline dictionary
    /// itself is unshared and never filtered, but a `/Pattern` or `/Font` entry
    /// inside it can be an indirect object that a filterable scope also declares,
    /// and filtering it there would strip entries this scope's content still names.
    fn protect_shared_categories(&mut self, resources: &Object) {
        let Ok(resources) = resources.as_dict() else {
            return;
        };
        for category in ResourceCategory::ALL {
            if let Ok(Object::Reference(object_id)) = resources.get(category.key()) {
                self.protected.insert(CategoryId::Shared(*object_id));
            }
        }
    }

    /// Queues a referenced stream under the scope its own `/Resources` establish —
    /// or, when it has none, under the enclosing scope it inherits (ISO 32000-1
    /// §8.10.1).
    fn enqueue(&mut self, chain: &[ResourcesId], value: &Object, required_subtype: Option<&[u8]>) {
        // A pattern or form is an indirect stream object; anything else has no
        // content to follow.
        let Object::Reference(object_id) = value else {
            return;
        };
        let document = self.document;
        let Ok(stream) = document.get_object(*object_id).and_then(Object::as_stream) else {
            return;
        };
        // Three outcomes, not two. A `/Subtype` that resolves to something else is
        // genuinely not a form and has no content to follow. A `/Subtype` that is
        // *present but unresolvable* — a reference to a missing object — leaves the
        // walk unable to tell, and skipping it would mean the stream is neither
        // walked nor preserved: over-deletion by omission. Absent entirely is not a
        // form, since ISO 32000-1 §8.10 requires the key on every Form XObject.
        let declared_subtype = stream.dict.get(b"Subtype").ok();
        let subtype_unreadable =
            declared_subtype.is_some() && resolved_name(self.document, declared_subtype).is_none();
        if let Some(required) = required_subtype
            && !subtype_unreadable
            && resolved_name(self.document, declared_subtype) != Some(required)
        {
            return;
        }
        // No `/Resources` of its own: inherit the enclosing scope rather than
        // resolving against nothing, which would lose both its own `Do` targets
        // and the patterns its content paints with.
        let Some(child) =
            self.child_chain(chain, Some(*object_id), stream.dict.get(b"Resources").ok())
        else {
            return;
        };
        if subtype_unreadable {
            self.preserve(&child);
            return;
        }
        if !self.admit(*object_id, &child) {
            return;
        }
        self.queue.push_back((Pending::Stream(*object_id), child));
    }
}

/// The resource name an operator resolves, taken as the **last** name-valued
/// operand.
///
/// Correct for every operator this walk reads — `Do`, `gs` and `sh` take a single
/// name, `Tf` takes `/name size`, and `scn`/`SCN` put the pattern name last, after
/// any colour components — and, unlike reading the first operand, immune to a
/// stray operand left in front of it by a mis-tokenised operator.
///
/// That immunity is load-bearing, not theoretical. `lopdf`'s `operator` parser
/// matches `[A-Za-z*'"]+`, so it cannot represent `d0` or `d1` — the only content
/// operators carrying a digit, and required to open **every** Type 3 glyph
/// procedure (ISO 32000-1 §9.6.5). They tokenise as operator `d`, leaving the
/// digit behind as the *first* operand of the following operation. Reading
/// `operands.first()` therefore saw `1` rather than `/Fglyph` in
/// `20 0 0 0 20 20 d1 /Fglyph Do`, missed the `Do`, pruned `/Fglyph`, and left the
/// surviving glyph procedure naming a resource nothing declared.
///
/// Taking the last name can only ever find more names than taking the first, so
/// it can only mark more entries live — the safe direction for this walk.
fn operand_name(operands: &[Object]) -> Option<&[u8]> {
    operands
        .iter()
        .rev()
        .find_map(|operand| operand.as_name().ok())
}

/// The name a value ultimately denotes, following any chain of references.
///
/// Every read in this walk goes through one of these helpers. Mixing raw
/// `Dictionary::get` with `Document::get_dictionary` — which *does* follow
/// reference chains — is a single mistake that showed up in four places: a
/// `/Subtype` written as `12 0 R` matched nothing while the dictionary holding it
/// resolved fine, an `/ExtGState` `/Font` written indirectly never matched
/// `Object::Array`, and two aliases of one `/Pattern` dictionary were treated as
/// two scopes and filtered inconsistently. A PDF value may always be indirect;
/// code that reads one without saying so is guessing.
fn resolved_name<'a>(document: &'a Document, object: Option<&'a Object>) -> Option<&'a [u8]> {
    document.dereference(object?).ok()?.1.as_name().ok()
}

/// The array a value ultimately denotes, following any chain of references.
fn resolved_array<'a>(document: &'a Document, object: Option<&'a Object>) -> Option<&'a [Object]> {
    match document.dereference(object?).ok()?.1 {
        Object::Array(items) => Some(items),
        _ => None,
    }
}

/// The identity of a `/Resources` value, when it has one.
///
/// The id reported is the one at the **end** of any reference chain, so two
/// aliases of the same dictionary are one scope. Keying on the alias instead let
/// [`category_dictionary`] resolve a dictionary through the chain while
/// [`retain_live_entries`] wrote back through the alias, which silently dropped
/// the protection that keeps a preserved scope's entries alive.
///
/// A `/Resources` dictionary written inline inside a **directly embedded** object
/// has neither an id of its own nor an owner to borrow one from, so it has no
/// identity at all — callers must handle that rather than invent one.
fn scope_id(
    document: &Document,
    owner: Option<ObjectId>,
    resources: &Object,
) -> Option<ResourcesId> {
    match document.dereference(resources).ok()? {
        (Some(object_id), _) => Some(ResourcesId::Shared(object_id)),
        (None, _) => owner.map(ResourcesId::InlineIn),
    }
}

/// The keep-set key for one resource entry, resolved so that two aliases of the
/// same object are one key.
///
/// Shared by [`resolve_resource`] and [`retain_live_entries`] so the two can never
/// disagree about what an entry is. Falls back to the written reference when the
/// target is missing: the entry then reaches nothing, but content may still name
/// it, and removing the declaration would leave that name dangling.
fn entry_key(
    document: &Document,
    category_id: CategoryId,
    name: &[u8],
    value: &Object,
) -> LiveResource {
    match value {
        Object::Reference(object_id) => LiveResource::Object(
            document
                .dereference(value)
                .ok()
                .and_then(|(id, _)| id)
                .unwrap_or(*object_id),
        ),
        _ => LiveResource::Inline(category_id, name.to_vec()),
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
        // The node itself may be reached through a reference chain; the owner id a
        // scope is keyed on has to be the object the chain ends at.
        let (owner, object) = document
            .dereference(document.objects.get(&object_id)?)
            .ok()?;
        let dictionary = object.as_dict().ok()?;
        let owner = owner.unwrap_or(object_id);
        if let Ok(resources) = dictionary.get(b"Resources") {
            return scope_id(document, Some(owner), resources);
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
    let value = resources.get(category.key()).ok()?;
    match document.dereference(value).ok()? {
        (Some(object_id), Object::Dictionary(entries)) => {
            Some((CategoryId::Shared(object_id), entries))
        }
        (None, Object::Dictionary(entries)) => {
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
        return Some((entry_key(document, category_id, name, value), value.clone()));
    }
    None
}

/// Resolves an object to a dictionary, reporting its id when it is indirect.
///
/// A directly embedded dictionary has no id, and therefore no identity a resource
/// scope can be keyed on — callers that need one must say so rather than invent it.
fn dereference_dictionary<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Option<(Option<ObjectId>, &'a Dictionary)> {
    let (object_id, object) = document.dereference(object).ok()?;
    Some((object_id, object.as_dict().ok()?))
}

/// Filters one category dictionary in place, keeping only live entries.
///
/// Filtering the dictionary the holders actually point at — rather than writing a
/// filtered copy onto one of them — is what makes a `/Resources` dictionary shared
/// between a page and a form come out consistent. It is also why `protected` is
/// keyed by the category object rather than by the holder: sharing means one
/// holder's filtering is every holder's.
fn retain_live_entries(
    document: &mut Document,
    resources_id: ResourcesId,
    category: ResourceCategory,
    live: &HashSet<LiveResource>,
    protected: &HashSet<CategoryId>,
) {
    let Some((category_id, entries)) = category_dictionary(document, resources_id, category) else {
        return;
    };
    if protected.contains(&category_id) {
        return;
    }
    let mut retained = Dictionary::new();
    for (name, value) in entries {
        let key = entry_key(document, category_id, name, value);
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

/// Builds the replacement page.
///
/// Note the absence of `/Annots`: annotations are not carried over, which is what
/// makes it safe for [`ResourceWalk`] to ignore annotation appearance streams when
/// deciding which patterns and shadings are still painted. Adding annotation
/// preservation here means teaching the walk to follow `/AP` in the same change.
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
