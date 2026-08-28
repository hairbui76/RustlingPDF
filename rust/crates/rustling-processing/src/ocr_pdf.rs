//! PDF OCR via `OCRmyPDF` or per-page Tesseract.
//!
//! `OCRmyPDF` remains preferred. When it is disabled or unavailable, `PDFium`
//! renders the Java-compatible page selection and Tesseract produces searchable
//! one-page PDFs that are merged back in source order.

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Output,
    sync::Arc,
};

use tempfile::TempDir;
use thiserror::Error;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    paddle_ocr::{PaddleOcrError, PaddleOcrService},
    pdf_document_ops::{DocumentOperationError, strip_images_to_file},
    pdf_merge::{MergeError, MergeInput, MergeOptions, merge_pdf_paths_to_file},
    pdfium_backend::{
        PdfiumOcrError, PdfiumOcrMode, PdfiumOcrPrepareAttempt, PdfiumOcrTextLine,
        try_prepare_tesseract_pages, write_paddle_searchable_page,
    },
    process_executor::{ProcessExecutor, ProcessExecutorError, exit_status},
    runtime_config::OcrProcessSettings,
    tessdata::available_tesseract_languages,
};

const OCRMYPDF_COMMAND_ENV: &str = "RUSTLING_PROCESSING_OCRMYPDF_COMMAND";
const TESSERACT_COMMAND_ENV: &str = "RUSTLING_PROCESSING_TESSERACT_COMMAND";

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // mirrors the OCRmyPDF request flags
pub struct OcrOptions {
    pub languages: Vec<String>,
    pub sidecar: bool,
    pub deskew: bool,
    pub clean: bool,
    pub clean_final: bool,
    pub ocr_type: Option<String>,
    pub ocr_render_type: String,
    pub remove_images_after: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct OcrRuntime {
    pub ocrmypdf_enabled: bool,
    pub tesseract_enabled: bool,
    pub tessdata_dir: PathBuf,
    pub render_dpi: i32,
    pub ocrmypdf_commands: Option<Vec<String>>,
    pub tesseract_commands: Option<Vec<String>>,
    pub process_controls: Arc<OcrProcessControls>,
    pub paddle_ocr_service: Arc<PaddleOcrService>,
}

#[derive(Clone, Debug)]
pub(crate) struct OcrProcessControls {
    ocrmypdf: ProcessExecutor,
    tesseract: ProcessExecutor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrOutput {
    /// A single OCR'd PDF.
    Pdf,
    /// A ZIP holding the OCR'd PDF and its sidecar text file.
    Zip,
}

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("OCR language options are not specified")]
    NoLanguages,
    #[error("Invalid OCR languages format: none of the selected languages are valid")]
    InvalidLanguages,
    #[error("Invalid OCR render type. Must be 'hocr' or 'sandwich'")]
    InvalidRenderType,
    #[error("OCRmyPDF was not found")]
    OcrMyPdfUnavailable,
    #[error("OCR tools are not installed")]
    OcrToolsUnavailable,
    #[error("OCRmyPDF with '{command}' failed with status {status}: {details}")]
    OcrMyPdfFailed {
        command: String,
        status: String,
        details: String,
    },
    #[error("could not start OCRmyPDF command '{command}': {source}")]
    OcrMyPdfStart {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("OCRmyPDF with '{command}' timed out after {timeout_minutes} minutes")]
    OcrMyPdfTimeout {
        command: String,
        timeout_minutes: u64,
    },
    #[error("Tesseract with '{command}' failed with status {status}: {details}")]
    TesseractFailed {
        command: String,
        status: String,
        details: String,
    },
    #[error("could not start Tesseract command '{command}': {source}")]
    TesseractStart {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Tesseract with '{command}' timed out after {timeout_minutes} minutes")]
    TesseractTimeout {
        command: String,
        timeout_minutes: u64,
    },
    #[error(transparent)]
    Paddle(#[from] PaddleOcrError),
    #[error("PDFium is required for the Tesseract OCR fallback: {details}")]
    PdfiumUnavailable { details: String },
    #[error(transparent)]
    Pdfium(#[from] PdfiumOcrError),
    #[error(transparent)]
    Merge(#[from] MergeError),
    #[error("could not remove images after OCR: {0}")]
    RemoveImages(#[from] DocumentOperationError),
    #[error("could not prepare the OCR workspace: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not build the output archive: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Runs OCR over `input_path` and writes the result to `output_path`.
///
/// Returns [`OcrOutput::Zip`] when a sidecar text file was requested (the PDF and
/// `.txt` are bundled), otherwise [`OcrOutput::Pdf`].
///
/// # Errors
///
/// Returns [`OcrError`] for invalid options, when no configured OCR tool is
/// available, or when rendering, OCR, merging, or optional image removal fails.
pub(crate) fn run_ocr(
    input_path: &Path,
    output_path: &Path,
    options: &OcrOptions,
    runtime: &OcrRuntime,
) -> Result<OcrOutput, OcrError> {
    let render_type = validated_request_shape(options)?;
    let languages = if runtime.paddle_ocr_service.is_selected() {
        options.languages.clone()
    } else {
        validated_tesseract_languages(options, &runtime.tessdata_dir)?
    };

    let work_dir = TempDir::new()?;
    let ocr_pdf = work_dir.path().join("ocr-output.pdf");
    let sidecar_txt = work_dir.path().join("ocr-sidecar.txt");

    let used_ocrmypdf = if runtime.paddle_ocr_service.is_selected() {
        run_paddle_fallback(
            input_path,
            &ocr_pdf,
            &sidecar_txt,
            options,
            runtime,
            work_dir.path(),
        )?;
        false
    } else if runtime.ocrmypdf_enabled {
        let arguments = ocrmypdf_arguments(
            input_path,
            &ocr_pdf,
            &sidecar_txt,
            options,
            &languages,
            render_type,
        );
        match run_ocrmypdf(
            &arguments,
            &runtime.resolved_ocrmypdf_commands(),
            &runtime.process_controls.ocrmypdf,
        ) {
            Ok(()) => true,
            Err(OcrError::OcrMyPdfUnavailable) if runtime.tesseract_enabled => {
                run_tesseract_fallback(
                    input_path,
                    &ocr_pdf,
                    &sidecar_txt,
                    options,
                    &languages,
                    runtime,
                    work_dir.path(),
                )?;
                false
            }
            Err(OcrError::OcrMyPdfUnavailable) => return Err(OcrError::OcrToolsUnavailable),
            Err(error) => return Err(error),
        }
    } else if runtime.tesseract_enabled {
        run_tesseract_fallback(
            input_path,
            &ocr_pdf,
            &sidecar_txt,
            options,
            &languages,
            runtime,
            work_dir.path(),
        )?;
        false
    } else {
        return Err(OcrError::OcrToolsUnavailable);
    };

    if used_ocrmypdf && options.remove_images_after {
        remove_images(&ocr_pdf, work_dir.path())?;
    }

    if options.sidecar {
        write_sidecar_zip(&ocr_pdf, &sidecar_txt, output_path)?;
        Ok(OcrOutput::Zip)
    } else {
        fs::copy(&ocr_pdf, output_path)?;
        Ok(OcrOutput::Pdf)
    }
}

impl OcrRuntime {
    fn resolved_ocrmypdf_commands(&self) -> Vec<String> {
        self.ocrmypdf_commands
            .clone()
            .unwrap_or_else(default_ocrmypdf_commands)
    }

    fn resolved_tesseract_commands(&self) -> Vec<String> {
        self.tesseract_commands
            .clone()
            .unwrap_or_else(default_tesseract_commands)
    }
}

impl OcrProcessControls {
    pub(crate) fn new(settings: OcrProcessSettings) -> Self {
        Self {
            ocrmypdf: ProcessExecutor::new(
                settings.ocrmypdf_session_limit,
                settings.ocrmypdf_timeout,
            ),
            tesseract: ProcessExecutor::new(
                settings.tesseract_session_limit,
                settings.tesseract_timeout,
            ),
        }
    }
}

fn ocrmypdf_arguments(
    input_path: &Path,
    output_path: &Path,
    sidecar_path: &Path,
    options: &OcrOptions,
    languages: &[String],
    render_type: &str,
) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--verbose"),
        OsString::from("2"),
        OsString::from("--output-type"),
        OsString::from("pdf"),
        OsString::from("--pdf-renderer"),
        OsString::from(render_type),
    ];
    if options.sidecar {
        arguments.push(OsString::from("--sidecar"));
        arguments.push(sidecar_path.as_os_str().to_owned());
    }
    if options.deskew {
        arguments.push(OsString::from("--deskew"));
    }
    if options.clean {
        arguments.push(OsString::from("--clean"));
    }
    if options.clean_final {
        arguments.push(OsString::from("--clean-final"));
    }
    match options.ocr_type.as_deref() {
        Some("force-ocr") => arguments.push(OsString::from("--force-ocr")),
        Some(value) if !value.is_empty() => arguments.push(OsString::from("--skip-text")),
        _ => {}
    }
    arguments.push(OsString::from("--invalidate-digital-signatures"));
    arguments.push(OsString::from("--language"));
    arguments.push(OsString::from(languages.join("+")));
    arguments.push(input_path.as_os_str().to_owned());
    arguments.push(output_path.as_os_str().to_owned());
    arguments
}

fn validated_request_shape(options: &OcrOptions) -> Result<&str, OcrError> {
    if options.languages.is_empty() {
        return Err(OcrError::NoLanguages);
    }
    let render_type = options.ocr_render_type.as_str();
    if render_type != "hocr" && render_type != "sandwich" {
        return Err(OcrError::InvalidRenderType);
    }
    Ok(render_type)
}

fn validated_tesseract_languages(
    options: &OcrOptions,
    tessdata_dir: &Path,
) -> Result<Vec<String>, OcrError> {
    let available_languages = available_tesseract_languages(tessdata_dir);
    let languages = options
        .languages
        .iter()
        .filter(|language| available_languages.contains(language))
        .cloned()
        .collect::<Vec<_>>();
    if languages.is_empty() {
        return Err(OcrError::InvalidLanguages);
    }

    Ok(languages)
}

/// Chooses which pages an engine OCRs, matching the Tesseract fallback rule.
///
/// `skip-text` retains pages that already carry text; every other value —
/// including `force-ocr`, `Normal`, and an absent value — OCRs every page.
fn ocr_page_selection_mode(options: &OcrOptions) -> PdfiumOcrMode {
    if options.ocr_type.as_deref() == Some("skip-text") {
        PdfiumOcrMode::SkipTextPages
    } else {
        PdfiumOcrMode::AllPages
    }
}

fn run_paddle_fallback(
    input_path: &Path,
    output_path: &Path,
    sidecar_path: &Path,
    options: &OcrOptions,
    runtime: &OcrRuntime,
    work_dir: &Path,
) -> Result<(), OcrError> {
    let font_path = runtime.paddle_ocr_service.font_path()?.clone();
    let images_dir = work_dir.join("paddle-images");
    let pages_dir = work_dir.join("paddle-pages");
    fs::create_dir_all(&images_dir)?;
    fs::create_dir_all(&pages_dir)?;
    let mode = ocr_page_selection_mode(options);
    let filename = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.pdf");
    let artifacts = match try_prepare_tesseract_pages(
        input_path,
        filename,
        mode,
        runtime.render_dpi,
        &images_dir,
        &pages_dir,
    )? {
        PdfiumOcrPrepareAttempt::Prepared(artifacts) => artifacts,
        PdfiumOcrPrepareAttempt::Unavailable {
            explicitly_configured,
            details,
        } => {
            let details = if explicitly_configured {
                format!("the explicitly configured PDFium runtime could not load: {details}")
            } else {
                details
            };
            return Err(OcrError::PdfiumUnavailable { details });
        }
    };

    let mut pages = Vec::with_capacity(artifacts.len());
    let mut sidecar = String::new();
    for artifact in artifacts {
        let selected_path = if let Some(image_path) = &artifact.image_path {
            let encoded_image = fs::read(image_path)?;
            let recognized = runtime.paddle_ocr_service.recognize_image(&encoded_image)?;
            let lines = recognized
                .iter()
                .map(|line| PdfiumOcrTextLine {
                    text: line.text.clone(),
                    points: line.points,
                })
                .collect::<Vec<_>>();
            write_paddle_searchable_page(
                image_path,
                &font_path,
                artifact.page_number,
                artifact.page_width,
                artifact.page_height,
                &lines,
                &artifact.expected_ocr_pdf_path,
            )?;
            for line in recognized {
                sidecar.push_str(&line.text);
                sidecar.push('\n');
            }
            artifact.expected_ocr_pdf_path
        } else {
            artifact.original_pdf_path
        };
        pages.push(MergeInput {
            filename: format!("page-{}.pdf", artifact.page_number),
            path: selected_path,
        });
    }
    merge_pdf_paths_to_file(&pages, MergeOptions::default(), output_path)?;
    if options.sidecar {
        fs::write(sidecar_path, sidecar)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_tesseract_fallback(
    input_path: &Path,
    output_path: &Path,
    sidecar_path: &Path,
    options: &OcrOptions,
    languages: &[String],
    runtime: &OcrRuntime,
    work_dir: &Path,
) -> Result<(), OcrError> {
    let images_dir = work_dir.join("tesseract-images");
    let pages_dir = work_dir.join("tesseract-pages");
    fs::create_dir_all(&images_dir)?;
    fs::create_dir_all(&pages_dir)?;
    let mode = if options.ocr_type.as_deref() == Some("skip-text") {
        PdfiumOcrMode::SkipTextPages
    } else {
        PdfiumOcrMode::AllPages
    };
    let filename = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.pdf");
    let artifacts = match try_prepare_tesseract_pages(
        input_path,
        filename,
        mode,
        runtime.render_dpi,
        &images_dir,
        &pages_dir,
    )? {
        PdfiumOcrPrepareAttempt::Prepared(artifacts) => artifacts,
        PdfiumOcrPrepareAttempt::Unavailable {
            explicitly_configured,
            details,
        } => {
            let details = if explicitly_configured {
                format!("the explicitly configured PDFium runtime could not load: {details}")
            } else {
                details
            };
            return Err(OcrError::PdfiumUnavailable { details });
        }
    };

    let mut pages = Vec::with_capacity(artifacts.len());
    let tesseract_commands = runtime.resolved_tesseract_commands();
    for artifact in artifacts {
        let selected_path = if let Some(image_path) = &artifact.image_path {
            run_tesseract_page(
                image_path,
                &artifact.ocr_output_base,
                languages,
                &tesseract_commands,
                &runtime.process_controls.tesseract,
            )?;
            if artifact.expected_ocr_pdf_path.is_file() {
                artifact.expected_ocr_pdf_path
            } else {
                artifact.original_pdf_path
            }
        } else {
            artifact.original_pdf_path
        };
        pages.push(MergeInput {
            filename: format!("page-{}.pdf", artifact.page_number),
            path: selected_path,
        });
    }
    merge_pdf_paths_to_file(&pages, MergeOptions::default(), output_path)?;
    if options.sidecar {
        fs::write(sidecar_path, [])?;
    }
    Ok(())
}

fn run_tesseract_page(
    image_path: &Path,
    output_base: &Path,
    languages: &[String],
    commands: &[String],
    executor: &ProcessExecutor,
) -> Result<(), OcrError> {
    let arguments = [
        image_path.as_os_str().to_owned(),
        output_base.as_os_str().to_owned(),
        OsString::from("-l"),
        OsString::from(languages.join("+")),
        OsString::from("pdf"),
    ];
    for command in commands {
        match executor.run(command, &arguments) {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => {
                return Err(OcrError::TesseractFailed {
                    command: command.clone(),
                    status: exit_status(output.status),
                    details: process_details(&output.stdout, &output.stderr),
                });
            }
            Err(ProcessExecutorError::Start(source)) if source.kind() == ErrorKind::NotFound => {}
            Err(ProcessExecutorError::Start(source) | ProcessExecutorError::Output(source)) => {
                return Err(OcrError::TesseractStart {
                    command: command.clone(),
                    source,
                });
            }
            Err(ProcessExecutorError::Timeout { timeout }) => {
                return Err(OcrError::TesseractTimeout {
                    command: command.clone(),
                    timeout_minutes: timeout_minutes(timeout),
                });
            }
            // The OCR executors set no memory limit, so this arm is unreachable;
            // it exists because `ProcessExecutorError` is shared with the built-in
            // Office engine, which does bound its worker's resident set.
            Err(ProcessExecutorError::MemoryLimit { limit_bytes }) => {
                return Err(OcrError::TesseractStart {
                    command: command.clone(),
                    source: std::io::Error::other(format!(
                        "the process exceeded its {limit_bytes}-byte memory limit"
                    )),
                });
            }
        }
    }
    Err(OcrError::OcrToolsUnavailable)
}

fn run_ocrmypdf(
    arguments: &[OsString],
    commands: &[String],
    executor: &ProcessExecutor,
) -> Result<(), OcrError> {
    for command in commands {
        match executor.run(command, arguments) {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) if should_retry_ocrmypdf(&output) => {
                let mut retry_arguments = arguments.to_vec();
                retry_arguments.push(OsString::from("--jobs"));
                retry_arguments.push(OsString::from("1"));
                let retry = run_ocrmypdf_command(command, &retry_arguments, executor)?;
                if retry.status.success() {
                    return Ok(());
                }
                return Err(ocrmypdf_failure(command.clone(), &retry));
            }
            Ok(output) => return Err(ocrmypdf_failure(command.clone(), &output)),
            Err(ProcessExecutorError::Start(source)) if source.kind() == ErrorKind::NotFound => {}
            Err(ProcessExecutorError::Start(source) | ProcessExecutorError::Output(source)) => {
                return Err(OcrError::OcrMyPdfStart {
                    command: command.clone(),
                    source,
                });
            }
            Err(ProcessExecutorError::Timeout { timeout }) => {
                return Err(OcrError::OcrMyPdfTimeout {
                    command: command.clone(),
                    timeout_minutes: timeout_minutes(timeout),
                });
            }
            // The OCR executors set no memory limit, so this arm is unreachable;
            // it exists because `ProcessExecutorError` is shared with the built-in
            // Office engine, which does bound its worker's resident set.
            Err(ProcessExecutorError::MemoryLimit { limit_bytes }) => {
                return Err(OcrError::OcrMyPdfStart {
                    command: command.clone(),
                    source: std::io::Error::other(format!(
                        "the process exceeded its {limit_bytes}-byte memory limit"
                    )),
                });
            }
        }
    }
    Err(OcrError::OcrMyPdfUnavailable)
}

fn run_ocrmypdf_command(
    command: &str,
    arguments: &[OsString],
    executor: &ProcessExecutor,
) -> Result<Output, OcrError> {
    match executor.run(command, arguments) {
        Ok(output) => Ok(output),
        Err(ProcessExecutorError::Start(source) | ProcessExecutorError::Output(source)) => {
            Err(OcrError::OcrMyPdfStart {
                command: command.to_owned(),
                source,
            })
        }
        Err(ProcessExecutorError::Timeout { timeout }) => Err(OcrError::OcrMyPdfTimeout {
            command: command.to_owned(),
            timeout_minutes: timeout_minutes(timeout),
        }),
        // Unreachable here for the same reason as above.
        Err(ProcessExecutorError::MemoryLimit { limit_bytes }) => Err(OcrError::OcrMyPdfStart {
            command: command.to_owned(),
            source: std::io::Error::other(format!(
                "the process exceeded its {limit_bytes}-byte memory limit"
            )),
        }),
    }
}

fn timeout_minutes(timeout: std::time::Duration) -> u64 {
    u64::try_from(timeout.as_millis().div_ceil(60_000))
        .unwrap_or(u64::MAX)
        .max(1)
}

fn should_retry_ocrmypdf(output: &Output) -> bool {
    is_multiprocessing_unavailable(&output.stdout, &output.stderr)
}

fn is_multiprocessing_unavailable(stdout: &[u8], stderr: &[u8]) -> bool {
    let details = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    details.contains("multiprocessing/synchronize.py")
        && details.contains("OSError: [Errno 38] Function not implemented")
}

fn ocrmypdf_failure(command: String, output: &Output) -> OcrError {
    OcrError::OcrMyPdfFailed {
        command,
        status: exit_status(output.status),
        details: process_details(&output.stdout, &output.stderr),
    }
}

/// Removes every raster image from an OCR'd PDF, keeping the searchable text
/// layer OCR just added.
///
/// This replaces the Ghostscript `-dFILTERIMAGE` pass: image `XObject` resources,
/// the `Do` operators painting them, and inline images are dropped in-process
/// with `lopdf`, and the orphaned image streams are pruned so their bytes leave
/// the file.
fn remove_images(ocr_pdf: &Path, work_dir: &Path) -> Result<(), OcrError> {
    let stripped = work_dir.join("ocr-no-images.pdf");
    strip_images_to_file(ocr_pdf, "document.pdf", &stripped)?;
    fs::copy(&stripped, ocr_pdf)?;
    Ok(())
}

fn write_sidecar_zip(
    ocr_pdf: &Path,
    sidecar_txt: &Path,
    output_path: &Path,
) -> Result<(), OcrError> {
    let file = File::create(output_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("document_OCR.pdf", options)?;
    zip.write_all(&fs::read(ocr_pdf)?)?;
    let sidecar_bytes = fs::read(sidecar_txt).unwrap_or_default();
    zip.start_file("document_OCR.txt", options)?;
    zip.write_all(&sidecar_bytes)?;
    zip.finish()?;
    Ok(())
}

fn default_ocrmypdf_commands() -> Vec<String> {
    if let Ok(command) = crate::environment::var(OCRMYPDF_COMMAND_ENV)
        && !command.trim().is_empty()
    {
        return vec![command];
    }
    if cfg!(windows) {
        vec!["ocrmypdf.exe".to_owned(), "ocrmypdf".to_owned()]
    } else {
        vec!["ocrmypdf".to_owned()]
    }
}

fn default_tesseract_commands() -> Vec<String> {
    if let Ok(command) = crate::environment::var(TESSERACT_COMMAND_ENV)
        && !command.trim().is_empty()
    {
        return vec![command];
    }
    if cfg!(windows) {
        vec!["tesseract.exe".to_owned(), "tesseract".to_owned()]
    } else {
        vec!["tesseract".to_owned()]
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
        fs,
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    #[cfg(unix)]
    use lopdf::{Document, Object, Stream, dictionary};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    use super::{
        OcrError, OcrOptions, OcrProcessControls, OcrRuntime, is_multiprocessing_unavailable,
        ocr_page_selection_mode, ocrmypdf_arguments, run_ocr, run_ocrmypdf,
        validated_request_shape, validated_tesseract_languages,
    };
    #[cfg(unix)]
    use crate::process_executor::ProcessExecutor;
    use crate::runtime_config::OcrProcessSettings;
    use crate::{
        paddle_ocr::{PaddleOcrConfig, PaddleOcrService},
        pdfium_backend::PdfiumOcrMode,
    };

    fn options() -> OcrOptions {
        OcrOptions {
            languages: vec!["eng".to_owned()],
            sidecar: false,
            deskew: false,
            clean: false,
            clean_final: false,
            ocr_type: None,
            ocr_render_type: "hocr".to_owned(),
            remove_images_after: false,
        }
    }

    fn runtime(tessdata_dir: &Path) -> OcrRuntime {
        OcrRuntime {
            ocrmypdf_enabled: false,
            tesseract_enabled: false,
            tessdata_dir: tessdata_dir.to_path_buf(),
            render_dpi: 500,
            ocrmypdf_commands: None,
            tesseract_commands: None,
            process_controls: Arc::new(OcrProcessControls::new(OcrProcessSettings {
                ocrmypdf_session_limit: 2,
                ocrmypdf_timeout: Duration::from_secs(30 * 60),
                tesseract_session_limit: 1,
                tesseract_timeout: Duration::from_secs(30 * 60),
            })),
            paddle_ocr_service: Arc::new(PaddleOcrService::from_config(Ok(None))),
        }
    }

    #[test]
    fn requires_at_least_one_language() {
        let mut request = options();
        request.languages.clear();
        assert!(matches!(
            run_ocr(
                Path::new("in.pdf"),
                Path::new("out.pdf"),
                &request,
                &runtime(Path::new("missing-tessdata")),
            ),
            Err(OcrError::NoLanguages)
        ));
    }

    #[test]
    fn does_not_trim_language_or_render_values() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        fs::write(directory.path().join("eng.traineddata"), "test")?;
        let mut request = options();
        request.languages = vec![" eng ".to_owned()];
        assert!(matches!(
            validated_tesseract_languages(&request, directory.path()),
            Err(OcrError::InvalidLanguages)
        ));

        request.languages = vec!["eng".to_owned()];
        request.ocr_render_type = " hocr ".to_owned();
        assert!(matches!(
            validated_request_shape(&request),
            Err(OcrError::InvalidRenderType)
        ));
        Ok(())
    }

    #[test]
    fn rejects_invalid_render_type() {
        let mut request = options();
        request.ocr_render_type = "fancy".to_owned();
        assert!(matches!(
            run_ocr(
                Path::new("in.pdf"),
                Path::new("out.pdf"),
                &request,
                &runtime(Path::new("missing-tessdata")),
            ),
            Err(OcrError::InvalidRenderType)
        ));
    }

    #[test]
    fn rejects_requests_when_no_selected_language_is_installed()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        fs::write(directory.path().join("eng.traineddata"), "test")?;
        let mut request = options();
        request.languages = vec!["fra".to_owned(), "ENG".to_owned()];

        assert!(matches!(
            validated_tesseract_languages(&request, directory.path()),
            Err(OcrError::InvalidLanguages)
        ));
        Ok(())
    }

    #[test]
    fn filters_languages_case_sensitively_while_preserving_order_and_duplicates()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        fs::write(directory.path().join("eng.traineddata"), "test")?;
        fs::write(directory.path().join("deu.traineddata"), "test")?;
        let mut request = options();
        request.languages = vec![
            "deu".to_owned(),
            "fra".to_owned(),
            "eng".to_owned(),
            "deu".to_owned(),
            "ENG".to_owned(),
        ];

        let languages = validated_tesseract_languages(&request, directory.path())?;
        assert_eq!(languages, ["deu", "eng", "deu"]);
        assert_eq!(validated_request_shape(&request)?, "hocr");
        Ok(())
    }

    #[test]
    fn reports_unavailable_when_both_ocr_tools_are_disabled()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        fs::write(directory.path().join("eng.traineddata"), "test")?;

        assert!(matches!(
            run_ocr(
                Path::new("in.pdf"),
                Path::new("out.pdf"),
                &options(),
                &runtime(directory.path()),
            ),
            Err(OcrError::OcrToolsUnavailable)
        ));
        Ok(())
    }

    fn paddle_runtime(
        tessdata_dir: &Path,
        config: Result<Option<PaddleOcrConfig>, String>,
    ) -> OcrRuntime {
        let mut runtime = runtime(tessdata_dir);
        // Both default engines stay "available" so a fall-back would be visible
        // as a different error rather than as the same unavailability.
        runtime.ocrmypdf_enabled = true;
        runtime.tesseract_enabled = true;
        runtime.paddle_ocr_service = Arc::new(PaddleOcrService::from_config(config));
        runtime
    }

    fn complete_paddle_config() -> PaddleOcrConfig {
        PaddleOcrConfig {
            onnx_runtime_path: PathBuf::from("/opt/onnx/libonnxruntime.so"),
            detector_model_path: PathBuf::from("/models/detector.onnx"),
            recognizer_model_path: PathBuf::from("/models/recognizer.onnx"),
            dictionary_path: PathBuf::from("/models/ppocrv6_dict.txt"),
            text_layer_font_path: PathBuf::from("/fonts/NotoSansCJK-Regular.otf"),
        }
    }

    /// The default engine keeps its tessdata gate. Paddle must not have relaxed
    /// validation for requests that never select it.
    #[test]
    fn default_engine_still_rejects_languages_without_installed_tessdata()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let mut runtime = runtime(directory.path());
        runtime.tesseract_enabled = true;
        assert!(matches!(
            run_ocr(
                Path::new("in.pdf"),
                Path::new("out.pdf"),
                &options(),
                &runtime,
            ),
            Err(OcrError::InvalidLanguages)
        ));
        Ok(())
    }

    /// Paddle uses one pinned model and dictionary pair, so `languages` cannot
    /// be checked against tessdata. An empty tessdata directory must not block
    /// an explicitly selected Paddle engine.
    #[test]
    fn explicit_paddle_does_not_require_installed_tessdata()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let runtime = paddle_runtime(directory.path(), Ok(Some(complete_paddle_config())));
        let error = run_ocr(
            Path::new("in.pdf"),
            Path::new("out.pdf"),
            &options(),
            &runtime,
        )
        .err()
        .ok_or("Paddle cannot succeed without the operator artifacts")?;
        assert!(
            !matches!(
                error,
                OcrError::InvalidLanguages | OcrError::NoLanguages | OcrError::OcrToolsUnavailable
            ),
            "request failed on the default engine's checks: {error}"
        );
        Ok(())
    }

    /// Selecting Paddle does not loosen the shared request contract.
    #[test]
    fn explicit_paddle_keeps_request_shape_validation() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let runtime = paddle_runtime(directory.path(), Ok(Some(complete_paddle_config())));

        let mut request = options();
        request.languages.clear();
        assert!(matches!(
            run_ocr(
                Path::new("in.pdf"),
                Path::new("out.pdf"),
                &request,
                &runtime
            ),
            Err(OcrError::NoLanguages)
        ));

        let mut request = options();
        request.ocr_render_type = "fancy".to_owned();
        assert!(matches!(
            run_ocr(
                Path::new("in.pdf"),
                Path::new("out.pdf"),
                &request,
                &runtime
            ),
            Err(OcrError::InvalidRenderType)
        ));
        Ok(())
    }

    /// A misconfigured Paddle engine is a configuration failure, never a quiet
    /// hand-off to `OCRmyPDF` or Tesseract, and it is reported before any
    /// rendering work begins.
    #[test]
    fn invalid_paddle_configuration_never_falls_back() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let runtime = paddle_runtime(
            directory.path(),
            Err("ocr.paddle.detectorModelPath is required when ocr.engine is paddle".to_owned()),
        );
        let error = run_ocr(
            Path::new("in.pdf"),
            Path::new("out.pdf"),
            &options(),
            &runtime,
        )
        .err()
        .ok_or("an invalid Paddle configuration must fail")?;
        assert!(
            matches!(
                &error,
                OcrError::Paddle(paddle)
                    if paddle.to_string().contains("detectorModelPath")
            ),
            "expected a Paddle configuration error, got: {error}"
        );
        Ok(())
    }

    /// Paddle selects the same pages as the Tesseract fallback, so `skip-text`
    /// keeps retaining pages that already carry text.
    #[test]
    fn paddle_selects_pages_with_the_tesseract_fallback_rule() {
        let mut request = options();
        request.ocr_type = Some("skip-text".to_owned());
        assert_eq!(
            ocr_page_selection_mode(&request),
            PdfiumOcrMode::SkipTextPages
        );
        for value in [None, Some("force-ocr"), Some("Normal")] {
            request.ocr_type = value.map(str::to_owned);
            assert_eq!(
                ocr_page_selection_mode(&request),
                PdfiumOcrMode::AllPages,
                "ocrType {value:?} must OCR every page"
            );
        }
    }

    #[test]
    fn builds_java_compatible_ocrmypdf_arguments() {
        let mut request = options();
        request.sidecar = true;
        request.deskew = true;
        request.clean = true;
        request.clean_final = true;
        request.ocr_type = Some("force-ocr".to_owned());
        let arguments = ocrmypdf_arguments(
            Path::new("input.pdf"),
            Path::new("output.pdf"),
            Path::new("sidecar.txt"),
            &request,
            &["deu".to_owned(), "eng".to_owned(), "deu".to_owned()],
            "hocr",
        );
        let arguments = arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            [
                "--verbose",
                "2",
                "--output-type",
                "pdf",
                "--pdf-renderer",
                "hocr",
                "--sidecar",
                "sidecar.txt",
                "--deskew",
                "--clean",
                "--clean-final",
                "--force-ocr",
                "--invalidate-digital-signatures",
                "--language",
                "deu+eng+deu",
                "input.pdf",
                "output.pdf",
            ]
        );
    }

    #[test]
    fn retries_only_the_known_ocrmypdf_multiprocessing_failure() {
        assert!(is_multiprocessing_unavailable(
            b"multiprocessing/synchronize.py",
            b"OSError: [Errno 38] Function not implemented",
        ));
        assert!(!is_multiprocessing_unavailable(
            b"multiprocessing/synchronize.py",
            b"some other error",
        ));
    }

    #[cfg(unix)]
    #[test]
    fn ocrmypdf_timeout_is_reported_after_process_cleanup() {
        let executor = ProcessExecutor::new(1, Duration::from_millis(100));
        let arguments = ["-c".into(), "sleep 30".into()];
        assert!(matches!(
            run_ocrmypdf(&arguments, &["sh".to_owned()], &executor),
            Err(OcrError::OcrMyPdfTimeout {
                timeout_minutes: 1,
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn tesseract_fallback_skips_text_and_reassembles_pages_in_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let tessdata = directory.path().join("tessdata");
        fs::create_dir(&tessdata)?;
        fs::write(tessdata.join("eng.traineddata"), "test")?;
        let input = directory.path().join("input.pdf");
        write_pdf(&input, &[Some("retained text"), None])?;
        let replacement = directory.path().join("replacement.pdf");
        write_pdf(&replacement, &[None])?;
        let output = directory.path().join("output.pdf");
        let arguments_file = directory.path().join("arguments.txt");
        let runner = directory.path().join("fake-tesseract.sh");
        fs::write(
            &runner,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncp '{}' \"$2.pdf\"\n",
                arguments_file.display(),
                replacement.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&runner)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&runner, permissions)?;
        let mut runtime = runtime(&tessdata);
        runtime.tesseract_enabled = true;
        runtime.tesseract_commands = Some(vec![runner.to_string_lossy().into_owned()]);
        let mut request = options();
        request.ocr_type = Some("skip-text".to_owned());

        match run_ocr(&input, &output, &request, &runtime) {
            Ok(output_kind) => assert_eq!(output_kind, super::OcrOutput::Pdf),
            Err(OcrError::PdfiumUnavailable { .. }) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        assert_eq!(Document::load(&output)?.get_pages().len(), 2);
        let arguments = fs::read_to_string(arguments_file)?;
        let arguments = arguments.lines().collect::<Vec<_>>();
        assert_eq!(arguments.get(2..), Some(&["-l", "eng", "pdf"][..]));
        assert!(
            arguments
                .first()
                .is_some_and(|path| path.ends_with("page_1.png"))
        );
        assert!(
            arguments
                .get(1)
                .is_some_and(|path| path.ends_with("page_1"))
        );
        Ok(())
    }

    #[cfg(unix)]
    fn write_pdf(
        path: &Path,
        page_text: &[Option<&str>],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let mut page_ids = Vec::new();
        for text in page_text {
            let contents = text.map_or_else(Vec::new, |text| {
                format!("BT /F1 12 Tf 20 50 Td ({text}) Tj ET").into_bytes()
            });
            let contents_id = document.add_object(Stream::new(dictionary! {}, contents));
            page_ids.push(document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => Object::Reference(font_id) },
                },
                "Contents" => contents_id,
            }));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => i64::try_from(page_text.len())?,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.save(path)?;
        Ok(())
    }
}
