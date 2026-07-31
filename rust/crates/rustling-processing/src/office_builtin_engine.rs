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
#[derive(Debug, Default, Clone, Copy)]
struct ArchiveFacts {
    /// Total `<row>` elements across every worksheet (XLSX only).
    sheet_rows: u64,
    /// Number of `ppt/slides/slideN.xml` parts (PPTX only).
    slides: usize,
}

/// Reads the cheap structural facts the bounds and the reconciliation need.
///
/// A package that cannot be opened is not rejected here: the worker gives a
/// better error for it, and this pass exists only to catch overload early.
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

    let names: Vec<String> = archive.file_names().map(ToOwned::to_owned).collect();
    for name in names {
        let lowered = name.to_ascii_lowercase();
        if extension == "pptx" && lowered.starts_with("ppt/slides/slide") && is_xml_part(&lowered) {
            facts.slides += 1;
        }
        if extension == "xlsx"
            && lowered.starts_with("xl/worksheets/")
            && is_xml_part(&lowered)
            && let Ok(entry) = archive.by_name(&name)
        {
            let remaining = max_rows.saturating_sub(facts.sheet_rows);
            facts.sheet_rows = facts
                .sheet_rows
                .saturating_add(count_rows(entry, remaining));
            if facts.sheet_rows > max_rows {
                return Ok(facts);
            }
        }
    }
    Ok(facts)
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

/// Compares the produced page count against the slide count of a PPTX.
///
/// `office2pdf` has been observed to drop whole slides from a malformed deck
/// while reporting no warnings at all, so the engine's own silence is not
/// evidence that nothing was lost. One slide renders to one page, which makes
/// the page count a usable independent check.
fn reconcile_slides(facts: &ArchiveFacts, pdf_path: &Path, conversion: &mut BuiltinConversion) {
    if facts.slides == 0 {
        return;
    }
    let Ok(document) = lopdf::Document::load(pdf_path) else {
        return;
    };
    let pages = document.get_pages().len();
    if pages >= facts.slides {
        return;
    }
    let missing = facts.slides - pages;
    conversion.dropped_content = true;
    conversion.warnings.push(format!(
        "PPTX: {missing} of {} slides produced no page and were dropped",
        facts.slides
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
        ArchiveFacts, BuiltinConversion, BuiltinEngineError, DEFAULT_MAX_SHEET_ROWS, convert,
        count_rows, inspect_archive, limit, reconcile_slides, supports_extension, truncate,
    };
    use std::io::Write;
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

    #[test]
    fn counts_worksheet_rows_and_slides() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let workbook = directory.path().join("book.xlsx");
        let sheet = b"<worksheet><sheetData><row/><row/><row/></sheetData></worksheet>";
        write_package(
            &workbook,
            &[
                ("xl/workbook.xml", b"<workbook/>"),
                ("xl/worksheets/sheet1.xml", sheet),
                ("xl/worksheets/sheet2.xml", sheet),
            ],
        )?;
        assert_eq!(inspect_archive(&workbook, "xlsx", u64::MAX)?.sheet_rows, 6);

        let deck = directory.path().join("deck.pptx");
        write_package(
            &deck,
            &[
                ("ppt/presentation.xml", b"<p/>"),
                ("ppt/slides/slide1.xml", b"<s/>"),
                ("ppt/slides/slide2.xml", b"<s/>"),
            ],
        )?;
        assert_eq!(inspect_archive(&deck, "pptx", u64::MAX)?.slides, 2);
        Ok(())
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
        assert_eq!(facts.slides, 0);
        assert_eq!(facts.sheet_rows, 0);
        Ok(())
    }

    #[test]
    fn a_missing_pdf_is_not_reconciled_into_a_false_warning() {
        let facts = ArchiveFacts {
            sheet_rows: 0,
            slides: 3,
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
