//! Office/text document to PDF conversion.
//!
//! Two engines back this surface. `LibreOffice` (`soffice --headless
//! --convert-to pdf`) has materially better fidelity and covers every format in
//! [`ALLOWED_EXTENSIONS`], so it is used whenever it is installed. The built-in
//! pure-Rust engine in [`crate::office_builtin_engine`] covers DOCX, XLSX, and
//! PPTX with no external tool at all, so the feature still works on a machine
//! that has no `LibreOffice` — the situation that made "Convert to PDF" report
//! itself unavailable on Windows desktop installs.
//!
//! Both engines see the same hardened input: HTML/HTM passes through the shared
//! strict HTML sanitizer, and OOXML/ODF packages are rewritten by the office
//! sanitizer, preventing external-resource SSRF and active-content execution.
//!
//! PDF → office conversion has no built-in engine and stays `LibreOffice`-only.

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;
use thiserror::Error;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    html_sanitizer::sanitize_html,
    office_builtin_engine::{self, BuiltinConversion, BuiltinEngineError},
    office_sanitizer::{is_sanitizable_extension, sanitize_office_archive},
    process_executor::exit_status,
};

const SOFFICE_COMMAND_ENV: &str = "RUSTLING_PROCESSING_SOFFICE_COMMAND";
const OFFICE_ENGINE_ENV: &str = "RUSTLING_PROCESSING_OFFICE_ENGINE";

/// Extensions `LibreOffice` can convert that this port accepts.
const ALLOWED_EXTENSIONS: &[&str] = &[
    "doc", "docx", "docm", "dot", "dotx", "dotm", "odt", "ott", "rtf", "txt", "xml", "wps", "xls",
    "xlsx", "xlsm", "xlt", "xltx", "xltm", "ods", "ots", "csv", "tsv", "ppt", "pptx", "pptm",
    "pot", "potx", "potm", "pps", "ppsx", "ppsm", "odp", "otp", "odg", "otg", "odf", "odc", "odi",
    "odm", "vsd", "vsdx", "pub", "epub", "fodt", "fods", "fodp", "html", "htm",
];

#[derive(Debug, Error)]
pub enum OfficeToPdfError {
    #[error("fileInput must have a file extension")]
    MissingExtension,
    #[error("unsupported file extension '{0}' for LibreOffice conversion")]
    InvalidExtension(String),
    #[error(
        "outputFormat '{0}' is not supported (expected one of doc, docx, odt, ppt, pptx, odp, rtf, xml)"
    )]
    InvalidOutputFormat(String),
    #[error("LibreOffice (soffice) is required to convert documents but was not found")]
    SofficeUnavailable,
    #[error(
        "'{extension}' needs LibreOffice, which is not in use here (not installed, or RUSTLING_PROCESSING_OFFICE_ENGINE forces the built-in engine); the built-in engine converts docx, xlsx, and pptx only"
    )]
    NoEngineForExtension { extension: String },
    #[error("{0}")]
    Builtin(#[from] BuiltinEngineError),
    #[error(
        "RUSTLING_PROCESSING_OFFICE_ENGINE is set to '{0}' (expected 'auto', 'libreoffice', or 'builtin')"
    )]
    InvalidEngine(String),
    #[error("LibreOffice conversion with '{command}' failed with status {status}: {details}")]
    SofficeFailed {
        command: String,
        status: String,
        details: String,
    },
    #[error("could not start LibreOffice command '{command}': {source}")]
    SofficeStart {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("LibreOffice did not produce a PDF")]
    NoOutput,
    #[error("office document sanitization failed: {0}")]
    UnsafeArchive(String),
    #[error("could not prepare the conversion workspace: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not build the output archive: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Extensions accepted for PDF → office conversions (`processPdfToOfficeFormat`).
const ALLOWED_OFFICE_FORMATS: &[&str] = &["doc", "docx", "odt", "ppt", "pptx", "odp", "rtf", "xml"];

/// Shape of a PDF → office conversion result.
#[derive(Debug, Clone)]
pub enum PdfToOfficeOutput {
    /// A single converted file with the given extension.
    Single { extension: String },
    /// Multiple output files bundled into a ZIP archive.
    Zip,
}

/// Which engine performs an office → PDF conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfficeEngine {
    /// `soffice --headless --convert-to pdf`.
    LibreOffice,
    /// The bundled pure-Rust engine, run out-of-process.
    Builtin,
}

impl OfficeEngine {
    /// The value reported to callers on the response header.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LibreOffice => "libreoffice",
            Self::Builtin => "builtin",
        }
    }
}

/// The operator's engine preference, from `RUSTLING_PROCESSING_OFFICE_ENGINE`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnginePreference {
    /// `LibreOffice` when it is installed, otherwise the built-in engine.
    Auto,
    /// `LibreOffice` only; fail when it is missing.
    LibreOffice,
    /// The built-in engine only, even when `LibreOffice` is installed.
    Builtin,
}

fn engine_preference() -> Result<EnginePreference, OfficeToPdfError> {
    let Ok(configured) = crate::environment::var(OFFICE_ENGINE_ENV) else {
        return Ok(EnginePreference::Auto);
    };
    parse_engine_preference(&configured)
}

/// An unrecognised value is refused rather than silently treated as `auto`: an
/// operator who set the variable meant something by it, and quietly ignoring a
/// typo would send documents to the engine they were trying to avoid.
fn parse_engine_preference(configured: &str) -> Result<EnginePreference, OfficeToPdfError> {
    match configured.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(EnginePreference::Auto),
        "libreoffice" | "soffice" => Ok(EnginePreference::LibreOffice),
        "builtin" | "office2pdf" => Ok(EnginePreference::Builtin),
        other => Err(OfficeToPdfError::InvalidEngine(other.to_owned())),
    }
}

/// What a completed office → PDF conversion has to say for itself.
#[derive(Debug, Clone)]
pub struct OfficeConversion {
    /// The engine that produced the PDF.
    pub engine: OfficeEngine,
    /// Non-fatal problems the engine reported, plus any derived by comparing
    /// the output against the input.
    pub warnings: Vec<String>,
    /// Whether source content is known to be missing from the PDF.
    pub dropped_content: bool,
}

/// Converts an office/text document at `input_path` (named `filename`) to a PDF
/// written to `output_path`.
///
/// `LibreOffice` is used when it is installed; otherwise the built-in engine
/// handles DOCX, XLSX, and PPTX. `RUSTLING_PROCESSING_OFFICE_ENGINE` forces one
/// or the other.
///
/// # Errors
///
/// Returns [`OfficeToPdfError`] for unsupported extensions, when no engine can
/// read the format, when the built-in engine exceeds one of its bounds, or when
/// the conversion produces no usable PDF.
pub fn convert_office_to_pdf(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<OfficeConversion, OfficeToPdfError> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| !value.is_empty())
        .ok_or(OfficeToPdfError::MissingExtension)?;
    if !ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(OfficeToPdfError::InvalidExtension(extension));
    }
    let preference = engine_preference()?;
    if preference == EnginePreference::Builtin
        && !office_builtin_engine::supports_extension(&extension)
    {
        return Err(OfficeToPdfError::NoEngineForExtension { extension });
    }

    // Sanitize once, into a workspace both engines read from. The built-in
    // engine gets exactly the same rewritten package LibreOffice would.
    let work_dir = TempDir::new()?;
    let base_name = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("input");
    let input_copy = work_dir.path().join(format!("{base_name}.{extension}"));
    prepare_conversion_input(input_path, &input_copy, &extension)?;

    if preference == EnginePreference::Builtin {
        return run_builtin(&input_copy, &extension, output_path);
    }

    match convert_with_libreoffice(&work_dir, &input_copy, base_name, output_path) {
        Ok(()) => Ok(OfficeConversion {
            engine: OfficeEngine::LibreOffice,
            warnings: Vec::new(),
            dropped_content: false,
        }),
        // `LibreOffice` is genuinely absent. In `auto` this is the ordinary case
        // on a machine that never installed it, so fall through to the built-in
        // engine when it can read the format; a conversion failure or a crash of
        // an *installed* LibreOffice is not retried, because repeating the work
        // on a second engine would hide the real diagnostic.
        Err(OfficeToPdfError::SofficeUnavailable) if preference == EnginePreference::Auto => {
            if office_builtin_engine::supports_extension(&extension) {
                run_builtin(&input_copy, &extension, output_path)
            } else {
                Err(OfficeToPdfError::NoEngineForExtension { extension })
            }
        }
        Err(error) => Err(error),
    }
}

fn run_builtin(
    input_copy: &Path,
    extension: &str,
    output_path: &Path,
) -> Result<OfficeConversion, OfficeToPdfError> {
    let BuiltinConversion {
        warnings,
        dropped_content,
    } = office_builtin_engine::convert(input_copy, extension, output_path)?;
    Ok(OfficeConversion {
        engine: OfficeEngine::Builtin,
        warnings,
        dropped_content,
    })
}

fn convert_with_libreoffice(
    work_dir: &TempDir,
    input_copy: &Path,
    base_name: &str,
    output_path: &Path,
) -> Result<(), OfficeToPdfError> {
    let profile_dir = TempDir::new()?;
    let user_installation = format!(
        "-env:UserInstallation={}",
        path_to_file_uri(profile_dir.path())
    );
    let mut arguments: Vec<OsString> = vec![
        OsString::from(user_installation),
        OsString::from("--headless"),
        OsString::from("--nologo"),
        OsString::from("--convert-to"),
        OsString::from("pdf"),
        OsString::from("--outdir"),
        work_dir.path().as_os_str().to_owned(),
    ];
    arguments.push(input_copy.as_os_str().to_owned());

    run_soffice(&arguments)?;

    let produced = work_dir.path().join(format!("{base_name}.pdf"));
    let produced = if produced.is_file() {
        produced
    } else {
        find_pdf(work_dir.path())?.ok_or(OfficeToPdfError::NoOutput)?
    };
    if fs::metadata(&produced)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        == 0
    {
        return Err(OfficeToPdfError::NoOutput);
    }
    fs::copy(&produced, output_path)?;
    Ok(())
}

/// Converts a PDF at `input_path` to an office format via `LibreOffice`, using the
/// given import `filter` (`writer_pdf_import` or `impress_pdf_import`). The result is
/// written to `output_path` (a single file, or a ZIP when `LibreOffice` emits several).
///
/// # Errors
///
/// Returns [`OfficeToPdfError`] for unsupported formats, when `LibreOffice` is
/// unavailable, or when the conversion produces no usable output.
pub fn convert_pdf_to_office(
    input_path: &Path,
    filename: &str,
    output_format: &str,
    filter: &str,
    output_path: &Path,
) -> Result<PdfToOfficeOutput, OfficeToPdfError> {
    let output_format = output_format.trim();
    if !ALLOWED_OFFICE_FORMATS.contains(&output_format) {
        return Err(OfficeToPdfError::InvalidOutputFormat(
            output_format.to_owned(),
        ));
    }

    let work_dir = TempDir::new()?;
    let profile_dir = TempDir::new()?;
    let base_name = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("input");
    let input_copy = work_dir.path().join(format!("{base_name}.pdf"));
    fs::copy(input_path, &input_copy)?;

    let user_installation = format!(
        "-env:UserInstallation={}",
        path_to_file_uri(profile_dir.path())
    );
    let arguments: Vec<OsString> = vec![
        OsString::from(user_installation),
        OsString::from("--headless"),
        OsString::from("--nologo"),
        OsString::from(format!("--infilter={filter}")),
        OsString::from("--convert-to"),
        OsString::from(output_format),
        OsString::from("--outdir"),
        work_dir.path().as_os_str().to_owned(),
        input_copy.as_os_str().to_owned(),
    ];
    run_soffice(&arguments)?;

    let mut outputs = Vec::new();
    for entry in fs::read_dir(work_dir.path())? {
        let path = entry?.path();
        if path == input_copy || !path.is_file() {
            continue;
        }
        outputs.push(path);
    }
    outputs.sort();
    match outputs.as_slice() {
        [] => Err(OfficeToPdfError::NoOutput),
        [produced] => {
            if fs::metadata(produced)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                == 0
            {
                return Err(OfficeToPdfError::NoOutput);
            }
            fs::copy(produced, output_path)?;
            let extension = produced
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or(output_format)
                .to_owned();
            Ok(PdfToOfficeOutput::Single { extension })
        }
        many => {
            let file = File::create(output_path)?;
            let mut zip = ZipWriter::new(file);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for path in many {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("output");
                zip.start_file(name, options)?;
                zip.write_all(&fs::read(path)?)?;
            }
            zip.finish()?;
            Ok(PdfToOfficeOutput::Zip)
        }
    }
}

fn run_soffice(arguments: &[OsString]) -> Result<(), OfficeToPdfError> {
    for command in soffice_commands() {
        match Command::new(&command).args(arguments).output() {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => {
                return Err(OfficeToPdfError::SofficeFailed {
                    command,
                    status: exit_status(output.status),
                    details: process_details(&output.stdout, &output.stderr),
                });
            }
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => return Err(OfficeToPdfError::SofficeStart { command, source }),
        }
    }
    Err(OfficeToPdfError::SofficeUnavailable)
}

fn prepare_conversion_input(
    input_path: &Path,
    output_path: &Path,
    extension: &str,
) -> Result<(), OfficeToPdfError> {
    if extension == "html" || extension == "htm" {
        let html = fs::read(input_path)?;
        Ok(fs::write(
            output_path,
            sanitize_html(&String::from_utf8_lossy(&html)).as_bytes(),
        )?)
    } else if is_sanitizable_extension(extension) {
        sanitize_office_archive(input_path, output_path)
            .map_err(|error| OfficeToPdfError::UnsafeArchive(error.to_string()))
    } else {
        Ok(fs::copy(input_path, output_path).map(|_| ())?)
    }
}

fn soffice_commands() -> Vec<String> {
    if let Ok(command) = crate::environment::var(SOFFICE_COMMAND_ENV)
        && !command.trim().is_empty()
    {
        return vec![command];
    }
    if cfg!(windows) {
        vec![
            "soffice.com".to_owned(),
            "soffice.exe".to_owned(),
            "soffice".to_owned(),
        ]
    } else {
        vec!["soffice".to_owned(), "/usr/bin/soffice".to_owned()]
    }
}

fn find_pdf(directory: &Path) -> Result<Option<PathBuf>, OfficeToPdfError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("pdf"))
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn path_to_file_uri(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
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
    use super::{
        EnginePreference, OfficeToPdfError, convert_office_to_pdf, convert_pdf_to_office,
        parse_engine_preference, path_to_file_uri, prepare_conversion_input,
    };
    use std::{fs, path::Path};
    use tempfile::tempdir;

    #[test]
    fn rejects_unknown_output_format() {
        assert!(matches!(
            convert_pdf_to_office(
                Path::new("input.pdf"),
                "doc.pdf",
                "pages",
                "writer_pdf_import",
                Path::new("out.docx")
            ),
            Err(OfficeToPdfError::InvalidOutputFormat(fmt)) if fmt == "pages"
        ));
    }

    #[test]
    fn rejects_missing_extension() {
        assert!(matches!(
            convert_office_to_pdf(Path::new("input"), "document", Path::new("out.pdf")),
            Err(OfficeToPdfError::MissingExtension)
        ));
    }

    #[test]
    fn rejects_unknown_extension() {
        assert!(matches!(
            convert_office_to_pdf(Path::new("input.xyz"), "document.xyz", Path::new("out.pdf")),
            Err(OfficeToPdfError::InvalidExtension(ext)) if ext == "xyz"
        ));
    }

    #[test]
    fn parses_every_engine_preference_spelling() {
        for (configured, expected) in [
            ("", EnginePreference::Auto),
            ("  ", EnginePreference::Auto),
            ("auto", EnginePreference::Auto),
            ("AUTO", EnginePreference::Auto),
            ("libreoffice", EnginePreference::LibreOffice),
            ("soffice", EnginePreference::LibreOffice),
            (" Builtin ", EnginePreference::Builtin),
            ("office2pdf", EnginePreference::Builtin),
        ] {
            assert_eq!(
                parse_engine_preference(configured).ok(),
                Some(expected),
                "{configured:?}"
            );
        }
    }

    #[test]
    fn refuses_an_unknown_engine_preference() {
        assert!(matches!(
            parse_engine_preference("typo"),
            Err(OfficeToPdfError::InvalidEngine(value)) if value == "typo"
        ));
    }

    #[test]
    fn builds_a_file_uri() {
        assert!(path_to_file_uri(Path::new("/tmp/profile")).starts_with("file:///tmp/profile"));
    }

    #[test]
    fn sanitizes_html_before_libreoffice_reads_it() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let input = directory.path().join("input.html");
        let output = directory.path().join("sanitized.html");
        fs::write(
            &input,
            "<h1>Safe</h1><script>alert(1)</script><img src=https://internal/secret>",
        )?;
        prepare_conversion_input(&input, &output, "html")?;
        let sanitized = fs::read_to_string(output)?;
        assert!(sanitized.contains("<h1>Safe</h1>"));
        assert!(!sanitized.contains("script"));
        assert!(!sanitized.contains("internal"));
        Ok(())
    }
}
