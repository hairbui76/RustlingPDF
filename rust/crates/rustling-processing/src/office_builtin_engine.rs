//! The built-in Office→PDF engine: pure-Rust `office2pdf`, run out-of-process.
//!
//! `office2pdf` converts DOCX, XLSX, and PPTX without any external tool, which
//! is what makes "Convert to PDF" work on a machine that has no `LibreOffice`
//! install. It is a library, though, and a hostile or merely malformed document
//! can make it misbehave in ways an in-process call cannot survive:
//!
//! - a non-terminating loop in the DOCX paragraph reader pins a core forever,
//!   and no timeout can cancel a thread that never yields;
//! - mutual recursion between the table and table-cell readers overflows the
//!   stack, which aborts the whole process in Rust;
//! - XLSX parsing costs roughly 77 MB per thousand rows, and long runs of
//!   whitespace in a document body expand by two orders of magnitude, so a
//!   modest upload can ask for more memory than the host has;
//! - the engine's Typst memoisation (`comemo`) and its font-metric caches are
//!   never evicted, so in-process memory grows with attacker-controlled input
//!   across requests and never plateaus.
//!
//! Every one of those is contained by running the conversion in a short-lived
//! child process: the parent kills it on a wall-clock timeout or a resident-set
//! breach, an abort becomes an ordinary non-zero exit, and all caches die with
//! the process instead of accumulating in the server. The child is this same
//! executable re-invoked with [`WORKER_ARGUMENT`].
//!
//! Cheap structural limits (upload size, worksheet rows) are still applied in
//! the parent first, so the common overload cases produce a precise 4xx instead
//! of a killed worker.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::process_executor::{ProcessExecutor, ProcessExecutorError, exit_status};

/// First argument that turns the processing executable into a one-shot
/// conversion worker instead of the HTTP service.
pub const WORKER_ARGUMENT: &str = "--office2pdf-worker";

/// Extensions the built-in engine understands. Everything else in the
/// office-conversion allow-list needs `LibreOffice`.
const BUILTIN_EXTENSIONS: &[&str] = &["docx", "xlsx", "pptx"];

/// Default ceiling on the uploaded document, before decompression.
const DEFAULT_MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;
/// Default ceiling on worksheet rows across an XLSX workbook.
const DEFAULT_MAX_SHEET_ROWS: u64 = 20_000;
/// Default wall-clock ceiling on one conversion.
const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
/// Default ceiling on the worker's resident set.
const DEFAULT_MAX_MEMORY_MB: u64 = 2_048;
/// Default number of conversions allowed to run at once. Kept at one so that
/// concurrent uploads cannot multiply the memory ceiling by the request rate.
const DEFAULT_MAX_CONCURRENCY: u64 = 1;

const MAX_INPUT_BYTES_ENV: &str = "RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_INPUT_BYTES";
const MAX_SHEET_ROWS_ENV: &str = "RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_SHEET_ROWS";
const TIMEOUT_SECONDS_ENV: &str = "RUSTLING_PROCESSING_OFFICE_BUILTIN_TIMEOUT_SECONDS";
const MAX_MEMORY_MB_ENV: &str = "RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_MEMORY_MB";
const MAX_CONCURRENCY_ENV: &str = "RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_CONCURRENCY";
const WORKER_COMMAND_ENV: &str = "RUSTLING_PROCESSING_OFFICE_WORKER_COMMAND";

/// Longest warning text forwarded to the caller, per warning.
const MAX_WARNING_CHARS: usize = 240;
/// Most warnings forwarded to the caller.
const MAX_WARNINGS: usize = 20;

/// A failure of the built-in engine.
#[derive(Debug, Error)]
pub enum BuiltinEngineError {
    #[error(
        "the built-in Office engine converts docx, xlsx, and pptx only; '{0}' needs LibreOffice installed"
    )]
    UnsupportedExtension(String),
    #[error(
        "the document is {size} bytes, above the built-in Office engine's {limit}-byte limit; install LibreOffice or raise {MAX_INPUT_BYTES_ENV}"
    )]
    InputTooLarge { size: u64, limit: u64 },
    #[error(
        "the workbook declares at least {rows} rows, above the built-in Office engine's {limit}-row limit; install LibreOffice or raise {MAX_SHEET_ROWS_ENV}"
    )]
    TooManyRows { rows: u64, limit: u64 },
    #[error(
        "the built-in Office engine did not finish within {seconds} seconds and was stopped; the document is too complex for it"
    )]
    TimedOut { seconds: u64 },
    #[error(
        "the built-in Office engine exceeded its {limit_mb} MB memory limit and was stopped; the document is too large for it"
    )]
    MemoryExhausted { limit_mb: u64 },
    #[error("the built-in Office engine could not convert the document: {0}")]
    Conversion(String),
    #[error("the built-in Office engine worker stopped unexpectedly (status {status}): {details}")]
    WorkerStopped { status: String, details: String },
    #[error("the built-in Office engine worker could not be started: {0}")]
    WorkerUnavailable(String),
    #[error("could not prepare the built-in Office engine workspace: {0}")]
    Io(#[from] std::io::Error),
}

/// The outcome of a successful built-in conversion.
#[derive(Debug, Clone, Default)]
pub struct BuiltinConversion {
    /// Human-readable warnings the engine reported, plus any this module
    /// derived by reconciling the output against the input.
    pub warnings: Vec<String>,
    /// Whether content from the source document is known to be missing from the
    /// PDF. A `true` here means the result must not be presented as a clean
    /// conversion.
    pub dropped_content: bool,
}

/// Worker → parent side-channel. The PDF travels as a file; this carries what a
/// PDF cannot say about itself.
#[derive(Debug, Default, Deserialize, Serialize)]
struct WorkerReport {
    warnings: Vec<String>,
    dropped_content: bool,
}

/// Returns whether the built-in engine can convert `extension`.
#[must_use]
pub fn supports_extension(extension: &str) -> bool {
    BUILTIN_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

/// Converts `input_path` (an OOXML package with the given `extension`) to a PDF
/// at `output_path` using the built-in engine.
///
/// # Errors
///
/// Returns [`BuiltinEngineError`] when the format is outside the built-in
/// engine's reach, when a structural limit is exceeded, when the worker breaches
/// its time or memory bound, or when the conversion itself fails.
pub fn convert(
    input_path: &Path,
    extension: &str,
    output_path: &Path,
) -> Result<BuiltinConversion, BuiltinEngineError> {
    convert_with_worker(input_path, extension, output_path, None)
}

/// Same as [`convert`], with an explicit worker executable.
///
/// The default worker is this executable re-invoked, which a test harness
/// cannot be — `cargo test` runs a binary that knows nothing about
/// [`WORKER_ARGUMENT`]. Tests therefore name the built service binary here.
///
/// # Errors
///
/// See [`convert`].
pub fn convert_with_worker(
    input_path: &Path,
    extension: &str,
    output_path: &Path,
    worker: Option<&std::ffi::OsStr>,
) -> Result<BuiltinConversion, BuiltinEngineError> {
    let extension = extension.to_ascii_lowercase();
    if !supports_extension(&extension) {
        return Err(BuiltinEngineError::UnsupportedExtension(extension));
    }

    let size = fs::metadata(input_path)?.len();
    let max_input_bytes = limit(MAX_INPUT_BYTES_ENV, DEFAULT_MAX_INPUT_BYTES);
    if size > max_input_bytes {
        return Err(BuiltinEngineError::InputTooLarge {
            size,
            limit: max_input_bytes,
        });
    }

    let max_rows = limit(MAX_SHEET_ROWS_ENV, DEFAULT_MAX_SHEET_ROWS);
    let facts = inspect_archive(input_path, &extension, max_rows)?;
    if facts.sheet_rows > max_rows {
        return Err(BuiltinEngineError::TooManyRows {
            rows: facts.sheet_rows,
            limit: max_rows,
        });
    }

    let workspace = tempfile::TempDir::new()?;
    let worker_input = workspace.path().join(format!("input.{extension}"));
    let worker_output = workspace.path().join("output.pdf");
    let worker_report = workspace.path().join("report.json");
    fs::copy(input_path, &worker_input)?;

    let report = run_worker(&worker_input, &worker_output, &worker_report, worker)?;

    let produced = fs::metadata(&worker_output).map(|metadata| metadata.len());
    if !matches!(produced, Ok(length) if length > 0) {
        return Err(BuiltinEngineError::Conversion(
            "the engine reported success but produced no PDF".to_owned(),
        ));
    }
    fs::copy(&worker_output, output_path)?;

    let mut conversion = BuiltinConversion {
        warnings: report.warnings,
        dropped_content: report.dropped_content,
    };
    reconcile_slides(&facts, output_path, &mut conversion);
    Ok(conversion)
}

/// Structural facts read from the input package before conversion.
#[derive(Debug, Default, Clone)]
struct ArchiveFacts {
    /// Total `<row>` elements across every worksheet the package declares.
    sheet_rows: u64,
    /// How many pages a deck is expected to produce.
    slides: SlideExpectation,
}

/// How many pages a PPTX is expected to produce, or why that is unknown.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
enum SlideExpectation {
    /// Not a PPTX; there is nothing to reconcile.
    #[default]
    NotApplicable,
    /// A PPTX whose slide list could not be read. Guessing here would be worse
    /// than admitting it: a guess that reads low turns a lossy conversion into
    /// a clean one.
    Undeterminable,
    /// Slides the deck declares that are not marked hidden.
    Visible(usize),
}

/// The OOXML relationships namespace, which carries the `r:id` attribute that
/// binds a `<sheet>` or `<sldId>` element to a relationship.
const RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// OPC content types identifying the parts this pass cares about. They are the
/// package's own second opinion, independent of both filenames and `.rels`.
const WORKSHEET_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";

/// Read ceiling for the directory parts (`workbook.xml`, `presentation.xml`),
/// `[Content_Types].xml`, and `.rels` siblings.
///
/// Matches the office sanitizer's own per-XML-part cap, so a `.rels` that
/// reaches this size cannot have come through the sanitizer. A part that hits
/// the cap is reported truncated and never parsed — feeding a parser half a
/// document produces a syntax error, and treating that error as "this package
/// declares nothing" is exactly the fail-open this cap was meant to prevent.
const MAX_DIRECTORY_PART_BYTES: usize = 16 * 1024 * 1024;
/// Read ceiling for the head of a slide part, which only has to contain the
/// root element's start tag.
const MAX_SLIDE_HEADER_BYTES: usize = 8 * 1024;

type PackageArchive = zip::ZipArchive<std::io::BufReader<fs::File>>;

/// The outcome of asking a package which parts it contains.
#[derive(Debug, Clone, Eq, PartialEq)]
enum PartDiscovery {
    /// The package answered.
    Resolved(Vec<String>),
    /// The package could not be asked: a directory part was missing, too large
    /// to read whole, or unparseable, and its content-type list was no better.
    Undeterminable,
}

/// Reads the cheap structural facts the bounds and the reconciliation need.
///
/// A package that cannot be opened is not rejected here: the worker gives a
/// better error for it, and this pass exists only to catch overload early.
///
/// Parts are located the way the engines locate them — by following the
/// relationship from the directory part — never by the conventional filename.
/// OOXML paths come from `.rels` targets, so a standards-legal workbook can put
/// its only worksheet at `xl/data/s1.xml` and a legal deck can name its slides
/// `ppt/slides/d1.xml`. Matching on `xl/worksheets/` or `ppt/slides/` would make
/// the row bound trivially evadable and would make the slide reconciliation
/// report a lossy conversion as clean.
///
/// When the package will not say, this pass **fails closed**. It never falls
/// back to a naming convention: a convention scan of a package that keeps its
/// parts elsewhere returns nothing, and "nothing" reads as "no rows" and "no
/// slides" — the same evasion, reinstated through the back door. Instead the
/// row count widens to every XML part in the package (which can only
/// over-count) and the slide expectation becomes
/// [`SlideExpectation::Undeterminable`], which is reported to the caller rather
/// than silently treated as agreement.
///
/// Row counting stops as soon as `max_rows` is exceeded: the caller only needs
/// to know that the workbook is over the line, and decompressing the rest of a
/// 200 MB worksheet just to produce an exact figure would itself be the
/// slowest way to reject an attack.
fn inspect_archive(
    path: &Path,
    extension: &str,
    max_rows: u64,
) -> Result<ArchiveFacts, BuiltinEngineError> {
    let mut facts = ArchiveFacts::default();
    let file = fs::File::open(path)?;
    let Ok(mut archive) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return Ok(facts);
    };

    match extension {
        "xlsx" => {
            let parts = match discover_parts(
                &mut archive,
                "xl/workbook.xml",
                "sheet",
                WORKSHEET_CONTENT_TYPE,
            ) {
                PartDiscovery::Resolved(parts) => parts,
                // Over-counting can only reject a workbook the engine could not
                // have read anyway; under-counting lets an attacker through.
                PartDiscovery::Undeterminable => every_xml_part(&archive),
            };
            for name in parts {
                let Ok(entry) = archive.by_name(&name) else {
                    continue;
                };
                let remaining = max_rows.saturating_sub(facts.sheet_rows);
                facts.sheet_rows = facts
                    .sheet_rows
                    .saturating_add(count_rows(entry, remaining));
                if facts.sheet_rows > max_rows {
                    break;
                }
            }
        }
        "pptx" => {
            facts.slides = match discover_parts(
                &mut archive,
                "ppt/presentation.xml",
                "sldId",
                SLIDE_CONTENT_TYPE,
            ) {
                PartDiscovery::Resolved(parts) => SlideExpectation::Visible(
                    parts
                        .into_iter()
                        .filter(|name| !slide_is_hidden(&mut archive, name))
                        .count(),
                ),
                PartDiscovery::Undeterminable => SlideExpectation::Undeterminable,
            };
        }
        _ => {}
    }
    Ok(facts)
}

/// Asks the package which parts it declares, first through the relationships
/// the engines follow, then through the OPC content-type list.
///
/// The content-type list is a genuine second source, not a guess: OPC requires
/// every part to be typed, and the type does not depend on where the part
/// lives. It rescues the case where a hostile `.rels` is too large to read
/// whole while leaving an honest package's answer untouched.
fn discover_parts(
    archive: &mut PackageArchive,
    directory_part: &str,
    item_local_name: &str,
    content_type: &str,
) -> PartDiscovery {
    if let Some(parts) = related_parts(archive, directory_part, item_local_name)
        && !parts.is_empty()
    {
        return PartDiscovery::Resolved(parts);
    }
    if let Some(parts) = content_type_parts(archive, content_type)
        && !parts.is_empty()
    {
        return PartDiscovery::Resolved(parts);
    }
    PartDiscovery::Undeterminable
}

/// Resolves the parts referenced by `<{item}>` elements in the directory part
/// at `part_name`.
///
/// Returns `None` when the directory part or its `.rels` sibling is missing,
/// truncated at the read cap, or unparseable — never an empty list, which the
/// caller would be entitled to read as "this package declares no parts".
///
/// Items are matched at any depth. The engines' readers are event-based and do
/// not care how deeply a `<sheet>` is nested, so requiring it to sit directly
/// inside `<sheets>` let one extra wrapper element hide every worksheet from
/// this pass while the engine still read them all.
fn related_parts(
    archive: &mut PackageArchive,
    part_name: &str,
    item_local_name: &str,
) -> Option<Vec<String>> {
    let base_directory = part_name.rsplit_once('/').map_or("", |(head, _)| head);
    let rels_name = relationship_part_name(part_name);

    let part = read_text_part(archive, part_name, MAX_DIRECTORY_PART_BYTES).complete()?;
    let rels = read_text_part(archive, &rels_name, MAX_DIRECTORY_PART_BYTES).complete()?;
    let targets = relationship_targets(&rels)?;

    let document = roxmltree::Document::parse(&part).ok()?;
    let mut parts = Vec::new();
    for node in document.descendants() {
        if !node.is_element() || node.tag_name().name() != item_local_name {
            continue;
        }
        let Some(relationship) = node.attribute((RELATIONSHIPS_NAMESPACE, "id")) else {
            continue;
        };
        let Some(target) = targets.get(relationship) else {
            continue;
        };
        let Some(resolved) = resolve_relationship_target(base_directory, target) else {
            continue;
        };
        if !parts.contains(&resolved) {
            parts.push(resolved);
        }
    }
    Some(parts)
}

/// Every part `[Content_Types].xml` gives the requested content type.
///
/// Only `<Override>` entries are read. A `<Default>` entry types a whole
/// extension, and no OOXML package types `.xml` wholesale as a worksheet or a
/// slide, so reading defaults would add nothing but a way to mistype every part
/// in the package at once.
fn content_type_parts(archive: &mut PackageArchive, content_type: &str) -> Option<Vec<String>> {
    let types =
        read_text_part(archive, "[Content_Types].xml", MAX_DIRECTORY_PART_BYTES).complete()?;
    let document = roxmltree::Document::parse(&types).ok()?;
    let mut parts = Vec::new();
    for node in document.descendants() {
        if !node.is_element() || node.tag_name().name() != "Override" {
            continue;
        }
        if !node
            .attribute("ContentType")
            .is_some_and(|declared| declared.trim().eq_ignore_ascii_case(content_type))
        {
            continue;
        }
        let Some(name) = node.attribute("PartName") else {
            continue;
        };
        let Some(resolved) = resolve_relationship_target("", name) else {
            continue;
        };
        if !parts.contains(&resolved) {
            parts.push(resolved);
        }
    }
    Some(parts)
}

/// `xl/workbook.xml` → `xl/_rels/workbook.xml.rels`.
fn relationship_part_name(part_name: &str) -> String {
    match part_name.rsplit_once('/') {
        Some((directory, file)) => format!("{directory}/_rels/{file}.rels"),
        None => format!("_rels/{part_name}.rels"),
    }
}

/// Maps relationship `Id` to `Target`, skipping external targets (which name a
/// URI, not a part in this package).
fn relationship_targets(rels_xml: &str) -> Option<BTreeMap<String, String>> {
    let document = roxmltree::Document::parse(rels_xml).ok()?;
    let mut targets = BTreeMap::new();
    for node in document.descendants() {
        if !node.is_element() || node.tag_name().name() != "Relationship" {
            continue;
        }
        if node
            .attribute("TargetMode")
            .is_some_and(|mode| mode.eq_ignore_ascii_case("External"))
        {
            continue;
        }
        if let (Some(id), Some(target)) = (node.attribute("Id"), node.attribute("Target")) {
            targets.insert(id.to_owned(), target.to_owned());
        }
    }
    Some(targets)
}

/// Resolves a relationship `Target` against the directory of the part that
/// declared it, normalising `.` and `..`.
///
/// A leading `/` means "relative to the package root". A target that climbs
/// above the root is refused: it names nothing inside the archive.
fn resolve_relationship_target(base_directory: &str, target: &str) -> Option<String> {
    let (base, relative) = match target.strip_prefix('/') {
        Some(absolute) => ("", absolute),
        None => (base_directory, target),
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in base.split('/').chain(relative.split('/')) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// Every XML part in the package, used when the package will not say which
/// parts matter. Over-counting rows is the only direction that is safe.
fn every_xml_part(archive: &PackageArchive) -> Vec<String> {
    archive
        .file_names()
        .filter(|name| is_xml_part(&name.to_ascii_lowercase()))
        .map(ToOwned::to_owned)
        .collect()
}

/// Whether a slide is marked hidden.
///
/// `office2pdf` skips `show="0"` slides, as `PowerPoint`'s own PDF export does,
/// so a hidden slide producing no page is a faithful conversion, not a loss.
/// Counting it would make the degraded signal fire on ordinary decks, and a
/// signal that cries wolf is worse than no signal.
///
/// This mirrors `office2pdf`'s own `is_hidden_slide` exactly: the first element
/// in the part must be `sld`, its `show` attribute is matched by qualified or
/// local name, and only the literal values `0` and `false` count as hidden.
/// Substring-matching the raw tag instead was wrong in both directions —
/// `show = "0"` (legal XML, engine hides it) went unnoticed, and a decoy
/// `other='x show="0" y'` (engine sees no `show`) silently removed a slide from
/// the expected count and took the loss signal with it.
///
/// Only the root element's start tag is read: it carries the attribute, and a
/// slide part is otherwise unbounded input that this pass has no reason to
/// parse. A tag that cannot be read is treated as visible, which can only
/// over-count and never hide a dropped slide.
fn slide_is_hidden(archive: &mut PackageArchive, name: &str) -> bool {
    let Some(head) = read_text_part(archive, name, MAX_SLIDE_HEADER_BYTES).head() else {
        return false;
    };
    let Some(tag) = root_element_start_tag(&head) else {
        return false;
    };
    if element_local_name(tag) != Some("sld") {
        return false;
    }
    attribute_value(tag, "show").is_some_and(|value| value == "0" || value == "false")
}

/// The local name of the element a start tag opens.
fn element_local_name(tag: &str) -> Option<&str> {
    let name = tag
        .trim_start_matches('<')
        .split(|character: char| character.is_whitespace() || character == '>' || character == '/')
        .next()
        .filter(|name| !name.is_empty())?;
    Some(name.rsplit(':').next().unwrap_or(name))
}

/// Reads an attribute out of an element start tag, by qualified or local name.
///
/// A hand-written scan rather than an XML parse: the tag is a fragment, and the
/// namespace prefixes it uses may be declared on the tag itself, so handing it
/// to a namespace-aware parser would reject exactly the documents the engine
/// accepts. Values are XML-unescaped, which is what the engine's reader does
/// before comparing them.
fn attribute_value(tag: &str, wanted: &str) -> Option<String> {
    // Drop the leading `<name` and the trailing `>` / `/>`.
    let mut rest = tag.trim_start_matches('<');
    let name_end = rest.find(|character: char| {
        character.is_whitespace() || character == '>' || character == '/'
    })?;
    rest = &rest[name_end..];
    rest = rest.trim_end_matches('>').trim_end_matches('/');

    let bytes = rest.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' {
            index += 1;
        }
        let key = rest.get(key_start..index)?;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            // A name with no value: not well-formed, and nothing more can be
            // trusted in this tag.
            return None;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let quote = *bytes.get(index)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        let value = rest.get(value_start..index)?;
        index += 1;
        if key == wanted || key.rsplit(':').next() == Some(wanted) {
            return Some(unescape_xml(value));
        }
    }
    None
}

/// Expands the five predefined XML entities and numeric character references.
///
/// The engine compares the *unescaped* value, so `show="&#48;"` hides a slide
/// there and has to hide one here too.
fn unescape_xml(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find(';') else {
            out.push_str(after);
            return out;
        };
        let entity = &after[1..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let decoded = entity
                    .strip_prefix('#')
                    .and_then(|number| match number.strip_prefix(['x', 'X']) {
                        Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok(),
                        None => number.parse::<u32>().ok(),
                    })
                    .and_then(char::from_u32);
                match decoded {
                    Some(character) => out.push(character),
                    // An entity this expander does not know is left verbatim,
                    // so it cannot accidentally become `0`.
                    None => out.push_str(&after[..=end]),
                }
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Returns the text of the first element start tag, skipping the XML
/// declaration, comments, and doctype/processing instructions before it.
fn root_element_start_tag(xml: &str) -> Option<&str> {
    let mut rest = xml;
    loop {
        let start = rest.find('<')?;
        rest = &rest[start..];
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->")?;
            rest = &after[end + 3..];
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest.find('>')?;
            rest = &rest[end + 1..];
            continue;
        }
        let end = rest.find('>')?;
        return Some(&rest[..=end]);
    }
}

/// The result of a bounded read of an archive entry.
#[derive(Debug, Clone, Eq, PartialEq)]
enum PartRead {
    /// The entry is not in the package.
    Missing,
    /// The entry is larger than the read cap; this is only its head.
    Truncated(String),
    /// The whole entry.
    Complete(String),
}

impl PartRead {
    /// The content, only when the whole part was read.
    ///
    /// A truncated document is not a document: parsing one yields a syntax
    /// error indistinguishable from "this package declares nothing", which is
    /// precisely the fail-open an oversized `.rels` was used to trigger.
    fn complete(self) -> Option<String> {
        match self {
            Self::Complete(text) => Some(text),
            Self::Missing | Self::Truncated(_) => None,
        }
    }

    /// Whatever was read, for the one caller that deliberately reads a head:
    /// the slide part whose root start tag is all that is wanted.
    fn head(self) -> Option<String> {
        match self {
            Self::Complete(text) | Self::Truncated(text) => Some(text),
            Self::Missing => None,
        }
    }
}

/// Reads an archive entry as lossy UTF-8, reporting whether the whole part
/// fitted inside `limit`.
///
/// One byte past the limit is requested so that "exactly at the cap" and "over
/// the cap" are distinguishable.
fn read_text_part(archive: &mut PackageArchive, name: &str, limit: usize) -> PartRead {
    let Ok(entry) = archive.by_name(name) else {
        return PartRead::Missing;
    };
    let mut bytes = Vec::new();
    let ceiling = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    if entry.take(ceiling).read_to_end(&mut bytes).is_err() {
        return PartRead::Missing;
    }
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        PartRead::Truncated(text)
    } else {
        PartRead::Complete(text)
    }
}

/// Returns whether an archive entry name is an XML part.
fn is_xml_part(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
}

/// Counts `<row` occurrences in a worksheet part without buffering the part.
///
/// The count only has to be good enough to decide whether the workbook is far
/// past the engine's practical size, so an approximate scan beats a full XML
/// parse of a part that may itself be the attack. Counting stops once `budget`
/// rows have been passed, so the returned figure is a lower bound.
fn count_rows(mut entry: impl Read, budget: u64) -> u64 {
    const CHUNK: usize = 64 * 1024;
    const NEEDLE: &[u8] = b"<row";

    let mut rows = 0_u64;
    let mut buffer = vec![0_u8; CHUNK];
    // Bytes carried over so a needle split across two reads is still counted.
    let mut carry: Vec<u8> = Vec::with_capacity(NEEDLE.len());
    loop {
        let Ok(read) = entry.read(&mut buffer) else {
            return rows;
        };
        if read == 0 {
            return rows;
        }
        let mut window = carry.clone();
        window.extend_from_slice(&buffer[..read]);
        let found = window
            .windows(NEEDLE.len())
            .filter(|candidate| *candidate == NEEDLE)
            .count();
        rows = rows.saturating_add(u64::try_from(found).unwrap_or(u64::MAX));
        if rows > budget {
            return rows;
        }
        let keep = window.len().saturating_sub(NEEDLE.len() - 1);
        carry = window[keep..].to_vec();
    }
}

/// Compares the produced page count against the number of slides the deck
/// expects to render.
///
/// `office2pdf` has been observed to drop whole slides from a malformed deck
/// while reporting no warnings at all, so the engine's own silence is not
/// evidence that nothing was lost. One visible slide renders to one page, which
/// makes the page count a usable independent check. `facts.slides` already
/// excludes hidden slides, which the engine skips on purpose.
fn reconcile_slides(facts: &ArchiveFacts, pdf_path: &Path, conversion: &mut BuiltinConversion) {
    let expected = match facts.slides {
        // Nothing to reconcile: not a deck, or a deck whose every slide is
        // hidden.
        SlideExpectation::NotApplicable | SlideExpectation::Visible(0) => return,
        // Saying nothing here would be indistinguishable from "checked, and
        // nothing was lost". Say instead that nothing was checked, without
        // claiming a loss there is no evidence for.
        SlideExpectation::Undeterminable => {
            conversion.warnings.push(
                "PPTX: the deck does not say which slides it contains, so the page count could \
                 not be verified and missing slides would not be detected"
                    .to_owned(),
            );
            return;
        }
        SlideExpectation::Visible(expected) => expected,
    };
    let Ok(document) = lopdf::Document::load(pdf_path) else {
        return;
    };
    let pages = document.get_pages().len();
    if pages >= expected {
        return;
    }
    let missing = expected - pages;
    conversion.dropped_content = true;
    conversion.warnings.push(format!(
        "PPTX: {missing} of {expected} visible slides produced no page and were dropped"
    ));
}

/// Spawns the worker and enforces the time and memory bounds around it.
fn run_worker(
    input: &Path,
    output: &Path,
    report_path: &Path,
    worker: Option<&std::ffi::OsStr>,
) -> Result<WorkerReport, BuiltinEngineError> {
    let command = worker_command(worker)?;
    let timeout = Duration::from_secs(limit(TIMEOUT_SECONDS_ENV, DEFAULT_TIMEOUT_SECONDS).max(1));
    let memory_bytes = limit(MAX_MEMORY_MB_ENV, DEFAULT_MAX_MEMORY_MB).saturating_mul(1024 * 1024);
    let concurrency = usize::try_from(limit(MAX_CONCURRENCY_ENV, DEFAULT_MAX_CONCURRENCY))
        .unwrap_or(1)
        .max(1);

    let executor = ProcessExecutor::new(concurrency, timeout).with_memory_limit(memory_bytes);
    let arguments: Vec<OsString> = vec![
        OsString::from(WORKER_ARGUMENT),
        input.as_os_str().to_owned(),
        output.as_os_str().to_owned(),
        report_path.as_os_str().to_owned(),
    ];

    let result = executor.run(&command, &arguments);
    match result {
        Ok(process) if process.status.success() => Ok(read_report(report_path)),
        Ok(process) => {
            let details = truncate(&String::from_utf8_lossy(&process.stderr), 2_048);
            // The worker reports a clean conversion failure on exit code 2 and
            // writes the reason to stderr; anything else is a crash, an abort,
            // or a stack overflow, which is a property of the document just the
            // same but has no message worth quoting verbatim.
            if process.status.code() == Some(2) {
                Err(BuiltinEngineError::Conversion(details))
            } else {
                Err(BuiltinEngineError::WorkerStopped {
                    status: exit_status(process.status),
                    details: if details.is_empty() {
                        "no diagnostic output".to_owned()
                    } else {
                        details
                    },
                })
            }
        }
        Err(ProcessExecutorError::Timeout { timeout }) => Err(BuiltinEngineError::TimedOut {
            seconds: timeout.as_secs(),
        }),
        Err(ProcessExecutorError::MemoryLimit { .. }) => Err(BuiltinEngineError::MemoryExhausted {
            limit_mb: limit(MAX_MEMORY_MB_ENV, DEFAULT_MAX_MEMORY_MB),
        }),
        Err(ProcessExecutorError::Start(source)) => Err(BuiltinEngineError::WorkerUnavailable(
            format!("{}: {source}", command.to_string_lossy()),
        )),
        Err(ProcessExecutorError::Output(source)) => {
            Err(BuiltinEngineError::WorkerUnavailable(source.to_string()))
        }
    }
}

/// Resolves the executable to re-invoke as the conversion worker.
fn worker_command(worker: Option<&std::ffi::OsStr>) -> Result<OsString, BuiltinEngineError> {
    if let Some(explicit) = worker {
        return Ok(explicit.to_owned());
    }
    if let Some(configured) = crate::environment::var_os(WORKER_COMMAND_ENV)
        && !configured.is_empty()
    {
        return Ok(configured);
    }
    std::env::current_exe()
        .map(PathBuf::into_os_string)
        .map_err(|error| {
            BuiltinEngineError::WorkerUnavailable(format!(
                "could not locate this executable to re-invoke as the conversion worker: {error}"
            ))
        })
}

/// Reads the worker's side-channel, tolerating its absence.
///
/// A missing or unreadable report only costs warning fidelity; the PDF is
/// already on disk and refusing it here would turn a cosmetic problem into a
/// failed conversion.
fn read_report(path: &Path) -> WorkerReport {
    let Ok(bytes) = fs::read(path) else {
        return WorkerReport::default();
    };
    let mut report: WorkerReport = serde_json::from_slice(&bytes).unwrap_or_default();
    report.warnings.truncate(MAX_WARNINGS);
    for warning in &mut report.warnings {
        *warning = truncate(warning, MAX_WARNING_CHARS);
    }
    report
}

/// Runs one conversion in worker mode. Returns the process exit code.
///
/// `2` is a conversion failure with a quotable reason on stderr; `1` is a
/// usage or I/O failure of the worker itself.
#[must_use]
pub fn run_worker_process(arguments: &[OsString]) -> i32 {
    let [input, output, report_path] = arguments else {
        eprintln!("usage: {WORKER_ARGUMENT} <input> <output.pdf> <report.json>");
        return 1;
    };
    let input = Path::new(input);
    let format = match input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("docx") => office2pdf::config::Format::Docx,
        Some("xlsx") => office2pdf::config::Format::Xlsx,
        Some("pptx") => office2pdf::config::Format::Pptx,
        other => {
            eprintln!("unsupported worker input format: {}", other.unwrap_or(""));
            return 1;
        }
    };

    let bytes = match fs::read(input) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("could not read the worker input: {error}");
            return 1;
        }
    };

    let options = office2pdf::config::ConvertOptions::default();
    let converted = match office2pdf::convert_bytes(&bytes, format, &options) {
        Ok(converted) => converted,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };

    if let Err(error) = fs::write(Path::new(output), &converted.pdf) {
        eprintln!("could not write the worker output: {error}");
        return 1;
    }

    let report = WorkerReport {
        dropped_content: converted.warnings.iter().any(|warning| {
            matches!(
                warning,
                office2pdf::error::ConvertWarning::UnsupportedElement { .. }
                    | office2pdf::error::ConvertWarning::ParseSkipped { .. }
            )
        }),
        warnings: converted
            .warnings
            .iter()
            .map(describe_warning)
            .collect::<Vec<_>>(),
    };
    match serde_json::to_vec(&report) {
        // A report that cannot be written costs warning fidelity only; the PDF
        // is already written, so this must not fail the conversion.
        Ok(bytes) => {
            let _ = fs::write(Path::new(report_path), bytes);
        }
        Err(error) => eprintln!("could not encode the worker report: {error}"),
    }
    0
}

fn describe_warning(warning: &office2pdf::error::ConvertWarning) -> String {
    use office2pdf::error::ConvertWarning;
    match warning {
        ConvertWarning::UnsupportedElement { format, element } => {
            format!("{format}: '{element}' is not supported and was omitted")
        }
        ConvertWarning::PartialElement {
            format,
            element,
            detail,
        } => format!("{format}: '{element}' was only partially rendered ({detail})"),
        ConvertWarning::FallbackUsed { format, from, to } => {
            format!("{format}: '{from}' was rendered as '{to}'")
        }
        ConvertWarning::ParseSkipped { format, reason } => {
            format!("{format}: content was skipped while parsing ({reason})")
        }
    }
}

fn truncate(value: &str, characters: usize) -> String {
    let trimmed = value.trim();
    let mut result: String = trimmed.chars().take(characters).collect();
    if trimmed.chars().nth(characters).is_some() {
        result.push('…');
    }
    result
}

/// Reads a positive numeric limit from the environment, falling back to
/// `default` when unset, unparseable, or zero.
fn limit(name: &str, default: u64) -> u64 {
    crate::environment::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveFacts, BuiltinConversion, BuiltinEngineError, DEFAULT_MAX_SHEET_ROWS,
        RELATIONSHIPS_NAMESPACE, SLIDE_CONTENT_TYPE, SlideExpectation, WORKSHEET_CONTENT_TYPE,
        attribute_value, convert, count_rows, element_local_name, inspect_archive, limit,
        reconcile_slides, relationship_part_name, relationship_targets,
        resolve_relationship_target, root_element_start_tag, supports_extension, truncate,
        unescape_xml,
    };
    use std::fmt::Write as _;
    use std::io::Write as _;
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn write_package(path: &std::path::Path, parts: &[(&str, &[u8])]) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, bytes) in parts {
            zip.start_file(*name, options)
                .map_err(std::io::Error::other)?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(std::io::Error::other)?;
        Ok(())
    }

    #[test]
    fn recognises_only_the_built_in_formats() {
        assert!(supports_extension("docx"));
        assert!(supports_extension("XLSX"));
        assert!(supports_extension("pptx"));
        assert!(!supports_extension("doc"));
        assert!(!supports_extension("odt"));
        assert!(!supports_extension("html"));
    }

    #[test]
    fn rejects_a_format_the_built_in_engine_cannot_read() {
        let error = convert(
            std::path::Path::new("input.odt"),
            "odt",
            std::path::Path::new("out.pdf"),
        );
        assert!(matches!(
            error,
            Err(BuiltinEngineError::UnsupportedExtension(extension)) if extension == "odt"
        ));
    }

    #[test]
    fn counts_rows_across_read_boundaries() {
        let mut sheet = Vec::new();
        for _ in 0..5_000 {
            sheet.extend_from_slice(b"<row r=\"1\"><c/></row>");
        }
        assert_eq!(count_rows(sheet.as_slice(), u64::MAX), 5_000);
    }

    const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const SHEET: &[u8] = b"<worksheet><sheetData><row/><row/><row/></sheetData></worksheet>";

    fn rels(entries: &[(&str, &str)]) -> Vec<u8> {
        let body = entries.iter().fold(String::new(), |mut xml, (id, target)| {
            let _ = write!(xml, r#"<Relationship Id="{id}" Target="{target}"/>"#);
            xml
        });
        format!(r#"<Relationships xmlns="{RELS_NS}">{body}</Relationships>"#).into_bytes()
    }

    fn workbook(sheets: &[&str]) -> Vec<u8> {
        let body = sheets
            .iter()
            .enumerate()
            .fold(String::new(), |mut xml, (index, name)| {
                let _ = write!(
                    xml,
                    r#"<sheet name="{name}" sheetId="{}" r:id="rId{}"/>"#,
                    index + 1,
                    index + 1
                );
                xml
            });
        format!(
            r#"<workbook xmlns:r="{RELATIONSHIPS_NAMESPACE}"><sheets>{body}</sheets></workbook>"#
        )
        .into_bytes()
    }

    fn presentation(slides: usize) -> Vec<u8> {
        let body = (0..slides).fold(String::new(), |mut xml, index| {
            let _ = write!(
                xml,
                r#"<sldId id="{}" r:id="rId{}"/>"#,
                256 + index,
                index + 1
            );
            xml
        });
        format!(
            r#"<p:presentation xmlns:p="pml" xmlns:r="{RELATIONSHIPS_NAMESPACE}"><p:sldIdLst>{body}</p:sldIdLst></p:presentation>"#
        )
        .into_bytes()
    }

    /// The evasion the naming convention allowed: OOXML part paths come from
    /// relationship targets, so a standards-legal workbook can put its only
    /// worksheet outside `xl/worksheets/` and a prefix match sees zero rows.
    #[test]
    fn counts_rows_in_a_worksheet_the_workbook_relocated() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let book = directory.path().join("relocated.xlsx");
        write_package(
            &book,
            &[
                ("xl/workbook.xml", &workbook(&["Sheet1"])),
                (
                    "xl/_rels/workbook.xml.rels",
                    &rels(&[("rId1", "data/s1.xml")]),
                ),
                ("xl/data/s1.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 3);
        Ok(())
    }

    #[test]
    fn resolves_absolute_and_climbing_relationship_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let book = directory.path().join("absolute.xlsx");
        write_package(
            &book,
            &[
                ("xl/workbook.xml", &workbook(&["A", "B"])),
                (
                    "xl/_rels/workbook.xml.rels",
                    &rels(&[("rId1", "/sheets/a.xml"), ("rId2", "../shared/b.xml")]),
                ),
                ("sheets/a.xml", SHEET),
                ("shared/b.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 6);
        Ok(())
    }

    /// A part the workbook does not reference is not read by the engine either,
    /// so counting it would reject a workbook for rows nothing will parse.
    #[test]
    fn ignores_a_worksheet_the_workbook_never_references() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let book = directory.path().join("orphan.xlsx");
        write_package(
            &book,
            &[
                ("xl/workbook.xml", &workbook(&["Sheet1"])),
                (
                    "xl/_rels/workbook.xml.rels",
                    &rels(&[("rId1", "worksheets/sheet1.xml")]),
                ),
                ("xl/worksheets/sheet1.xml", SHEET),
                ("xl/worksheets/orphan.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 3);
        Ok(())
    }

    fn content_types(parts: &[(&str, &str)]) -> Vec<u8> {
        let body = parts.iter().fold(String::new(), |mut xml, (name, kind)| {
            let _ = write!(xml, r#"<Override PartName="{name}" ContentType="{kind}"/>"#);
            xml
        });
        format!(
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{body}</Types>"#
        )
        .into_bytes()
    }

    /// When the workbook cannot be read, the content-type list is asked
    /// instead. It types parts wherever they live, so it survives the tricks
    /// that defeat both the relationship walk and a filename scan.
    #[test]
    fn falls_back_to_the_content_type_list() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let book = directory.path().join("book.xlsx");
        write_package(
            &book,
            &[
                ("xl/workbook.xml", b"<not-well-formed"),
                (
                    "[Content_Types].xml",
                    &content_types(&[("/xl/data/s1.xml", WORKSHEET_CONTENT_TYPE)]),
                ),
                ("xl/data/s1.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 3);

        let deck = directory.path().join("deck.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", b"<not-well-formed"),
                (
                    "[Content_Types].xml",
                    &content_types(&[
                        ("/ppt/decks/a.xml", SLIDE_CONTENT_TYPE),
                        ("/ppt/decks/b.xml", SLIDE_CONTENT_TYPE),
                    ]),
                ),
                ("ppt/decks/a.xml", b"<p:sld/>"),
                ("ppt/decks/b.xml", b"<p:sld/>"),
            ],
        )?;
        assert_eq!(
            inspect_archive(&deck, "pptx", u64::MAX)?.slides,
            SlideExpectation::Visible(2)
        );
        Ok(())
    }

    /// The fail-open that reinstated the original evasion: a `.rels` too large
    /// to read whole used to parse as nothing, and the filename scan then found
    /// no worksheet because the real one lives elsewhere. The rows must still
    /// be counted.
    #[test]
    fn an_oversized_rels_part_cannot_hide_the_worksheet() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let book = directory.path().join("padded.xlsx");
        let mut padded = Vec::from(
            format!(
                r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Target="data/s1.xml"/>"#
            )
            .as_bytes(),
        );
        padded.extend(std::iter::repeat_n(b' ', 17 * 1024 * 1024));
        padded.extend_from_slice(b"</Relationships>");
        write_package(
            &book,
            &[
                ("xl/workbook.xml", &workbook(&["Sheet1"])),
                ("xl/_rels/workbook.xml.rels", &padded),
                (
                    "[Content_Types].xml",
                    &content_types(&[("/xl/data/s1.xml", WORKSHEET_CONTENT_TYPE)]),
                ),
                ("xl/data/s1.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 3);
        Ok(())
    }

    /// The other fail-open door: one extra wrapper element defeated the
    /// parent-element check while the engine's event-based reader still saw
    /// every `<sheet>`.
    #[test]
    fn an_extra_nesting_level_cannot_hide_the_worksheet() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let book = directory.path().join("nested.xlsx");
        let nested = format!(
            r#"<workbook xmlns:r="{RELATIONSHIPS_NAMESPACE}"><sheets><wrap><sheet name="S" sheetId="1" r:id="rId1"/></wrap></sheets></workbook>"#
        );
        write_package(
            &book,
            &[
                ("xl/workbook.xml", nested.as_bytes()),
                (
                    "xl/_rels/workbook.xml.rels",
                    &rels(&[("rId1", "data/s1.xml")]),
                ),
                ("xl/data/s1.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 3);
        Ok(())
    }

    /// With neither a readable workbook nor a readable content-type list there
    /// is no honest answer, so the count widens to the whole package. A scan
    /// that reads low here is the evasion; over-counting only rejects a package
    /// the engine could not have read either.
    #[test]
    fn an_undeterminable_package_counts_every_xml_part() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let book = directory.path().join("opaque.xlsx");
        write_package(
            &book,
            &[
                ("xl/workbook.xml", b"<not-well-formed"),
                ("[Content_Types].xml", b"<not-well-formed"),
                ("xl/somewhere/else.xml", SHEET),
            ],
        )?;
        assert_eq!(inspect_archive(&book, "xlsx", u64::MAX)?.sheet_rows, 3);
        Ok(())
    }

    /// A deck whose slide list cannot be read is reported as unverified, never
    /// as verified-and-clean.
    #[test]
    fn an_undeterminable_deck_reports_that_it_was_not_verified()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let deck = directory.path().join("opaque.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", b"<not-well-formed"),
                ("[Content_Types].xml", b"<not-well-formed"),
                ("ppt/slides/slide1.xml", b"<p:sld/>"),
            ],
        )?;
        let facts = inspect_archive(&deck, "pptx", u64::MAX)?;
        assert_eq!(facts.slides, SlideExpectation::Undeterminable);

        let mut conversion = BuiltinConversion::default();
        reconcile_slides(
            &facts,
            std::path::Path::new("/nonexistent.pdf"),
            &mut conversion,
        );
        assert!(!conversion.dropped_content, "no evidence of loss to claim");
        assert!(
            conversion
                .warnings
                .iter()
                .any(|warning| warning.contains("could not be verified")),
            "the caller must be told the check did not run: {:?}",
            conversion.warnings
        );
        Ok(())
    }

    /// Slide parts are named by their relationship target too, so a deck whose
    /// slides are not called `slideN.xml` must still be reconciled — otherwise
    /// a dropped slide is reported as a clean conversion.
    #[test]
    fn counts_slides_with_non_conventional_part_names() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let deck = directory.path().join("renamed.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", &presentation(2)),
                (
                    "ppt/_rels/presentation.xml.rels",
                    &rels(&[("rId1", "slides/d1.xml"), ("rId2", "slides/d2.xml")]),
                ),
                ("ppt/slides/d1.xml", b"<p:sld xmlns:p=\"pml\"/>"),
                ("ppt/slides/d2.xml", b"<p:sld xmlns:p=\"pml\"/>"),
            ],
        )?;
        assert_eq!(
            inspect_archive(&deck, "pptx", u64::MAX)?.slides,
            SlideExpectation::Visible(2)
        );
        Ok(())
    }

    /// `office2pdf` skips `show="0"` slides on purpose, as `PowerPoint`'s own PDF
    /// export does, so counting them would make the degraded signal fire on
    /// ordinary decks.
    #[test]
    fn excludes_hidden_slides_from_the_expected_page_count()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let deck = directory.path().join("hidden.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", &presentation(3)),
                (
                    "ppt/_rels/presentation.xml.rels",
                    &rels(&[
                        ("rId1", "slides/slide1.xml"),
                        ("rId2", "slides/slide2.xml"),
                        ("rId3", "slides/slide3.xml"),
                    ]),
                ),
                ("ppt/slides/slide1.xml", b"<p:sld xmlns:p=\"pml\"/>"),
                (
                    "ppt/slides/slide2.xml",
                    b"<?xml version=\"1.0\"?><!-- hidden --><p:sld xmlns:p=\"pml\" show=\"0\"/>",
                ),
                (
                    "ppt/slides/slide3.xml",
                    b"<p:sld xmlns:p=\"pml\" show='false'/>",
                ),
            ],
        )?;
        assert_eq!(
            inspect_archive(&deck, "pptx", u64::MAX)?.slides,
            SlideExpectation::Visible(1)
        );
        Ok(())
    }

    /// The two ways substring matching was wrong. `show = "0"` is legal XML and
    /// the engine's real parser hides the slide; a decoy `show="0"` inside an
    /// unrelated attribute value is not a `show` attribute at all, and treating
    /// it as one silently removed a slide from the expected count.
    #[test]
    fn reads_the_show_attribute_rather_than_matching_the_raw_tag()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let deck = directory.path().join("attrs.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", &presentation(4)),
                (
                    "ppt/_rels/presentation.xml.rels",
                    &rels(&[
                        ("rId1", "slides/spaced.xml"),
                        ("rId2", "slides/decoy.xml"),
                        ("rId3", "slides/entity.xml"),
                        ("rId4", "slides/plain.xml"),
                    ]),
                ),
                // Legal XML, and the engine hides it. Must not be counted.
                (
                    "ppt/slides/spaced.xml",
                    b"<p:sld xmlns:p=\"pml\" show = \"0\" />",
                ),
                // No `show` attribute at all: the engine renders it, so it must
                // be counted or a real drop goes unreported.
                (
                    "ppt/slides/decoy.xml",
                    b"<p:sld xmlns:p=\"pml\" descr='x show=\"0\" y'/>",
                ),
                // The engine compares the unescaped value, so this is hidden.
                (
                    "ppt/slides/entity.xml",
                    b"<p:sld xmlns:p=\"pml\" show=\"&#48;\"/>",
                ),
                ("ppt/slides/plain.xml", b"<p:sld xmlns:p=\"pml\"/>"),
            ],
        )?;
        assert_eq!(
            inspect_archive(&deck, "pptx", u64::MAX)?.slides,
            SlideExpectation::Visible(2),
            "only the decoy and the plain slide are visible"
        );
        Ok(())
    }

    /// A root element that is not `sld` is never hidden, matching the engine,
    /// which gives up on the first element if it is not a slide.
    #[test]
    fn only_a_slide_root_element_can_be_hidden() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let deck = directory.path().join("notslide.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", &presentation(1)),
                (
                    "ppt/_rels/presentation.xml.rels",
                    &rels(&[("rId1", "slides/odd.xml")]),
                ),
                (
                    "ppt/slides/odd.xml",
                    b"<p:notASlide xmlns:p=\"pml\" show=\"0\"/>",
                ),
            ],
        )?;
        assert_eq!(
            inspect_archive(&deck, "pptx", u64::MAX)?.slides,
            SlideExpectation::Visible(1)
        );
        Ok(())
    }

    #[test]
    fn reads_attributes_out_of_an_element_start_tag() {
        let tag = r#"<p:sld xmlns:p="pml" show = '0' descr="a show=&quot;1&quot; b"/>"#;
        assert_eq!(element_local_name(tag), Some("sld"));
        assert_eq!(attribute_value(tag, "show").as_deref(), Some("0"));
        assert_eq!(
            attribute_value(tag, "descr").as_deref(),
            Some(r#"a show="1" b"#)
        );
        assert_eq!(attribute_value(tag, "absent"), None);
        // Matched by local name, exactly as the engine's reader does.
        assert_eq!(
            attribute_value(r#"<sld p:show="false">"#, "show").as_deref(),
            Some("false")
        );
        assert_eq!(element_local_name("<sld>"), Some("sld"));
        assert_eq!(element_local_name("<a:sld/>"), Some("sld"));
    }

    #[test]
    fn unescapes_the_values_the_engine_unescapes() {
        assert_eq!(unescape_xml("plain"), "plain");
        assert_eq!(unescape_xml("&#48;"), "0");
        assert_eq!(unescape_xml("&#x30;"), "0");
        assert_eq!(
            unescape_xml("a&amp;b&lt;c&gt;d&quot;e&apos;f"),
            "a&b<c>d\"e'f"
        );
        // An entity this expander does not know must not silently become
        // something else.
        assert_eq!(unescape_xml("&nbsp;"), "&nbsp;");
        assert_eq!(unescape_xml("&unterminated"), "&unterminated");
    }

    #[test]
    fn resolves_relationship_targets_against_the_declaring_part() {
        assert_eq!(
            resolve_relationship_target("xl", "data/s1.xml").as_deref(),
            Some("xl/data/s1.xml")
        );
        assert_eq!(
            resolve_relationship_target("xl", "/ppt/slides/a.xml").as_deref(),
            Some("ppt/slides/a.xml")
        );
        assert_eq!(
            resolve_relationship_target("ppt", "./slides/./a.xml").as_deref(),
            Some("ppt/slides/a.xml")
        );
        assert_eq!(
            resolve_relationship_target("ppt/slides", "../media/a.xml").as_deref(),
            Some("ppt/media/a.xml")
        );
        // Climbing above the package root names nothing in the archive.
        assert_eq!(resolve_relationship_target("xl", "../../escape.xml"), None);
        assert_eq!(resolve_relationship_target("", ""), None);
    }

    #[test]
    fn finds_the_root_element_past_declarations_and_comments() {
        assert_eq!(
            root_element_start_tag("<?xml version=\"1.0\"?>\n<!-- c --><p:sld show=\"0\"/>"),
            Some("<p:sld show=\"0\"/>")
        );
        assert_eq!(root_element_start_tag("no markup here"), None);
    }

    #[test]
    fn derives_the_relationship_part_name() {
        assert_eq!(
            relationship_part_name("xl/workbook.xml"),
            "xl/_rels/workbook.xml.rels"
        );
        assert_eq!(
            relationship_part_name("ppt/presentation.xml"),
            "ppt/_rels/presentation.xml.rels"
        );
        assert_eq!(relationship_part_name("root.xml"), "_rels/root.xml.rels");
    }

    #[test]
    fn skips_external_relationship_targets() {
        let external = format!(
            r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Target="http://example.invalid/s.xml" TargetMode="External"/><Relationship Id="rId2" Target="worksheets/sheet1.xml"/></Relationships>"#
        );
        let targets = relationship_targets(&external).unwrap_or_default();
        assert!(!targets.contains_key("rId1"));
        assert_eq!(
            targets.get("rId2").map(String::as_str),
            Some("worksheets/sheet1.xml")
        );
    }

    /// The row bound must trip before any worker is spawned, so this test also
    /// proves the expensive path is never entered for an oversized workbook.
    #[test]
    fn rejects_a_workbook_past_the_row_limit() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let workbook = directory.path().join("book.xlsx");
        let rows = DEFAULT_MAX_SHEET_ROWS + 1;
        let mut sheet = Vec::from(*b"<worksheet><sheetData>");
        for _ in 0..rows {
            sheet.extend_from_slice(b"<row/>");
        }
        sheet.extend_from_slice(b"</sheetData></worksheet>");
        write_package(&workbook, &[("xl/worksheets/sheet1.xml", &sheet)])?;

        let result = convert(&workbook, "xlsx", &directory.path().join("out.pdf"));
        assert!(
            matches!(
                result,
                Err(BuiltinEngineError::TooManyRows { rows: reported, limit })
                    if reported == rows && limit == DEFAULT_MAX_SHEET_ROWS
            ),
            "expected a row-limit rejection, got {result:?}"
        );
        Ok(())
    }

    #[test]
    fn tolerates_an_unopenable_package_and_leaves_it_to_the_worker()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let broken = directory.path().join("broken.docx");
        std::fs::write(&broken, b"not a zip at all")?;
        let facts = inspect_archive(&broken, "docx", u64::MAX)?;
        assert_eq!(facts.slides, SlideExpectation::NotApplicable);
        assert_eq!(facts.sheet_rows, 0);
        Ok(())
    }

    #[test]
    fn a_missing_pdf_is_not_reconciled_into_a_false_warning() {
        let facts = ArchiveFacts {
            sheet_rows: 0,
            slides: SlideExpectation::Visible(3),
        };
        let mut conversion = BuiltinConversion::default();
        reconcile_slides(
            &facts,
            std::path::Path::new("/nonexistent.pdf"),
            &mut conversion,
        );
        assert!(conversion.warnings.is_empty());
        assert!(!conversion.dropped_content);
    }

    #[test]
    fn falls_back_to_the_default_limit() {
        assert_eq!(limit("RUSTLING_PROCESSING_OFFICE_BUILTIN_ABSENT", 7), 7);
    }

    #[test]
    fn truncates_long_diagnostics() {
        assert_eq!(truncate("  hello  ", 20), "hello");
        assert_eq!(truncate("abcdef", 3), "abc…");
    }
}
