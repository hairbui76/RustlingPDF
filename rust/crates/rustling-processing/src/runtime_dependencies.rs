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
    /// Well-known Windows installation directories, probed **only after** the
    /// `PATH` search above has come up empty, and never when the
    /// `RUSTLING_PROCESSING_*_COMMAND` override is set.
    ///
    /// Several Windows installers deliberately do not modify `PATH`, so a tool
    /// that is correctly installed is still invisible to a name-only lookup —
    /// which is why "Convert to PDF" reported the `LibreOffice` group missing on
    /// Windows desktop installs. Entries are directory *templates*: `%NAME%` is
    /// expanded from the process environment (so the 64-bit `%ProgramFiles%`,
    /// the separate 32-bit `%ProgramFiles(x86)%`, and per-user
    /// `%LOCALAPPDATA%` roots are all read from the host, never hardcoded), and
    /// an entry naming a variable the host does not set is skipped.
    ///
    /// Empty for tools distributed on Windows only as pip wheels (`OCRmyPDF`,
    /// `WeasyPrint`), plain zip archives (`pdftohtml`/Poppler, qpdf, `FFmpeg`),
    /// or an installer whose target directory is user-chosen with no stable
    /// default (`veraPDF`): those have no default directory that can be cited, and
    /// guessing one would be worse than the `PATH` lookup they already rely on.
    /// The desktop bundle's own staged qpdf/Tesseract keep arriving through the
    /// sidecar's `RUSTLING_PROCESSING_*_COMMAND` overrides, which still win
    /// outright.
    windows_directories: &'static [&'static str],
    minimum_version: Option<[u64; 3]>,
    capability: Option<DependencyCapability>,
}

/// `PATHEXT` fallback used when the host does not define it, matching the value
/// Windows itself defaults to for the extensions we care about.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

const DEPENDENCY_SPECS: &[DependencySpec] = &[
    DependencySpec {
        group: "OCRmyPDF",
        environment: "RUSTLING_PROCESSING_OCRMYPDF_COMMAND",
        unix_candidates: &["ocrmypdf"],
        windows_candidates: &["ocrmypdf.exe", "ocrmypdf"],
        // Windows OCRmyPDF is a pip/pipx install; `ocrmypdf.exe` lands in the
        // interpreter's version-specific `Scripts` directory, which has no
        // citable fixed default. `PATH` remains the only sound lookup.
        windows_directories: &[],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "tesseract",
        environment: "RUSTLING_PROCESSING_TESSERACT_COMMAND",
        unix_candidates: &["tesseract"],
        windows_candidates: &["tesseract.exe", "tesseract"],
        // UB-Mannheim's installer is the de-facto Windows Tesseract build. Its
        // default install directory is `<ProgramFiles>\Tesseract-OCR`
        // (`<ProgramFiles(x86)>\Tesseract-OCR` for the 32-bit build), and its
        // "add to PATH" step is an optional, per-machine checkbox. A
        // current-user install instead targets
        // `%LOCALAPPDATA%\Programs\Tesseract-OCR`.
        windows_directories: &[
            r"%ProgramFiles%\Tesseract-OCR",
            r"%ProgramFiles(x86)%\Tesseract-OCR",
            r"%LOCALAPPDATA%\Programs\Tesseract-OCR",
        ],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "LibreOffice",
        environment: "RUSTLING_PROCESSING_SOFFICE_COMMAND",
        unix_candidates: &["soffice", "/usr/bin/soffice"],
        windows_candidates: &["soffice.com", "soffice.exe", "soffice"],
        // The LibreOffice MSI installs the launchers as
        // `<ProgramFiles>\LibreOffice\program\soffice.com` / `soffice.exe` and
        // never adds that directory to `PATH` — the reported root cause of
        // "Convert to PDF is not available from your server" on Windows.
        // LibreOffice publishes both a 64-bit and a 32-bit build; the 32-bit one
        // installs under `<ProgramFiles(x86)>` on a 64-bit host.
        windows_directories: &[
            r"%ProgramFiles%\LibreOffice\program",
            r"%ProgramFiles(x86)%\LibreOffice\program",
        ],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "Weasyprint",
        environment: "RUSTLING_PROCESSING_WEASYPRINT_COMMAND",
        unix_candidates: &["weasyprint", "/usr/bin/weasyprint"],
        windows_candidates: &["weasyprint.exe", "weasyprint"],
        // Same as OCRmyPDF: a pip install into a version-specific `Scripts`
        // directory, with no fixed installer default to cite.
        windows_directories: &[],
        minimum_version: Some([58, 0, 0]),
        capability: None,
    },
    DependencySpec {
        group: "Pdftohtml",
        environment: "RUSTLING_PROCESSING_PDFTOHTML_COMMAND",
        unix_candidates: &["pdftohtml"],
        windows_candidates: &["pdftohtml.exe", "pdftohtml"],
        // Poppler for Windows ships as a zip the operator unpacks wherever they
        // like; there is no installer and therefore no default directory.
        windows_directories: &[],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "qpdf",
        environment: "RUSTLING_PROCESSING_QPDF_COMMAND",
        unix_candidates: &["qpdf"],
        windows_candidates: &["qpdf.exe", "qpdf"],
        // The upstream Windows installation guide documents qpdf's install
        // directory as `C:\Program Files\qpdf\bin` and instructs the operator to
        // add it to `PATH` by hand, so the installer does not. qpdf also ships
        // plain zips that get unpacked anywhere and version-suffixed install
        // directories, both of which this cannot cover — the probe is a bonus,
        // not a guarantee. The desktop bundle stages its own copy and points the
        // service at it with `RUSTLING_PROCESSING_QPDF_COMMAND`, which takes
        // precedence over all of this.
        windows_directories: &[r"%ProgramFiles%\qpdf\bin", r"%ProgramFiles(x86)%\qpdf\bin"],
        minimum_version: Some([12, 0, 0]),
        capability: None,
    },
    DependencySpec {
        group: "rar",
        environment: "RUSTLING_PROCESSING_RAR_COMMAND",
        unix_candidates: &["rar"],
        windows_candidates: &["rar.exe", "rar"],
        // RAR creation on Windows means WinRAR, whose installer defaults to
        // `<ProgramFiles>\WinRAR` (`<ProgramFiles(x86)>\WinRAR` for the 32-bit
        // build) and ships the console `Rar.exe` next to `WinRAR.exe`. It does
        // not put that directory on `PATH`.
        windows_directories: &[r"%ProgramFiles%\WinRAR", r"%ProgramFiles(x86)%\WinRAR"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "Calibre",
        environment: "RUSTLING_PROCESSING_EBOOK_CONVERT_COMMAND",
        unix_candidates: &["ebook-convert"],
        windows_candidates: &["ebook-convert.exe", "ebook-convert"],
        // Calibre's Windows installer defaults to `<ProgramFiles>\Calibre2`
        // (the directory name still carries the "2" from the 2.x series) and
        // installs the CLI tools, including `ebook-convert.exe`, directly in it.
        // Adding it to `PATH` is an installer option, so it cannot be relied on;
        // the 32-bit build installs under `<ProgramFiles(x86)>`.
        windows_directories: &[r"%ProgramFiles%\Calibre2", r"%ProgramFiles(x86)%\Calibre2"],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "FFmpeg",
        environment: "RUSTLING_PROCESSING_FFMPEG_COMMAND",
        unix_candidates: &["ffmpeg", "/usr/bin/ffmpeg"],
        windows_candidates: &["ffmpeg.exe", "ffmpeg"],
        // FFmpeg's Windows builds are zip archives unpacked wherever the
        // operator chooses; no installer, no default directory.
        windows_directories: &[],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "veraPDF",
        environment: "RUSTLING_PROCESSING_VERAPDF_COMMAND",
        unix_candidates: &["verapdf", "verapdf.bat"],
        windows_candidates: &["verapdf.bat", "verapdf.exe", "verapdf"],
        // veraPDF's installer asks for a target directory rather than defaulting
        // to a stable well-known one, so there is nothing citable to probe.
        windows_directories: &[],
        minimum_version: None,
        capability: None,
    },
    DependencySpec {
        group: "unrar",
        environment: "RUSTLING_PROCESSING_UNRAR_COMMAND",
        unix_candidates: &["unrar", "7z", "7zz"],
        windows_candidates: &["unrar.exe", "unrar", "7z.exe", "7z"],
        // Both RAR-extraction providers install to a fixed directory and leave
        // `PATH` alone: WinRAR ships `UnRAR.exe` in `<ProgramFiles>\WinRAR`, and
        // 7-Zip's installer defaults to `<ProgramFiles>\7-Zip` with `7z.exe` in
        // it. `<ProgramFiles(x86)>` covers the 32-bit build of each. WinRAR is
        // probed first because genuine `unrar` needs no capability probe,
        // whereas a 7-Zip hit still has to prove it carries a RAR codec.
        windows_directories: &[
            r"%ProgramFiles%\WinRAR",
            r"%ProgramFiles(x86)%\WinRAR",
            r"%ProgramFiles%\7-Zip",
            r"%ProgramFiles(x86)%\7-Zip",
        ],
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
    resolve_dependency_with(
        spec,
        crate::environment::var_os(spec.environment),
        resolve_command,
        platform_installation_directory_hits,
        |command| dependency_capability_available(command, spec.capability),
    )
}

/// The resolution order, independent of the host it runs on.
///
/// 1. An explicit `RUSTLING_PROCESSING_*_COMMAND` override resolves on its own
///    and never falls through to anything else — an operator (or the desktop
///    sidecar staging its own bundled tool) who names a command must get that
///    command or nothing, so a stale override can never be masked by an
///    unrelated installation elsewhere on the machine.
/// 2. Otherwise the platform command names are looked up on `PATH`, exactly as
///    before.
/// 3. Only if `PATH` yields nothing are the well-known Windows installation
///    directories probed. On non-Windows hosts step 3 produces nothing, so Unix
///    resolution is unchanged.
///
/// The capability filter (RAR decompression) is applied identically to `PATH`
/// hits and directory hits, so a directory-discovered 7-Zip still has to prove
/// it carries a RAR codec.
fn resolve_dependency_with(
    spec: &DependencySpec,
    configured: Option<OsString>,
    resolve_path: impl Fn(&OsStr) -> Option<PathBuf>,
    installation_directory_hits: impl Fn(&DependencySpec) -> Vec<PathBuf>,
    capable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let configured = configured.filter(|command| !command.is_empty());
    let explicitly_configured = configured.is_some();
    let candidates = configured_or_platform_candidates_with(
        configured,
        spec.unix_candidates,
        spec.windows_candidates,
    );
    let resolved = candidates
        .iter()
        .filter_map(|candidate| resolve_path(candidate))
        .find(|command| capable(command));
    if resolved.is_some() || explicitly_configured {
        return resolved;
    }
    installation_directory_hits(spec)
        .into_iter()
        .find(|command| capable(command))
}

fn configured_or_platform_candidates(
    environment: &str,
    unix_candidates: &[&str],
    windows_candidates: &[&str],
) -> Vec<OsString> {
    configured_or_platform_candidates_with(
        crate::environment::var_os(environment),
        unix_candidates,
        windows_candidates,
    )
}

/// Thin platform-selection layer: the well-known installation directories are a
/// Windows-only concept, so on every other host this is an unconditional no-op
/// that touches neither the environment nor the filesystem.
fn platform_installation_directory_hits(spec: &DependencySpec) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    installation_directory_hits(
        spec.windows_directories,
        spec.windows_candidates,
        &|name: &str| crate::environment::var_os(name),
        &|path: &Path| path.is_file(),
    )
}

/// Every executable in `directories` matching one of `candidates`, in priority
/// order: directories outermost (an installation is probed in full before the
/// next one), then the candidate names in their declared preference order.
///
/// Pure by construction — the environment and the filesystem both arrive as
/// parameters — so the Windows layout is exercised by unit tests on any host.
/// Existence is a single `is_file` stat per candidate path; no process is ever
/// spawned here.
fn installation_directory_hits(
    directories: &[&str],
    candidates: &[&str],
    lookup_environment: &impl Fn(&str) -> Option<OsString>,
    is_file: &impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    for template in directories {
        let Some(directory) = expand_environment_placeholders(template, lookup_environment) else {
            continue;
        };
        let directory = Path::new(&directory);
        for candidate in candidates {
            for extension in
                windows_candidate_extensions(OsStr::new(*candidate), lookup_environment)
            {
                let mut filename = OsString::from(*candidate);
                filename.push(&extension);
                let path = directory.join(filename);
                if is_file(&path) {
                    hits.push(path);
                }
            }
        }
    }
    hits
}

/// Expands `%NAME%` placeholders in a Windows directory template.
///
/// Returns `None` when a referenced variable is unset or empty, so a template
/// rooted at a directory the host does not have — `%ProgramFiles(x86)%` is
/// absent on a 32-bit-only or ARM64 image, `%LOCALAPPDATA%` under a service
/// account — is skipped rather than probed as a relative or truncated path. An
/// unterminated `%` is kept verbatim.
fn expand_environment_placeholders(
    template: &str,
    lookup_environment: &impl Fn(&str) -> Option<OsString>,
) -> Option<OsString> {
    let mut expanded = OsString::new();
    let mut rest = template;
    while let Some(start) = rest.find('%') {
        let Some(length) = rest[start + 1..].find('%') else {
            break;
        };
        let name = &rest[start + 1..start + 1 + length];
        let value = lookup_environment(name).filter(|value| !value.is_empty())?;
        expanded.push(&rest[..start]);
        expanded.push(&value);
        rest = &rest[start + 1 + length + 1..];
    }
    expanded.push(rest);
    Some(expanded)
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
    let search_path = crate::environment::var_os("PATH")?;
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
    if !cfg!(windows) {
        return vec![OsString::new()];
    }
    windows_candidate_extensions(command, &|name: &str| crate::environment::var_os(name))
}

/// The suffixes to append to a Windows candidate name: just the empty suffix
/// when the candidate already carries an extension, otherwise the empty suffix
/// followed by every `PATHEXT` entry.
fn windows_candidate_extensions(
    candidate: &OsStr,
    lookup_environment: &impl Fn(&str) -> Option<OsString>,
) -> Vec<OsString> {
    if Path::new(candidate).extension().is_some() {
        return vec![OsString::new()];
    }
    let mut extensions = vec![OsString::new()];
    extensions.extend(
        lookup_environment("PATHEXT")
            .unwrap_or_else(|| OsString::from(DEFAULT_PATHEXT))
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

    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    };

    use super::{
        DEPENDENCY_SPECS, DependencySpec, configured_or_platform_candidates_with,
        dependency_group_names, expand_environment_placeholders, installation_directory_hits,
        is_seven_zip_capability_listing, parse_version, resolve_command, resolve_dependency_with,
        seven_zip_reports_rar,
    };

    /// A 64-bit Windows host with a per-user profile, as `std::env` would report
    /// it. Nothing in the module hardcodes a drive letter — every root below
    /// comes from these variables.
    fn windows_environment() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([
            ("ProgramFiles", r"C:\Program Files"),
            ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
            ("LOCALAPPDATA", r"C:\Users\ada\AppData\Local"),
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
        ])
    }

    fn lookup<'a>(
        environment: &'a BTreeMap<&'static str, &'static str>,
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name: &str| environment.get(name).map(OsString::from)
    }

    /// Builds the same path the module builds, so expectations cannot drift from
    /// the platform's path separator when these tests run on Linux.
    fn installed(directory: &str, file: &str) -> PathBuf {
        Path::new(directory).join(file)
    }

    fn exists(files: &[PathBuf]) -> impl Fn(&Path) -> bool + '_ {
        move |path: &Path| files.iter().any(|file| file == path)
    }

    fn spec_for(group: &str) -> &'static DependencySpec {
        match DEPENDENCY_SPECS.iter().find(|spec| spec.group == group) {
            Some(spec) => spec,
            None => panic!("no dependency spec named {group}"),
        }
    }

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

    // ---------------------------------------------------------------------
    // Windows installation-directory fallback.
    //
    // The resolver runs against the real host, so these tests drive the pure
    // layers directly: `installation_directory_hits` takes the environment and
    // an existence predicate as parameters, and `resolve_dependency_with` takes
    // the PATH lookup and the directory probe as parameters. Both therefore
    // exercise the Windows layout on a Linux CI machine.
    // ---------------------------------------------------------------------

    #[test]
    fn windows_directory_probe_finds_the_default_libreoffice_installation() {
        // The reported bug: LibreOffice installed at its MSI default, which the
        // installer does not add to PATH.
        let environment = windows_environment();
        let soffice = installed(r"C:\Program Files\LibreOffice\program", "soffice.com");
        let files = [
            soffice.clone(),
            installed(r"C:\Program Files\LibreOffice\program", "soffice.exe"),
        ];
        let spec = spec_for("LibreOffice");
        let hits = installation_directory_hits(
            spec.windows_directories,
            spec.windows_candidates,
            &lookup(&environment),
            &exists(&files),
        );
        // `soffice.com` is the declared first preference and must lead.
        assert_eq!(hits.first(), Some(&soffice));
    }

    #[test]
    fn windows_directory_probe_falls_back_to_the_thirty_two_bit_program_files() {
        // LibreOffice ships a 32-bit build too; on a 64-bit host it installs
        // under `Program Files (x86)`, a directory distinct from `%ProgramFiles%`.
        let environment = windows_environment();
        let soffice = installed(r"C:\Program Files (x86)\LibreOffice\program", "soffice.com");
        let files = [soffice.clone()];
        let spec = spec_for("LibreOffice");
        let hits = installation_directory_hits(
            spec.windows_directories,
            spec.windows_candidates,
            &lookup(&environment),
            &exists(&files),
        );
        assert_eq!(hits, vec![soffice]);
    }

    #[test]
    fn windows_directory_probe_finds_a_per_user_tesseract_installation() {
        // UB-Mannheim's "install for me only" mode targets
        // %LOCALAPPDATA%\Programs, never a Program Files root.
        let environment = windows_environment();
        let tesseract = installed(
            r"C:\Users\ada\AppData\Local\Programs\Tesseract-OCR",
            "tesseract.exe",
        );
        let files = [tesseract.clone()];
        let spec = spec_for("tesseract");
        let hits = installation_directory_hits(
            spec.windows_directories,
            spec.windows_candidates,
            &lookup(&environment),
            &exists(&files),
        );
        assert_eq!(hits, vec![tesseract]);
    }

    #[test]
    fn windows_directory_probe_finds_nothing_when_nothing_is_installed() {
        let environment = windows_environment();
        for spec in DEPENDENCY_SPECS {
            let hits = installation_directory_hits(
                spec.windows_directories,
                spec.windows_candidates,
                &lookup(&environment),
                &|_: &Path| false,
            );
            assert!(hits.is_empty(), "{} probed a nonexistent tool", spec.group);
        }
    }

    #[test]
    fn windows_directory_probe_skips_templates_whose_variables_are_unset() {
        // A 32-bit-only or ARM64 image has no %ProgramFiles(x86)%; the entry must
        // be skipped, never probed as a truncated or relative path.
        let environment = BTreeMap::from([("ProgramFiles", r"C:\Program Files")]);
        let spec = spec_for("LibreOffice");
        let probed = std::cell::RefCell::new(Vec::new());
        let hits = installation_directory_hits(
            spec.windows_directories,
            spec.windows_candidates,
            &lookup(&environment),
            &|path: &Path| {
                probed.borrow_mut().push(path.to_owned());
                false
            },
        );
        assert!(hits.is_empty());
        // A textual prefix check, not `Path::starts_with`: these are Windows
        // paths, whose separators are not path separators on the host running
        // the test.
        assert!(
            probed
                .borrow()
                .iter()
                .all(|path| path.to_string_lossy().starts_with(r"C:\Program Files\")),
            "probed outside the one root the host defines: {:?}",
            probed.borrow()
        );
        assert!(!probed.borrow().is_empty(), "the defined root was skipped");
    }

    #[test]
    fn windows_directory_probe_expands_extensionless_candidates_through_pathext() {
        // `unrar` (no extension) must be tried with each PATHEXT suffix, exactly
        // as the PATH search does, so a `.bat`/`.cmd` shim is still found.
        let environment = windows_environment();
        let unrar = installed(r"C:\Program Files\WinRAR", "unrar.CMD");
        let files = [unrar.clone()];
        let spec = spec_for("unrar");
        let hits = installation_directory_hits(
            spec.windows_directories,
            spec.windows_candidates,
            &lookup(&environment),
            &exists(&files),
        );
        assert_eq!(hits, vec![unrar]);
    }

    #[test]
    fn environment_placeholder_expansion_handles_parenthesised_and_missing_names() {
        let environment = windows_environment();
        assert_eq!(
            expand_environment_placeholders(r"%ProgramFiles(x86)%\7-Zip", &lookup(&environment)),
            Some(OsString::from(r"C:\Program Files (x86)\7-Zip"))
        );
        assert_eq!(
            expand_environment_placeholders(r"%NOT_SET%\7-Zip", &lookup(&environment)),
            None
        );
        // An empty value is as useless as an unset one.
        let empty = BTreeMap::from([("ProgramFiles", "")]);
        assert_eq!(
            expand_environment_placeholders(r"%ProgramFiles%\7-Zip", &lookup(&empty)),
            None
        );
        // No placeholder, and an unterminated `%`, both pass through verbatim.
        assert_eq!(
            expand_environment_placeholders(r"D:\tools", &lookup(&environment)),
            Some(OsString::from(r"D:\tools"))
        );
        assert_eq!(
            expand_environment_placeholders("100%", &lookup(&environment)),
            Some(OsString::from("100%"))
        );
    }

    #[test]
    fn explicit_command_override_wins_and_never_falls_through_to_directory_probing() {
        // The documented contract: an operator-named command (or the desktop
        // sidecar's staged tool) resolves on its own. A *broken* override must
        // stay broken rather than silently resolving an unrelated installation.
        let spec = spec_for("LibreOffice");
        let staged = PathBuf::from(r"C:\Program Files\RustlingPDF\resources\tools\soffice.exe");
        let resolved = resolve_dependency_with(
            spec,
            Some(staged.as_os_str().to_owned()),
            |candidate| Some(PathBuf::from(candidate)),
            |_| panic!("directory probing must not run for an explicit override"),
            |_| true,
        );
        assert_eq!(resolved, Some(staged));

        let missing = resolve_dependency_with(
            spec,
            Some(OsString::from(r"C:\gone\soffice.exe")),
            |_| None,
            |_| panic!("directory probing must not run for an explicit override"),
            |_| true,
        );
        assert_eq!(missing, None);
    }

    /// The candidate names `resolve_dependency_with` will actually try on the
    /// host running the test — the platform selection inside
    /// `configured_or_platform_candidates_with` is deliberately left intact.
    fn platform_candidates(spec: &DependencySpec) -> &'static [&'static str] {
        if cfg!(windows) {
            spec.windows_candidates
        } else {
            spec.unix_candidates
        }
    }

    #[test]
    fn path_resolution_wins_over_the_directory_fallback() {
        let spec = spec_for("LibreOffice");
        let preferred = OsString::from(platform_candidates(spec)[0]);
        let on_path = PathBuf::from("/opt/libreoffice/program/soffice");
        let resolved = resolve_dependency_with(
            spec,
            None,
            |candidate| (candidate == preferred).then(|| on_path.clone()),
            |_| panic!("directory probing must not run once PATH resolved a command"),
            |_| true,
        );
        assert_eq!(resolved, Some(on_path));
    }

    #[test]
    fn the_directory_fallback_runs_only_after_path_comes_up_empty() {
        let spec = spec_for("LibreOffice");
        let installed_default = PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.com");
        let resolved = resolve_dependency_with(
            spec,
            None,
            |_| None,
            |_| vec![installed_default.clone()],
            |_| true,
        );
        assert_eq!(resolved, Some(installed_default.clone()));

        // Neither source has it: the group stays missing and is reported as
        // DEPENDENCY, as before.
        assert_eq!(
            resolve_dependency_with(spec, None, |_| None, |_| Vec::new(), |_| true),
            None
        );
        // An empty override is not an override; it falls back like `None`.
        assert_eq!(
            resolve_dependency_with(
                spec,
                Some(OsString::new()),
                |_| None,
                |_| vec![installed_default.clone()],
                |_| true
            ),
            Some(installed_default)
        );
    }

    #[test]
    fn the_capability_filter_applies_to_directory_hits_too() {
        // A directory-discovered 7-Zip without a RAR codec must be rejected just
        // like a PATH-discovered one, and the next hit tried.
        let spec = spec_for("unrar");
        let seven_zip = PathBuf::from(r"C:\Program Files\7-Zip\7z.exe");
        let unrar = PathBuf::from(r"C:\Program Files\WinRAR\UnRAR.exe");
        let resolved = resolve_dependency_with(
            spec,
            None,
            |_| None,
            |_| vec![seven_zip.clone(), unrar.clone()],
            |command| command != seven_zip,
        );
        assert_eq!(resolved, Some(unrar));
    }

    #[test]
    fn windows_directories_are_environment_rooted_never_hardcoded_drive_letters() {
        for spec in DEPENDENCY_SPECS {
            for template in spec.windows_directories {
                assert!(
                    template.starts_with('%'),
                    "{}: {template} does not start from an environment variable",
                    spec.group
                );
                assert!(
                    !template.contains(":\\"),
                    "{}: {template} hardcodes a drive letter",
                    spec.group
                );
            }
        }
    }

    #[test]
    fn unix_resolution_is_untouched_by_the_windows_fallback() {
        // The platform layer is the only thing that decides whether directories
        // are probed at all; on a non-Windows host it yields nothing, so every
        // spec resolves purely from PATH exactly as it did before this change.
        if cfg!(windows) {
            return;
        }
        for spec in DEPENDENCY_SPECS {
            assert!(
                super::platform_installation_directory_hits(spec).is_empty(),
                "{} probed installation directories on a non-Windows host",
                spec.group
            );
        }
    }
}
