use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The only file name this binary is ever allowed to delete.
///
/// Must stay identical to `PROVISIONING_FILE_NAME` in
/// `frontend/editor/src-tauri/src/commands/connection.rs`.
const PROVISIONING_FILE_NAME: &str = "rustling-provisioning.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvisioningConfig<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    server_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lock_connection_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    login_agreement_enabled: Option<bool>,
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "y"
    )
}

/// Delete the provisioning file at `output`, and nothing else.
///
/// Called from the MSI's uninstall-time deferred CustomActions, which run
/// elevated (the all-users branch runs as LocalSystem), so this is deliberately
/// paranoid:
///
/// - the path's **file name** must be exactly `rustling-provisioning.json`, so a
///   mangled or hostile `--output` can never turn this into a general-purpose
///   elevated delete;
/// - only the file is removed. The containing directory
///   (`%APPDATA%\RustlingPDF` / `%PROGRAMDATA%\RustlingPDF`) holds the user's
///   own state — `settings.yml`, `custom_settings.yml`, `logs/` — and is never
///   touched, matching the Windows convention of leaving user data behind on
///   uninstall;
/// - a missing file is success, not an error: the MSI runs this on every
///   uninstall, including installs that were never provisioned.
///
/// On Windows a symlink planted at the target path is deleted as a link (the
/// reparse point itself), not followed, so an unprivileged user who pre-creates
/// `%PROGRAMDATA%\RustlingPDF` cannot redirect the elevated delete at another
/// file.
fn remove_provisioning_file(output: &Path) -> Result<(), String> {
    let file_name = output.file_name().and_then(|name| name.to_str());
    match file_name {
        Some(name) if name.eq_ignore_ascii_case(PROVISIONING_FILE_NAME) => {}
        _ => {
            return Err(format!(
                "--remove refuses to delete {}: expected a path ending in {}",
                output.display(),
                PROVISIONING_FILE_NAME
            ));
        }
    }

    match fs::remove_file(output) {
        Ok(()) => Ok(()),
        // Never provisioned (or already cleaned up) — the uninstall is still a
        // success.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to remove provisioning file {}: {}",
            output.display(),
            error
        )),
    }
}

fn main() -> Result<(), String> {
    let mut output: Option<PathBuf> = None;
    let mut url: Option<String> = None;
    let mut lock_value: Option<String> = None;
    let mut login_agreement_value: Option<String> = None;
    let mut remove = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--remove" => {
                remove = true;
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--output requires a value".to_string())?;
                output = Some(PathBuf::from(value));
            }
            "--url" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--url requires a value".to_string())?;
                url = Some(value);
            }
            "--lock" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--lock requires a value".to_string())?;
                lock_value = Some(value);
            }
            "--login-agreement" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--login-agreement requires a value".to_string())?;
                login_agreement_value = Some(value);
            }
            _ => {
                return Err(format!("Unknown argument: {}", arg));
            }
        }
    }

    let output = output.ok_or_else(|| "Missing --output".to_string())?;

    // Uninstall path: delete the provisioning file and stop. The MSI never
    // passes provisioning values together with --remove.
    if remove {
        return remove_provisioning_file(&output);
    }

    let url = url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    // Treat an empty/whitespace value as "not supplied" (None), matching url.
    // The MSI always passes --login-agreement "[RUSTLING_LOGIN_AGREEMENT]",
    // which expands to "" when the property is unset; that must NOT write
    // loginAgreementEnabled:false and clobber a previously-provisioned true.
    let login_agreement = login_agreement_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_bool);

    // Nothing to write — avoid clobbering an existing provisioning file when the
    // MSI is invoked without any provisioning directives
    // (RUSTLING_SERVER_URL / RUSTLING_LOGIN_AGREEMENT).
    if url.is_none() && login_agreement.is_none() {
        return Ok(());
    }

    let lock = if url.is_some() {
        Some(lock_value.as_deref().map(parse_bool).unwrap_or(false))
    } else {
        None
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    let config = ProvisioningConfig {
        server_url: url.as_deref(),
        lock_connection_mode: lock,
        login_agreement_enabled: login_agreement,
    };

    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize provisioning data: {}", e))?;

    fs::write(&output, json).map_err(|e| {
        format!(
            "Failed to write provisioning file {}: {}",
            output.display(),
            e
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique scratch directory. The crate deliberately carries no dev
    /// dependencies (it is cross-compiled to windows-msvc in the desktop gate),
    /// so no `tempfile`.
    fn scratch_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = env::temp_dir().join(format!(
            "rustling-provisioner-test-{}-{}-{}",
            std::process::id(),
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn remove_deletes_the_provisioning_file() {
        let dir = scratch_dir();
        let target = dir.join(PROVISIONING_FILE_NAME);
        fs::write(&target, "{}").expect("seed provisioning file");

        remove_provisioning_file(&target).expect("removal succeeds");

        assert!(!target.exists(), "provisioning file should be gone");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_is_a_no_op_when_the_file_was_never_written() {
        let dir = scratch_dir();
        let target = dir.join(PROVISIONING_FILE_NAME);

        // Uninstalling a never-provisioned install must not fail.
        remove_provisioning_file(&target).expect("missing file is success");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_leaves_the_directory_and_user_state_intact() {
        let dir = scratch_dir();
        let target = dir.join(PROVISIONING_FILE_NAME);
        let settings = dir.join("settings.yml");
        let logs = dir.join("logs");
        fs::write(&target, "{}").expect("seed provisioning file");
        fs::write(&settings, "system:\n").expect("seed user settings");
        fs::create_dir_all(&logs).expect("seed logs dir");

        remove_provisioning_file(&target).expect("removal succeeds");

        assert!(!target.exists());
        assert!(dir.is_dir(), "RustlingPDF directory must survive");
        assert!(settings.is_file(), "user settings.yml must survive");
        assert!(logs.is_dir(), "logs/ must survive");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_refuses_any_other_file_name() {
        let dir = scratch_dir();
        let settings = dir.join("settings.yml");
        fs::write(&settings, "system:\n").expect("seed user settings");

        let error = remove_provisioning_file(&settings).expect_err("must refuse");
        assert!(error.contains(PROVISIONING_FILE_NAME), "{error}");
        assert!(settings.is_file(), "refused file must be untouched");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_refuses_a_bare_directory_path() {
        let dir = scratch_dir();

        let error = remove_provisioning_file(&dir).expect_err("must refuse a directory");
        assert!(error.contains(PROVISIONING_FILE_NAME), "{error}");
        assert!(dir.is_dir(), "directory must be untouched");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_refuses_a_path_with_no_file_name() {
        // "/" (unix) and "C:\" (windows) both have no file_name component.
        let root = if cfg!(target_os = "windows") {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        let error = remove_provisioning_file(&root).expect_err("must refuse a root path");
        assert!(error.contains(PROVISIONING_FILE_NAME), "{error}");
    }
}
