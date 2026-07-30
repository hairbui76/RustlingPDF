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
        Some(DependencyCapability::RarExtraction) => {
            seven_zip_rar_capability(command).unwrap_or(true)
        }
        None => true,
    }
}

/// Probes `command i` for genuine RAR *decompression* capability, independent of the
/// candidate's file name. The probe used to run only when the file stem was exactly
/// `7z`/`7zz`; an operator pointing `RUSTLING_PROCESSING_UNRAR_COMMAND` at a 7-Zip binary
/// under any other name (a renamed copy, a wrapper script, `/opt/vendor/archiver`, ...)
/// skipped the check entirely and was assumed capable unconditionally — the same
/// truthfulness gap this module exists to close, just reached through the file name
/// instead of the RAR-handler/codec confusion.
///
/// Returns `None` when `command i` does not produce a `7z i`-style capability listing at
/// all (no `Formats:` section) — the shape genuine `unrar` produces, since it has no `i`
/// subcommand. The caller treats `None` as "assume capable", preserving the long-standing
/// behaviour for real `unrar`, which always supports RAR.
fn seven_zip_rar_capability(command: &Path) -> Option<bool> {
    let output = run_with_timeout(command, &["i"])?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    is_seven_zip_capability_listing(&stdout).then(|| seven_zip_reports_rar(&stdout))
}

fn is_seven_zip_capability_listing(output: &str) -> bool {
    output.lines().any(|line| line.trim() == "Formats:")
}

/// Whether `7z i` output proves genuine RAR *decompression* capability.
///
/// 7-Zip's RAR format handler shows up under `Formats:` as `Rar`/`Rar5`. That only lets the
/// tool recognise and open a `.rar` container — list its entries, and extract any that
/// happen to be stored uncompressed. Actually decompressing a RAR-compressed entry requires
/// one of the RAR *codecs* (`Rar`/`Rar1`/`Rar2`/`Rar3`/`Rar5`), which appear under the
/// separate `Codecs:` section.
///
/// This distinction is exactly what makes Debian's DFSG-compliant `7zip` package a false
/// positive under a `Formats:`-only check: its `debian/copyright` lists
/// `Files-Excluded: CPP/7zip/Compress/Rar*` (the codec sources) as encumbered by the
/// non-free unRAR licence, but does **not** exclude `CPP/7zip/Archive/Rar/` (the format
/// handler) — so the shipped `7z i` still lists `Rar`/`Rar5` under `Formats:` while having
/// zero RAR entries under `Codecs:`. A probe that only looks at `Formats:` therefore reports
/// this build as RAR-capable, which is false: it can open a `.rar` archive and list/extract
/// stored entries, but cannot decompress a single RAR-compressed entry because no RAR
/// decoder is registered. Installing the non-free `7zip-rar` plugin adds exactly the missing
/// `Codecs:` entries (`Rar1`/`Rar2`/`Rar3`/`Rar5`) without touching `Formats:` at all.
///
/// We therefore require BOTH a `Formats:` handler and a `Codecs:` entry: the codec is the
/// necessary condition (no decoder, no decompression, full stop), and the format handler is
/// required in addition because without it 7-Zip cannot recognise/open a `.rar` container at
/// all regardless of which codecs happen to be linked in — so a hypothetical build with a
/// stray RAR codec but no format handler would not be genuinely RAR-capable either.
fn seven_zip_reports_rar(output: &str) -> bool {
    section_has_rar_entry(formats_section(output)) && section_has_rar_entry(codecs_section(output))
}

fn section_has_rar_entry<'a>(mut lines: impl Iterator<Item = &'a str>) -> bool {
    lines.any(|line| {
        line.split_ascii_whitespace()
            .any(|field| matches!(field, "Rar" | "Rar1" | "Rar2" | "Rar3" | "Rar5"))
    })
}

/// Yields the lines of the `Formats:` section of `7z i` output.
fn formats_section(output: &str) -> impl Iterator<Item = &str> {
    section(output, "Formats:")
}

/// Yields the lines of the `Codecs:` section of `7z i` output.
fn codecs_section(output: &str) -> impl Iterator<Item = &str> {
    section(output, "Codecs:")
}

/// Yields the lines of the named section (e.g. `Formats:`, `Codecs:`) of `7z i` output,
/// stopping at whichever comes first: a blank line, or the next section header (`Codecs:`,
/// `Hashers:`, `Libs:`, or any other bare `Word:` line). `7z i` normally separates sections
/// with a blank line, but stopping on a header too means the boundary is correct even when a
/// build happens not to blank-line-separate two sections — previously, a `Formats:` section
/// not followed by a blank line let the scan run straight into the next section (e.g.
/// `Codecs:`'s `Rar1` line), producing a false positive. This is a conservative fix: a blank
/// line *inside* a section, or a localised (non-English) header, both still terminate the
/// section early or late respectively, but neither the 7-Zip CLI nor its `i` output ever do
/// that in practice (7-Zip's own output is English-only).
fn section<'a>(output: &'a str, heading: &'static str) -> impl Iterator<Item = &'a str> {
    output
        .lines()
        .skip_while(move |line| line.trim() != heading)
        .skip(1)
        .take_while(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !is_section_header(trimmed)
        })
}

/// Whether a trimmed line is itself a section header — a single bare word followed by a
/// colon and nothing else (`Formats:`, `Codecs:`, `Hashers:`, `Libs:`, ...).
fn is_section_header(trimmed: &str) -> bool {
    trimmed.strip_suffix(':').is_some_and(|name| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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
        is_seven_zip_capability_listing, parse_version, resolve_command, seven_zip_reports_rar,
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

    // The fixtures below are trimmed excerpts of `7z i` captured from the actual Debian
    // packages `docker/Dockerfile` installs: `7zip_23.01+dfsg-11_amd64.deb` (base, DFSG,
    // no RAR codecs) and the same binary with the non-free `7zip-rar_23.01-4_amd64.deb`
    // plugin's `Codecs/Rar.so` installed alongside it. Both list `Rar`/`Rar5` under
    // `Formats:` in every case — the DFSG package does NOT omit the format handler, only
    // the decompression codecs. Irrelevant `Formats:`/`Codecs:` rows were dropped for
    // brevity; every remaining line is copied verbatim, including exact spacing.

    const REAL_DFSG_FORMATS_AND_CODECS: &str = "7-Zip 23.01 (x64) : Copyright (c) 1999-2023 Igor Pavlov : 2023-06-20\n\
         \n\
         Formats:\n\
         0 C...F..........c.a.m+.. w...0  7z       7z            7 z BC AF ' 1C\n\
         0  ......................  Compound msi msp doc xls ppt D0 CF 11 E0 A1 B1 1A E1\n\
         0  ...F..................  Rar      rar r00       R a r ! 1A 07 00\n\
         0  ...F..................  Rar5     rar r00       R a r ! 1A 07 01 00\n\
         0 C...FMG........c.a.m+.. wud.0  zip      zip z01 zipx jar xpi odt ods docx xlsx epub ipa apk appx P K 03 04\n\
         \n\
         Codecs:\n\
         0  EDF  6F00181 AES256CBC\n\
         0  ED     30401 PPMD\n\
         0  ED     30101 LZMA\n\
         0  ED         0 Copy\n\
         0  EDF  3030103 BCJ\n\
         0 4ED   303011B BCJ2\n\
         \n\
         Hashers:\n\
               4        1 CRC32\n";

    #[test]
    fn seven_zip_rejects_real_debian_dfsg_output_without_rar_codec() {
        // Real captured shape: Formats: has Rar/Rar5 (the handler is NOT excluded), but
        // Codecs: has no Rar/Rar1/Rar2/Rar3/Rar5 entry (the codecs ARE excluded per
        // debian/copyright's `Files-Excluded: CPP/7zip/Compress/Rar*`). This build can open
        // a .rar container and list/extract stored entries, but cannot decompress a single
        // RAR-compressed entry — it must be rejected.
        assert!(!seven_zip_reports_rar(REAL_DFSG_FORMATS_AND_CODECS));
    }

    #[test]
    fn seven_zip_accepts_real_rar_plugin_output() {
        // Same binary as above, plus the non-free 7zip-rar plugin's Codecs/Rar.so: Codecs:
        // gains Rar1/Rar2/Rar3/Rar5 while Formats: is untouched. Now genuinely RAR-capable.
        let output = REAL_DFSG_FORMATS_AND_CODECS.replace(
            "0 4ED   303011B BCJ2\n\
             \n\
             Hashers:",
            "0 4ED   303011B BCJ2\n\
             1   D     40301 Rar1\n\
             1   D     40302 Rar2\n\
             1   D     40303 Rar3\n\
             1   D     40305 Rar5\n\
             \n\
             Hashers:",
        );
        assert!(seven_zip_reports_rar(&output));
    }

    #[test]
    fn seven_zip_rejects_codecs_only_without_formats_handler() {
        // Synthetic: a Rar codec present under Codecs: is not sufficient on its own — without
        // a Formats: handler, 7-Zip cannot recognise/open a .rar container in the first place.
        let output = "7-Zip 23.01 (x64) : Copyright (c) 1999-2023 Igor Pavlov\n\
             \n\
             Formats:\n\
             0 C...F..........c.a.m+.. w...0  7z       7z            7 z BC AF ' 1C\n\
             0 C...FMG........c.a.m+.. wud.0  zip      zip z01 zipx jar xpi odt ods docx xlsx epub ipa apk appx P K 03 04\n\
             \n\
             Codecs:\n\
             0  ED         0 Copy\n\
             1   D     40301 Rar1\n\
             1   D     40303 Rar3\n\
             1   D     40305 Rar5\n\
             \n\
             Hashers:\n\
                   4        1 CRC32\n";
        assert!(!seven_zip_reports_rar(output));
    }

    #[test]
    fn seven_zip_rejects_codecs_only_without_formats_handler_and_no_blank_line_boundary() {
        // Same logical content as `seven_zip_rejects_codecs_only_without_formats_handler`,
        // but with the blank line between Formats: and Codecs: removed. Under the previous
        // `take_while(!blank)` implementation, formats_section would run straight past the
        // missing blank line into the Codecs: line and its Rar1 entry, wrongly treating Rar1
        // as if it were a Formats: handler — a false positive. Terminating on the next
        // section header too (this fixture's regression pin) keeps the answer correct
        // regardless of blank-line formatting.
        let output = "7-Zip 23.01 (x64) : Copyright (c) 1999-2023 Igor Pavlov\n\
             \n\
             Formats:\n\
             0 C...F..........c.a.m+.. w...0  7z       7z            7 z BC AF ' 1C\n\
             0 C...FMG........c.a.m+.. wud.0  zip      zip z01 zipx jar xpi odt ods docx xlsx epub ipa apk appx P K 03 04\n\
             Codecs:\n\
             0  ED         0 Copy\n\
             1   D     40301 Rar1\n\
             1   D     40303 Rar3\n\
             1   D     40305 Rar5\n\
             Hashers:\n\
                   4        1 CRC32\n";
        assert!(!seven_zip_reports_rar(output));
    }

    #[test]
    fn seven_zip_rejects_output_with_no_rar_anywhere() {
        // A minimal/older 7-Zip or p7zip build that lacks RAR support entirely: no Rar/Rar5
        // handler under Formats:, and no Rar codec under Codecs: either.
        let output = "p7zip Version 16.02 (locale=en_US.UTF-8,Utf16=on,HugeFiles=on,64 bits,4 CPUs)\n\
             \n\
             Formats:\n\
             0 C...F..........c.a.m+.. w...0  7z       7z            7 z BC AF ' 1C\n\
             0 C...FMG........c.a.m+.. wud.0  zip      zip z01 zipx jar xpi odt ods docx xlsx epub ipa apk appx P K 03 04\n\
             \n\
             Codecs:\n\
             0  ED         0 Copy\n\
             0 4ED   303011B BCJ2\n\
             \n\
             Hashers:\n\
                   4        1 CRC32\n";
        assert!(!seven_zip_reports_rar(output));
    }

    #[test]
    fn seven_zip_capability_listing_requires_a_formats_section() {
        assert!(is_seven_zip_capability_listing(
            REAL_DFSG_FORMATS_AND_CODECS
        ));
        // Genuine `unrar` has no `i` subcommand; its output (whatever it is) never contains
        // a bare `Formats:` line, so it must not be mistaken for a 7-Zip listing.
        assert!(!is_seven_zip_capability_listing(
            "RAR 6.24 beta 1   Copyright (c) 1993-2024 Alexander Roshal\nUsage: unrar <command>\n"
        ));
        assert!(!is_seven_zip_capability_listing(""));
    }
}
