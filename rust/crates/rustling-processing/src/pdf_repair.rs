//! Java-compatible PDF repair orchestration.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Output,
};

use thiserror::Error;

use crate::{
    pdf_document_ops::{DocumentOperationError, repair_pdf_to_file},
    process_executor::{ProcessExecutor, ProcessExecutorError},
    runtime_config::{RepairProcessSettings, RuntimeConfig},
};

#[derive(Clone, Debug)]
pub(crate) struct RepairRuntime {
    qpdf_command: Option<PathBuf>,
    qpdf: ProcessExecutor,
}

#[derive(Debug, Error)]
pub(crate) enum RepairError {
    #[error(transparent)]
    InProcess(#[from] DocumentOperationError),
    #[error("PDF repair failed with the available external tools: {details}")]
    ExternalTools { details: String },
}

impl RepairRuntime {
    pub(crate) fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self::new(
            config.dependency_command("qpdf"),
            config.repair_process_settings(),
        )
    }

    fn new(qpdf_command: Option<PathBuf>, settings: RepairProcessSettings) -> Self {
        Self {
            qpdf_command,
            qpdf: ProcessExecutor::new(settings.qpdf_session_limit, settings.qpdf_timeout),
        }
    }

    /// Runs qpdf, or the in-process structural rewrite when startup discovery
    /// found no external repair tool.
    pub(crate) fn repair(
        &self,
        input_path: &Path,
        filename: &str,
        output_path: &Path,
    ) -> Result<(), RepairError> {
        let Some(command) = &self.qpdf_command else {
            return repair_pdf_to_file(input_path, filename, output_path)
                .map_err(RepairError::from);
        };

        // Deliberate divergence from Java's `RepairController`, which passes
        // `--replace-input` *and* an output path. qpdf rejects that combination
        // outright — `--replace-input` rewrites the input file in place and forbids
        // a second positional argument, so every real qpdf (verified on 11.9.0 and
        // 12.3.2) exits 2 and never writes the output file. Java hides the failure
        // because it ignores the qpdf exit code and returns the never-created temp
        // file; this service checks the exit code. Dropping `--replace-input` keeps
        // the intended `--qdf --object-streams=disable` normalization and writes to
        // the output path, which is what the caller consumes.
        let arguments = [
            OsString::from("--qdf"),
            OsString::from("--object-streams=disable"),
            input_path.as_os_str().to_owned(),
            output_path.as_os_str().to_owned(),
        ];
        let failure = match self.qpdf.run(command, &arguments) {
            Ok(output) if matches!(output.status.code(), Some(0 | 3)) => return Ok(()),
            Ok(output) => process_failure("qpdf", &output),
            Err(error) => execution_failure("qpdf", &error),
        };
        remove_partial_output(output_path);

        Err(RepairError::ExternalTools { details: failure })
    }
}

fn remove_partial_output(output_path: &Path) {
    let _ = fs::remove_file(output_path);
}

fn process_failure(tool: &str, output: &Output) -> String {
    format!(
        "{tool} exited with {} ({})",
        output
            .status
            .code()
            .map_or_else(|| "no exit code".to_owned(), |code| code.to_string()),
        process_details(&output.stdout, &output.stderr)
    )
}

fn execution_failure(tool: &str, error: &ProcessExecutorError) -> String {
    format!("{tool} {error}")
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
    use std::{fs, time::Duration};

    use lopdf::{Document, Object, dictionary};
    use tempfile::tempdir;

    use super::{RepairError, RepairRuntime};
    use crate::runtime_config::RepairProcessSettings;

    fn settings() -> RepairProcessSettings {
        RepairProcessSettings {
            qpdf_session_limit: 2,
            qpdf_timeout: Duration::from_secs(5),
        }
    }

    #[cfg(unix)]
    fn executable(path: &std::path::Path, script: &str) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, script)?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)
    }

    /// Runs `repair`, retrying while Linux reports `ETXTBSY`.
    ///
    /// The fixture scripts are written and then executed straight away. Under the
    /// full parallel suite another thread can still hold a write descriptor to the
    /// just-created file when this thread forks, and the exec then fails with
    /// "Text file busy" — a property of the harness, not of the repair path.
    #[cfg(unix)]
    fn repair_retrying_on_busy_executable(
        runtime: &RepairRuntime,
        input: &std::path::Path,
        output: &std::path::Path,
    ) -> Result<(), RepairError> {
        for _ in 0..50 {
            match runtime.repair(input, "input.pdf", output) {
                Err(error) if error.to_string().contains("Text file busy") => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => return result,
            }
        }
        runtime.repair(input, "input.pdf", output)
    }

    #[cfg(unix)]
    #[test]
    fn uses_qpdf_with_java_arguments() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let qpdf = directory.path().join("qpdf");
        let arguments = directory.path().join("arguments");
        executable(
            &qpdf,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncp \"$3\" \"$4\"\nexit 3\n",
                arguments.display()
            ),
        )?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        fs::write(&input, b"qpdf repair fixture")?;

        repair_retrying_on_busy_executable(
            &RepairRuntime::new(Some(qpdf), settings()),
            &input,
            &output,
        )?;

        assert_eq!(fs::read(&output)?, b"qpdf repair fixture");
        assert_eq!(
            fs::read_to_string(arguments)?.lines().collect::<Vec<_>>(),
            vec![
                "--qdf",
                "--object-streams=disable",
                input.to_str().ok_or("non-UTF-8 input path")?,
                output.to_str().ok_or("non-UTF-8 output path")?,
            ]
        );
        Ok(())
    }

    /// Guards the fix for the `--replace-input` regression against a *real* qpdf
    /// rather than a shell stub: `--replace-input` forbids an output positional,
    /// so the previous argument list made every qpdf-only repair exit 2. Skips
    /// when no qpdf is discoverable, which keeps the gate green on hosts without
    /// the optional dependency.
    #[test]
    fn repairs_a_damaged_document_with_a_real_qpdf() -> Result<(), Box<dyn std::error::Error>> {
        let Some(qpdf) = discoverable_qpdf() else {
            return Ok(());
        };
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        fs::write(&input, damaged_pdf()?)?;

        // No Ghostscript: exactly the desktop-bundle configuration, where qpdf is
        // the only external repair tool that ships.
        RepairRuntime::new(None, Some(qpdf.clone()), settings()).repair(
            &input,
            "input.pdf",
            &output,
        )?;

        let repaired = fs::read(&output)?;
        assert!(
            repaired.starts_with(b"%PDF-"),
            "qpdf wrote a non-PDF output"
        );
        assert!(
            Document::load_mem(&repaired).is_ok(),
            "output does not parse"
        );
        // qpdf itself must now consider the repaired file structurally sound.
        let check = std::process::Command::new(&qpdf)
            .arg("--check")
            .arg(&output)
            .output()?;
        assert_eq!(
            check.status.code(),
            Some(0),
            "qpdf --check rejected the repaired output: {}",
            String::from_utf8_lossy(&check.stdout)
        );
        Ok(())
    }

    fn discoverable_qpdf() -> Option<std::path::PathBuf> {
        let configured = crate::env_compat::var_os("RUSTLING_PROCESSING_QPDF_COMMAND")
            .filter(|command| !command.is_empty())
            .map(std::path::PathBuf::from);
        let candidates = configured.map_or_else(
            || vec![std::path::PathBuf::from("qpdf")],
            |command| vec![command],
        );
        candidates.into_iter().find(|command| {
            std::process::Command::new(command)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
    }

    /// A structurally valid PDF whose `startxref` offset points nowhere, which is
    /// the corruption class qpdf reconstructs (and reports with exit code 3).
    fn damaged_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut healthy = Vec::new();
        single_page_document().save_to(&mut healthy)?;
        let text = String::from_utf8_lossy(&healthy).into_owned();
        let Some(offset) = text.rfind("startxref") else {
            return Err("serialized PDF has no startxref".into());
        };
        let mut damaged = healthy[..offset].to_vec();
        damaged.extend_from_slice(b"startxref\n999999999\n%%EOF\n");
        Ok(damaged)
    }

    fn single_page_document() -> Document {
        let mut document = Document::with_version("1.7");
        let pages_root_id = document.new_object_id();
        let page_object_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_root_id,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
        });
        document.objects.insert(
            pages_root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_object_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_root_id,
        });
        document.trailer.set("Root", catalog_id);
        document
    }

    #[test]
    fn rewrites_in_process_when_no_external_tool_was_discovered()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        let mut document = single_page_document();
        document.save(&input)?;

        RepairRuntime::new(None, settings()).repair(&input, "input.pdf", &output)?;

        assert_eq!(Document::load(&output)?.get_pages().len(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn fails_when_a_discovered_external_tool_cannot_repair()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let qpdf = directory.path().join("qpdf");
        executable(&qpdf, "#!/bin/sh\necho malformed >&2\nexit 1\n")?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        fs::write(&input, b"broken")?;

        let error = match repair_retrying_on_busy_executable(
            &RepairRuntime::new(Some(qpdf), settings()),
            &input,
            &output,
        ) {
            Ok(()) => return Err("qpdf failure must not fall through to lopdf".into()),
            Err(error) => error,
        };

        assert!(matches!(error, RepairError::ExternalTools { .. }));
        assert!(error.to_string().contains("qpdf exited with 1"));
        assert!(!output.exists());
        Ok(())
    }
}
