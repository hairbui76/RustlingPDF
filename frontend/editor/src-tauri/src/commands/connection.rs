use crate::utils::{add_log, app_data_dir, system_provisioning_dir};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "connection.json";
const FIRST_LAUNCH_KEY: &str = "setup_completed";
const LOGIN_AGREEMENT_KEY: &str = "login_agreement_enabled";
const PROVISIONING_FILE_NAME: &str = "rustling-provisioning.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvisioningConfig {
    login_agreement_enabled: Option<bool>,
}

fn provisioning_file_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(app_data_dir().join(PROVISIONING_FILE_NAME));

    if let Some(system_dir) = system_provisioning_dir() {
        paths.push(system_dir.join(PROVISIONING_FILE_NAME));
    }

    paths
}

pub fn apply_provisioning_if_present(app_handle: &AppHandle) -> Result<(), String> {
    let provisioning_paths = provisioning_file_paths();
    let provisioning_path = provisioning_paths.into_iter().find(|path| path.exists());

    let provisioning_path = match provisioning_path {
        Some(path) => path,
        None => return Ok(()),
    };

    add_log(format!(
        "🧩 Provisioning file detected: {}",
        provisioning_path.display()
    ));

    let raw = fs::read_to_string(&provisioning_path)
        .map_err(|e| format!("Failed to read provisioning file: {}", e))?;
    let parsed: ProvisioningConfig = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse provisioning file: {}", e))?;

    // Login agreement can be provisioned independently of a server URL so it also applies to
    // local, no-login desktop installs. Persist it before the server-URL handling below, which
    // may early-return when no URL is present.
    if let Some(login_agreement_enabled) = parsed.login_agreement_enabled {
        if let Ok(store) = app_handle.store(STORE_FILE) {
            store.set(
                LOGIN_AGREEMENT_KEY,
                serde_json::json!(login_agreement_enabled),
            );
            let _ = store.save();
        }
        add_log(format!(
            "🧩 Provisioned login agreement enabled = {}",
            login_agreement_enabled
        ));
    }

    // Only short-circuit when there is nothing to apply. login_agreement is handled above, but it
    // must still be in this guard so a login-agreement-only file falls through to the deletion
    // block at the end (otherwise the per-user file would linger and re-apply forever).
    if parsed.login_agreement_enabled.is_none() {
        add_log(
            "⚠️ Provisioning file has no actionable fields (loginAgreement); skipping apply"
                .to_string(),
        );
        return Ok(());
    }

    let user_app_data = app_data_dir();
    if provisioning_path.starts_with(&user_app_data) {
        match fs::remove_file(&provisioning_path) {
            Ok(_) => add_log("✅ Provisioning file applied and removed".to_string()),
            Err(err) => add_log(format!(
                "⚠️ Provisioning applied but failed to remove file: {}",
                err
            )),
        }
    } else {
        add_log("ℹ️ Provisioning applied from system location; leaving file in place".to_string());
    }

    Ok(())
}

/// Whether the login agreement was provisioned as enabled. Read by the backend launcher to pass
/// the `-Dlegal.loginAgreement.enabled` flag to the bundled JVM in local desktop mode.
pub fn login_agreement_enabled(app_handle: &AppHandle) -> bool {
    app_handle
        .store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(LOGIN_AGREEMENT_KEY))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn is_first_launch(app_handle: AppHandle) -> Result<bool, String> {
    let store = app_handle
        .store(STORE_FILE)
        .map_err(|e| format!("Failed to access store: {}", e))?;

    let setup_completed = store
        .get(FIRST_LAUNCH_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(!setup_completed)
}

/// Mark the first-launch setup as completed so the onboarding bootstrap only
/// runs once. Called by the frontend after the bundled backend has been
/// started on first launch.
#[tauri::command]
pub async fn complete_setup(app_handle: AppHandle) -> Result<(), String> {
    let store = app_handle
        .store(STORE_FILE)
        .map_err(|e| format!("Failed to access store: {}", e))?;

    store.set(FIRST_LAUNCH_KEY, serde_json::json!(true));

    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn reset_setup_completion(app_handle: AppHandle) -> Result<(), String> {
    log::info!("Resetting setup completion flag");

    let store = app_handle
        .store(STORE_FILE)
        .map_err(|e| format!("Failed to access store: {}", e))?;

    // Reset setup completion flag to force the first-launch bootstrap on next launch
    store.set(FIRST_LAUNCH_KEY, serde_json::json!(false));

    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    log::info!("Setup completion flag reset successfully");
    Ok(())
}
