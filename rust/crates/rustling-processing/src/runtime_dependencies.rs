//! Startup discovery for optional native command-line dependencies.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum DependencyCapability {
    RarExtraction,
}

struct DependencySpec {
    group: &'static str,
    environment: &'static str,
    unix_candidates: &'static [&'static str],
    windows_candidates: &'static [&'static str],
    minimum_version: Option<[u64; 3]>,
    capability: Option<DependencyCapability>,
}

const DEPENDENCY_SPECS: &[DependencySpec] = &[
    DependencySpec {
        group: "Ghostscript",
        environment: "RUSTLING_PROCESSING_GHOSTSCRIPT_COMMAND",
        unix_candidates: &["gs"],
        windows_candidates: &["gswin64c.exe", "gswin32c.exe", "gs.exe"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "OCRmyPDF",
        environment: "RUSTLING_PROCESSING_OCRMYPDF_COMMAND",
        unix_candidates: &["ocrmypdf"],
        windows_candidates: &["ocrmypdf.exe", "ocrmypdf"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "tesseract",
        environment: "RUSTLING_PROCESSING_TESSERACT_COMMAND",
        unix_candidates: &["tesseract"],
        windows_candidates: &["tesseract.exe", "tesseract"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "LibreOffice",
        environment: "RUSTLING_PROCESSING_SOFFICE_COMMAND",
        unix_candidates: &["soffice", "/usr/bin/soffice"],
        windows_candidates: &["soffice.com", "soffice.exe", "soffice"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "Weasyprint",
        environment: "RUSTLING_PROCESSING_WEASYPRINT_COMMAND",
        unix_candidates: &["weasyprint", "/usr/bin/weasyprint"],
        windows_candidates: &["weasyprint.exe", "weasyprint"],
        minimum_version: Some([58, 0, 0]),
        capability: None,
    },
    DependencySpec {
        group: "Pdftohtml",
        environment: "RUSTLING_PROCESSING_PDFTOHTML_COMMAND",
        unix_candidates: &["pdftohtml"],
        windows_candidates: &["pdftohtml.exe", "pdftohtml"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "qpdf",
        environment: "RUSTLING_PROCESSING_QPDF_COMMAND",
        unix_candidates: &["qpdf"],
        windows_candidates: &["qpdf.exe", "qpdf"],
        minimum_version: Some([12, 0, 0]),
        capability: None,
    },
    DependencySpec {
        group: "rar",
        environment: "RUSTLING_PROCESSING_RAR_COMMAND",
        unix_candidates: &["rar"],
        windows_candidates: &["rar.exe", "rar"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "Calibre",
        environment: "RUSTLING_PROCESSING_EBOOK_CONVERT_COMMAND",
        unix_candidates: &["ebook-convert"],
        windows_candidates: &["ebook-convert.exe", "ebook-convert"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "FFmpeg",
        environment: "RUSTLING_PROCESSING_FFMPEG_COMMAND",
        unix_candidates: &["ffmpeg", "/usr/bin/ffmpeg"],
        windows_candidates: &["ffmpeg.exe", "ffmpeg"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "veraPDF",
        environment: "RUSTLING_PROCESSING_VERAPDF_COMMAND",
        unix_candidates: &["verapdf", "verapdf.bat"],
        windows_candidates: &["verapdf.bat", "verapdf.exe", "verapdf"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "unrar",
        environment: "RUSTLING_PROCESSING_UNRAR_COMMAND",
        unix_candidates: &["unrar", "7z", "7zz"],
        windows_candidates: &["unrar.exe", "unrar", "7z.exe", "7z"],
        minimum_version: None,
        capability: Some(DependencyCapability::RarExtraction),
    },
];

#[derive(Debug, Default)]
pub(crate) struct DependencyDiscovery {
    pub(crate) disabled_groups: BTreeSet<String>,
    pub(crate) commands: BTreeMap<String, PathBuf>,
}

pub(crate) fn dependency_group_names() -> impl Iterator<Item = &'static str> {
    DEPENDENCY_SPECS.iter().map(|spec| spec.group)
}

/// Finds unavailable or too-old tool groups used by the Rust service.
pub(crate) fn discover_dependencies() -> DependencyDiscovery {
    let mut discovery = DependencyDiscovery::default();
    for spec in DEPENDENCY_SPECS {
        let Some(command) = resolve_dependency(spec) else {
            discovery.disabled_groups.insert(spec.group.to_owned());
            continue;
        };
        if let Some(required) = spec.minimum_version
            && probe_version(&command).is_some_and(|installed| installed < required)
        {
            discovery.disabled_groups.insert(spec.group.to_owned());
            continue;
        }
        discovery.commands.insert(spec.group.to_owned(), command);
    }
    discovery
}

fn resolve_dependency(spec: &DependencySpec) -> Option<PathBuf> {
    let candidates = configured_or_platform_candidates(
        spec.environment,
        spec.unix_candidates,
        spec.windows_candidates,
    );
    candidates
        .iter()
        .filter_map(|candidate| resolve_command(candidate))
        .find(|command| dependency_capability_available(command, spec.capability))
}

fn configured_or_platform_candidates(
    environment: &str,
    unix_candidates: &[&str],
    windows_candidates: &[&str],
) -> Vec<OsString> {
    configured_or_platform_candidates_with(
        crate::env_compat::var_os(environment),
        unix_candidates,
        windows_candidates,
    )
}

fn configured_or_platform_candidates_with(
    configured: Option<OsString>,
    unix_candidates: &[&str],
    windows_candidates: &[&str],
) -> Vec<OsString> {
    if let Some(command) = configured.filter(|command| !command.is_empty()) {
        return vec![command];
    }
    if cfg!(windows) {
        windows_candidates.iter().map(OsString::from).collect()
    } else {
        unix_candidates.iter().map(OsString::from).collect()
    }
}

fn dependency_capability_available(
    command: &Path,
    capability: Option<DependencyCapability>,
) -> bool {
    match capability {
        Some(DependencyCapability::RarExtraction) if is_seven_zip(command) => {
            run_with_timeout(command, &["i"]).is_some_and(|output| {
                output.status.success()
                    && seven_zip_reports_rar(&String::from_utf8_lossy(&output.stdout))
            })
        }
        None | Some(DependencyCapability::RarExtraction) => true,
    }
}

fn is_seven_zip(command: &Path) -> bool {
    command
        .file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("7z") || name.eq_ignore_ascii_case("7zz"))
}

fn seven_zip_reports_rar(output: &str) -> bool {
    output.lines().any(|line| {
        line.split_ascii_whitespace()
            .any(|field| field == "Rar" || field.starts_with("Rar"))
    })
}

fn resolve_command(command: &OsStr) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || path.components().count() > 1 {
        return path.is_file().then(|| path.to_owned());
    }
    let search_path = crate::env_compat::var_os("PATH")?;
    let extensions = executable_extensions(command);
    for directory in env::split_paths(&search_path) {
        for extension in &extensions {
            let mut filename = command.to_os_string();
            filename.push(extension);
            let candidate = directory.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_extensions(command: &OsStr) -> Vec<OsString> {
    if !cfg!(windows) || Path::new(command).extension().is_some() {
        return vec![OsString::new()];
    }
    let mut extensions = vec![OsString::new()];
    extensions.extend(
        crate::env_compat::var_os("PATHEXT")
            .unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"))
            .to_string_lossy()
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(OsString::from),
    );
    extensions
}

fn probe_version(command: &Path) -> Option<[u64; 3]> {
    let output = run_with_timeout(command, &["--version"])?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_version(&text)
}

fn run_with_timeout(command: &Path, arguments: &[&str]) -> Option<std::process::Output> {
    let mut child = Command::new(command)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn parse_version(value: &str) -> Option<[u64; 3]> {
    value
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .filter(|candidate| {
            candidate
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit())
        })
        .find_map(|candidate| {
            let mut parts = candidate.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
            let patch = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
            Some([major, minor, patch])
        })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, ffi::OsString, fs};

    use tempfile::tempdir;

    use super::{
        DEPENDENCY_SPECS, configured_or_platform_candidates_with, dependency_group_names,
        parse_version, resolve_command, seven_zip_reports_rar,
    };

    #[test]
    fn parses_numeric_tool_versions() {
        assert_eq!(parse_version("qpdf version 12.2.0"), Some([12, 2, 0]));
        assert_eq!(parse_version("WeasyPrint version 68.1"), Some([68, 1, 0]));
        assert_eq!(parse_version("tool 9"), Some([9, 0, 0]));
        assert_eq!(parse_version("unknown"), None);
    }

    #[test]
    fn dependency_specs_have_unique_group_and_environment_names() {
        let groups = dependency_group_names().collect::<BTreeSet<_>>();
        let environments = DEPENDENCY_SPECS
            .iter()
            .map(|spec| spec.environment)
            .collect::<BTreeSet<_>>();
        assert_eq!(groups.len(), DEPENDENCY_SPECS.len());
        assert_eq!(environments.len(), DEPENDENCY_SPECS.len());
        assert_eq!(
            groups,
            BTreeSet::from([
                "Calibre",
                "FFmpeg",
                "Ghostscript",
                "LibreOffice",
                "OCRmyPDF",
                "Pdftohtml",
                "Weasyprint",
                "qpdf",
                "rar",
                "tesseract",
                "unrar",
                "veraPDF",
            ])
        );
    }

    #[test]
    fn new_dependency_specs_use_the_runtime_command_overrides() {
        let overrides = [
            ("FFmpeg", "RUSTLING_PROCESSING_FFMPEG_COMMAND"),
            ("veraPDF", "RUSTLING_PROCESSING_VERAPDF_COMMAND"),
            ("unrar", "RUSTLING_PROCESSING_UNRAR_COMMAND"),
        ];
        for (group, environment) in overrides {
            assert_eq!(
                DEPENDENCY_SPECS
                    .iter()
                    .find(|spec| spec.group == group)
                    .map(|spec| spec.environment),
                Some(environment)
            );
        }
    }

    #[test]
    fn configured_commands_keep_existing_empty_and_file_resolution_semantics()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let non_executable = directory.path().join("tool");
        fs::write(&non_executable, b"not executable")?;
        let new_groups = ["FFmpeg", "veraPDF", "unrar"];
        let mut tested_groups = BTreeSet::new();
        for spec in DEPENDENCY_SPECS
            .iter()
            .filter(|spec| new_groups.contains(&spec.group))
        {
            tested_groups.insert(spec.group);
            let configured = configured_or_platform_candidates_with(
                Some(non_executable.as_os_str().to_owned()),
                spec.unix_candidates,
                spec.windows_candidates,
            );
            assert_eq!(configured, vec![non_executable.as_os_str().to_owned()]);
            assert_eq!(
                resolve_command(&configured[0]),
                Some(non_executable.clone())
            );

            let empty = configured_or_platform_candidates_with(
                Some(OsString::new()),
                spec.unix_candidates,
                spec.windows_candidates,
            );
            assert_eq!(
                empty,
                if cfg!(windows) {
                    spec.windows_candidates
                        .iter()
                        .map(OsString::from)
                        .collect::<Vec<_>>()
                } else {
                    spec.unix_candidates
                        .iter()
                        .map(OsString::from)
                        .collect::<Vec<_>>()
                }
            );

            let missing = configured_or_platform_candidates_with(
                Some(OsString::from("/definitely/missing/rustling-tool")),
                spec.unix_candidates,
                spec.windows_candidates,
            );
            assert_eq!(missing.len(), 1);
            assert_eq!(resolve_command(&missing[0]), None);
        }
        assert_eq!(tested_groups, BTreeSet::from(new_groups));
        Ok(())
    }

    #[test]
    fn seven_zip_requires_an_installed_rar_handler() {
        assert!(!seven_zip_reports_rar(
            "Formats:\n  C   F         7z       7z\n      F         zip      zip\n"
        ));
        assert!(seven_zip_reports_rar(
            "Formats:\n               Rar      rar r00\n               Rar5     rar r00\n"
        ));
    }
}
