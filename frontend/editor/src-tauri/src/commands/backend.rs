use crate::state::connection_state::{AppConnectionState, ConnectionMode};
use crate::utils::{add_log, app_data_dir};
use std::path::{Path, PathBuf};
use std::{
    env,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

// Store backend process handle and port globally
static BACKEND_PROCESS: Mutex<Option<tauri_plugin_shell::process::CommandChild>> = Mutex::new(None);
static BACKEND_STARTING: Mutex<bool> = Mutex::new(false);
static BACKEND_PORT: Mutex<Option<u16>> = Mutex::new(None);
static BACKEND_FAILURE: Mutex<Option<String>> = Mutex::new(None);

const NATIVE_BACKEND_STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const BACKEND_STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Bundled sidecar program name. Must match the `bundle.externalBin` entry in
/// `tauri.conf.json` (`binaries/stirling-processing`): the bundler strips the
/// target-triple suffix and installs the binary next to the app executable,
/// which is exactly where `ShellExt::sidecar` resolves this name.
const SIDECAR_PROGRAM: &str = "stirling-processing";

/// Environment variable the processing backend reads to locate PDFium. The
/// launcher points it at the bundled resource directory when one is packaged.
const PDFIUM_LIBRARY_PATH_ENV: &str = "STIRLING_PDFIUM_LIBRARY_PATH";

#[derive(Debug, PartialEq, Eq)]
enum NativeStartupState {
    Waiting,
    Ready(u16),
    Failed(String),
}

// Helper function to reset starting flag
fn reset_starting_flag() {
    let mut starting_guard = BACKEND_STARTING.lock().unwrap();
    *starting_guard = false;
}

fn reset_backend_runtime_state() {
    *BACKEND_PORT.lock().unwrap() = None;
    *BACKEND_FAILURE.lock().unwrap() = None;
}

fn record_backend_failure(message: String) {
    *BACKEND_PORT.lock().unwrap() = None;
    *BACKEND_FAILURE.lock().unwrap() = Some(message);
}

fn native_startup_state(
    port: Option<u16>,
    failure: Option<String>,
    process_running: bool,
) -> NativeStartupState {
    if let Some(port) = port {
        NativeStartupState::Ready(port)
    } else if let Some(failure) = failure {
        NativeStartupState::Failed(failure)
    } else if process_running {
        NativeStartupState::Waiting
    } else {
        NativeStartupState::Failed(
            "Native Rust backend terminated before reporting its port".to_string(),
        )
    }
}

async fn wait_for_native_backend_startup(timeout: Duration) -> Result<u16, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let port = *BACKEND_PORT.lock().unwrap();
        let failure = BACKEND_FAILURE.lock().unwrap().clone();
        let process_running = BACKEND_PROCESS.lock().unwrap().is_some();
        match native_startup_state(port, failure, process_running) {
            NativeStartupState::Ready(port) => return Ok(port),
            NativeStartupState::Failed(message) => {
                cleanup_backend();
                return Err(message);
            }
            NativeStartupState::Waiting => {}
        }

        if Instant::now() >= deadline {
            let message = format!(
                "Native Rust backend did not report its port within {} seconds",
                timeout.as_secs()
            );
            record_backend_failure(message.clone());
            cleanup_backend();
            return Err(message);
        }
        tokio::time::sleep(BACKEND_STARTUP_POLL_INTERVAL).await;
    }
}

// Extract port number from "Stirling-PDF running on port: PORT" log line
fn extract_port_from_running_log(log_line: &str) -> Option<u16> {
    let (_, after_prefix) = log_line.split_once("running on port: ")?;
    let port_str: String = after_prefix
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    port_str.parse::<u16>().ok()
}

// Check if backend is already running or starting
fn check_backend_status() -> Result<(), String> {
    // Check if backend is already running
    {
        let process_guard = BACKEND_PROCESS.lock().unwrap();
        if process_guard.is_some() {
            add_log("⚠️ Backend process already running, skipping start".to_string());
            return Err("Backend already running".to_string());
        }
    }

    // Check and set starting flag to prevent multiple simultaneous starts
    {
        let mut starting_guard = BACKEND_STARTING.lock().unwrap();
        if *starting_guard {
            add_log("⚠️ Backend already starting, skipping duplicate start".to_string());
            return Err("Backend startup already in progress".to_string());
        }
        *starting_guard = true;
    }

    reset_backend_runtime_state();

    Ok(())
}

/// Development-only override for the processing backend. When
/// `STIRLING_NATIVE_BACKEND_PATH` is set, the launcher starts that executable
/// instead of the bundled sidecar (useful for pointing the desktop shell at a
/// freshly built `rust/target/{debug,release}/stirling-processing`).
fn native_backend_path() -> Result<Option<PathBuf>, String> {
    let Some(path) = env::var_os("STIRLING_NATIVE_BACKEND_PATH") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(format!(
            "STIRLING_NATIVE_BACKEND_PATH must point to an executable file: {}",
            path.display()
        ));
    }
    Ok(Some(path))
}

/// Resolve the command used to launch the Rust processing backend.
///
/// Resolution order:
/// 1. `STIRLING_NATIVE_BACKEND_PATH` — explicit development override.
/// 2. The bundled `stirling-processing` sidecar (`bundle.externalBin`).
///
/// There is no further fallback: a desktop bundle without the sidecar is a
/// packaging error and must fail loudly.
fn native_backend_command(
    app: &tauri::AppHandle,
) -> Result<(tauri_plugin_shell::process::Command, String), String> {
    if let Some(override_path) = native_backend_path()? {
        let override_path = normalize_path(&override_path);
        let label = format!(
            "dev override STIRLING_NATIVE_BACKEND_PATH={}",
            override_path.display()
        );
        return Ok((
            app.shell()
                .command(override_path.to_string_lossy().as_ref()),
            label,
        ));
    }

    let command = app.shell().sidecar(SIDECAR_PROGRAM).map_err(|error| {
        format!(
            "❌ Could not resolve the bundled Rust backend sidecar '{SIDECAR_PROGRAM}': {error}. \
             The desktop bundle must ship the processing sidecar (stage it with \
             `task desktop:stage-sidecar`); set STIRLING_NATIVE_BACKEND_PATH only as a \
             development override."
        )
    })?;
    Ok((command, format!("bundled sidecar '{SIDECAR_PROGRAM}'")))
}

/// Directory holding the bundled PDFium shared library, when the desktop
/// bundle ships one (`bundle.resources` entry `resources/pdfium`). Returns
/// `None` in unpackaged development runs where the resource dir is absent.
fn bundled_pdfium_directory(app: &tauri::AppHandle) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;
    let pdfium_dir = normalize_path(&resource_dir.join("resources").join("pdfium"));
    pdfium_dir.is_dir().then_some(pdfium_dir)
}

// Normalize path to remove Windows UNC prefix
fn normalize_path(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let path_str = path.to_string_lossy();
        if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    }
}

fn migrate_legacy_workspace(legacy_dir: &Path, target_root: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(legacy_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = target_root.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}

fn migrate_legacy_workspace_and_remove(
    legacy_dir: &Path,
    target_root: &Path,
) -> std::io::Result<()> {
    migrate_legacy_workspace(legacy_dir, target_root)?;
    std::fs::remove_dir_all(legacy_dir)
}

fn migrate_legacy_workspace_if_present(app_data_root: &Path) {
    let legacy_work_dir = app_data_root.join("workspace");
    if !legacy_work_dir.exists() {
        return;
    }

    add_log(format!(
        "📦 Migrating legacy workspace from {}",
        legacy_work_dir.display()
    ));
    match migrate_legacy_workspace_and_remove(&legacy_work_dir, app_data_root) {
        Ok(()) => add_log("✅ Removed legacy workspace directory after migration".to_string()),
        Err(error) => add_log(format!("⚠️ Failed to migrate legacy workspace: {}", error)),
    }
}

fn native_backend_environment(
    work_dir: &Path,
    parent_process_id: u32,
    login_agreement_enabled: bool,
    pdfium_directory: Option<&Path>,
) -> Vec<(String, String)> {
    let config_dir = work_dir.join("configs");
    let log_dir = work_dir.join("logs");
    let mut environment = vec![
        (
            "TAURI_PARENT_PID".to_string(),
            parent_process_id.to_string(),
        ),
        ("STIRLING_PORT".to_string(), "0".to_string()),
        ("STIRLING_PDF_TAURI_MODE".to_string(), "true".to_string()),
        (
            "STIRLING_BASE_PATH".to_string(),
            work_dir.to_string_lossy().into_owned(),
        ),
        (
            "STIRLING_PDF_CONFIG_DIR".to_string(),
            config_dir.to_string_lossy().into_owned(),
        ),
        (
            "STIRLING_PDF_LOG_DIR".to_string(),
            log_dir.to_string_lossy().into_owned(),
        ),
        (
            "STIRLING_PDF_WORK_DIR".to_string(),
            work_dir.to_string_lossy().into_owned(),
        ),
    ];
    if login_agreement_enabled {
        environment.push((
            "LEGAL_LOGINAGREEMENT_ENABLED".to_string(),
            "true".to_string(),
        ));
    }
    if let Some(pdfium_directory) = pdfium_directory {
        environment.push((
            PDFIUM_LIBRARY_PATH_ENV.to_string(),
            pdfium_directory.to_string_lossy().into_owned(),
        ));
    }
    environment
}

fn run_native_backend(app: &tauri::AppHandle) -> Result<(), String> {
    let work_dir = app_data_dir();
    let config_dir = work_dir.join("configs");
    let log_dir = work_dir.join("logs");
    std::fs::create_dir_all(&work_dir).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&config_dir).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
    migrate_legacy_workspace_if_present(&work_dir);

    // PDFium wiring: an operator-set STIRLING_PDFIUM_LIBRARY_PATH in the
    // launcher's environment is inherited by the child untouched; otherwise
    // the bundled resource directory is used when the package ships one. In
    // unpackaged development runs neither exists and the variable stays
    // unset (the backend then binds a system PDFium, if any).
    let pdfium_directory = if env::var_os(PDFIUM_LIBRARY_PATH_ENV).is_some() {
        add_log(format!(
            "📚 {PDFIUM_LIBRARY_PATH_ENV} already set in the launcher environment; the backend inherits it"
        ));
        None
    } else {
        match bundled_pdfium_directory(app) {
            Some(directory) => {
                add_log(format!(
                    "📚 Using bundled PDFium runtime: {}",
                    directory.display()
                ));
                Some(directory)
            }
            None => {
                add_log(format!(
                    "⚠️ Bundled PDFium runtime not found (development run?); leaving {PDFIUM_LIBRARY_PATH_ENV} unset"
                ));
                None
            }
        }
    };

    let (mut sidecar_command, source_label) = native_backend_command(app).inspect_err(|error| {
        record_backend_failure(error.clone());
        add_log(error.clone());
    })?;
    add_log(format!(
        "🦀 Starting native Rust backend ({})",
        source_label
    ));
    sidecar_command = sidecar_command.current_dir(&work_dir);
    for (name, value) in native_backend_environment(
        &work_dir,
        std::process::id(),
        crate::commands::connection::login_agreement_enabled(app),
        pdfium_directory.as_deref(),
    ) {
        sidecar_command = sidecar_command.env(name, value);
    }
    let (rx, child) = sidecar_command.spawn().map_err(|error| {
        let message = format!(
            "❌ Failed to spawn native Rust backend ({}): {}",
            source_label, error
        );
        record_backend_failure(message.clone());
        add_log(message.clone());
        message
    })?;
    let child_pid = child.pid();
    {
        let mut process_guard = BACKEND_PROCESS.lock().unwrap();
        *process_guard = Some(child);
    }
    add_log("✅ Native Rust backend started, monitoring output...".to_string());
    monitor_backend_output(rx, child_pid);
    Ok(())
}

fn record_backend_port_for_process(process_id: u32, port: u16) -> bool {
    let process_guard = BACKEND_PROCESS.lock().unwrap();
    let is_current = process_guard
        .as_ref()
        .map(|child| child.pid() == process_id)
        .unwrap_or(false);
    if is_current {
        *BACKEND_PORT.lock().unwrap() = Some(port);
        *BACKEND_FAILURE.lock().unwrap() = None;
    }
    is_current
}

fn record_backend_failure_for_process(process_id: u32, message: String) -> bool {
    let process_guard = BACKEND_PROCESS.lock().unwrap();
    let is_current = process_guard
        .as_ref()
        .map(|child| child.pid() == process_id)
        .unwrap_or(false);
    if is_current {
        record_backend_failure(message);
    }
    is_current
}

fn remove_backend_process_and_record_failure(process_id: u32, message: String) -> bool {
    let mut process_guard = BACKEND_PROCESS.lock().unwrap();
    let is_current = process_guard
        .as_ref()
        .map(|child| child.pid() == process_id)
        .unwrap_or(false);
    if is_current {
        *process_guard = None;
        record_backend_failure(message);
    }
    is_current
}

fn detect_and_record_backend_port(process_id: u32, output: &str) -> bool {
    let Some(port) = extract_port_from_running_log(output) else {
        return false;
    };
    if !record_backend_port_for_process(process_id, port) {
        return false;
    }
    add_log(format!("🎉 Backend started on port: {}", port));
    add_log(format!("🔌 Navigate to: http://127.0.0.1:{}/", port));
    true
}

// Monitor backend output in a separate task
fn monitor_backend_output(
    mut rx: tauri::async_runtime::Receiver<tauri_plugin_shell::process::CommandEvent>,
    process_id: u32,
) {
    tokio::spawn(async move {
        let mut error_count = 0;

        while let Some(event) = rx.recv().await {
            match event {
                tauri_plugin_shell::process::CommandEvent::Stdout(output) => {
                    let output_str = String::from_utf8_lossy(&output);
                    // Strip exactly one trailing newline to avoid double newlines
                    let output_str = output_str.strip_suffix('\n').unwrap_or(&output_str);
                    add_log(format!("📤 Backend: {}", output_str));

                    // The port handshake is accepted on either stream so a
                    // logging-writer change cannot strand desktop startup.
                    detect_and_record_backend_port(process_id, output_str);
                }
                tauri_plugin_shell::process::CommandEvent::Stderr(output) => {
                    let output_str = String::from_utf8_lossy(&output);
                    // Strip exactly one trailing newline to avoid double newlines
                    let output_str = output_str.strip_suffix('\n').unwrap_or(&output_str);
                    add_log(format!("📥 Backend Error: {}", output_str));

                    detect_and_record_backend_port(process_id, output_str);

                    // Look for error indicators
                    if output_str.contains("ERROR") || output_str.contains("FATAL") {
                        error_count += 1;
                        add_log(format!("⚠️ Backend error #{}: {}", error_count, output_str));
                    }
                }
                tauri_plugin_shell::process::CommandEvent::Error(error) => {
                    let message = format!("Backend process error: {}", error);
                    record_backend_failure_for_process(process_id, message.clone());
                    add_log(format!("❌ {}", message));
                }
                tauri_plugin_shell::process::CommandEvent::Terminated(payload) => {
                    add_log(format!(
                        "💀 Backend terminated with code: {:?}",
                        payload.code
                    ));
                    if let Some(code) = payload.code {
                        match code {
                            0 => println!("✅ Process terminated normally"),
                            1 => println!("❌ Process terminated with generic error"),
                            2 => println!("❌ Process terminated due to misuse"),
                            126 => println!("❌ Command invoked cannot execute"),
                            127 => println!("❌ Command not found"),
                            128 => println!("❌ Invalid exit argument"),
                            130 => println!("❌ Process terminated by Ctrl+C"),
                            _ => println!("❌ Process terminated with code: {}", code),
                        }
                    }
                    remove_backend_process_and_record_failure(
                        process_id,
                        format!("Backend process terminated (code: {:?})", payload.code),
                    );
                }
                _ => {
                    println!("🔍 Unknown command event: {:?}", event);
                }
            }
        }

        if error_count > 0 {
            println!(
                "⚠️ Backend process ended with {} errors detected",
                error_count
            );
        }
    });
}

// Command to start the bundled Rust processing backend.
#[tauri::command]
pub async fn start_backend(
    app: tauri::AppHandle,
    connection_state: tauri::State<'_, AppConnectionState>,
) -> Result<String, String> {
    add_log("🚀 start_backend() called - starting the bundled Rust backend...".to_string());

    // Check connection mode
    let mode = {
        let state = connection_state.0.lock().map_err(|e| {
            let error_msg = format!("❌ Failed to access connection state: {}", e);
            add_log(error_msg.clone());
            error_msg
        })?;
        state.mode.clone()
    };

    match mode {
        ConnectionMode::SaaS => {
            add_log("☁️ Running in SaaS mode - starting local backend".to_string());
        }
        ConnectionMode::SelfHosted => {
            add_log("🌐 Running in Self-Hosted mode - starting local backend (for hybrid execution support)".to_string());
        }
        ConnectionMode::Local => {
            add_log("💻 Running in Local-only mode - starting local backend".to_string());
        }
    }

    // Check if backend is already running or starting
    if let Err(msg) = check_backend_status() {
        return Ok(msg);
    }

    if let Err(error) = run_native_backend(&app) {
        reset_starting_flag();
        return Err(error);
    }
    let startup_result = wait_for_native_backend_startup(NATIVE_BACKEND_STARTUP_TIMEOUT).await;
    reset_starting_flag();
    let port = startup_result?;
    Ok(format!(
        "Native Rust backend started successfully on port {}",
        port
    ))
}

// Get the dynamically assigned backend port
#[tauri::command]
pub fn get_backend_port() -> Option<u16> {
    let port_guard = BACKEND_PORT.lock().unwrap();
    *port_guard
}

// Cleanup function to stop backend on app exit
pub fn cleanup_backend() {
    let mut process_guard = BACKEND_PROCESS.lock().unwrap();
    if let Some(child) = process_guard.take() {
        let pid = child.pid();
        add_log(format!(
            "🧹 App shutting down, cleaning up backend process (PID: {})",
            pid
        ));

        match child.kill() {
            Ok(_) => {
                add_log(format!(
                    "✅ Backend process (PID: {}) terminated during cleanup",
                    pid
                ));
            }
            Err(e) => {
                add_log(format!(
                    "❌ Failed to terminate backend process during cleanup: {}",
                    e
                ));
                println!(
                    "❌ Failed to terminate backend process during cleanup: {}",
                    e
                );
            }
        }
    }
    *BACKEND_PORT.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::{
        extract_port_from_running_log, migrate_legacy_workspace_and_remove,
        native_backend_environment, native_startup_state, NativeStartupState,
    };
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> std::io::Result<Self> {
            let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "stirling-desktop-backend-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn extracts_the_stable_port_handshake_from_plain_or_decorated_output() {
        assert_eq!(
            extract_port_from_running_log("Stirling-PDF running on port: 43127"),
            Some(43_127)
        );
        assert_eq!(
            extract_port_from_running_log(
                "2026-07-18T00:00:00 INFO Stirling-PDF running on port: 8081 extra"
            ),
            Some(8_081)
        );
        assert_eq!(extract_port_from_running_log("backend ready"), None);
        assert_eq!(
            extract_port_from_running_log("Stirling-PDF running on port: invalid"),
            None
        );
    }

    #[test]
    fn native_environment_preserves_desktop_and_state_contracts(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let directory = TestDirectory::new()?;
        let work_dir = directory.path();
        let work_dir_text = work_dir.to_string_lossy();
        let config_dir_text = work_dir.join("configs").to_string_lossy().into_owned();
        let environment = native_backend_environment(work_dir, 4242, true, None)
            .into_iter()
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            environment.get("TAURI_PARENT_PID").map(String::as_str),
            Some("4242")
        );
        assert_eq!(
            environment.get("STIRLING_PORT").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            environment
                .get("STIRLING_PDF_TAURI_MODE")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            environment.get("STIRLING_BASE_PATH").map(String::as_str),
            Some(work_dir_text.as_ref())
        );
        assert_eq!(
            environment
                .get("STIRLING_PDF_CONFIG_DIR")
                .map(String::as_str),
            Some(config_dir_text.as_str())
        );
        assert_eq!(
            environment
                .get("LEGAL_LOGINAGREEMENT_ENABLED")
                .map(String::as_str),
            Some("true")
        );

        let without_agreement = native_backend_environment(work_dir, 4242, false, None)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert!(!without_agreement.contains_key("LEGAL_LOGINAGREEMENT_ENABLED"));
        Ok(())
    }

    #[test]
    fn pdfium_library_path_is_forwarded_only_when_a_bundled_directory_exists(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let directory = TestDirectory::new()?;
        let work_dir = directory.path();
        let pdfium_dir = work_dir.join("resources").join("pdfium");
        fs::create_dir_all(&pdfium_dir)?;

        let with_pdfium =
            native_backend_environment(work_dir, 4242, false, Some(pdfium_dir.as_path()))
                .into_iter()
                .collect::<BTreeMap<_, _>>();
        assert_eq!(
            with_pdfium
                .get("STIRLING_PDFIUM_LIBRARY_PATH")
                .map(String::as_str),
            Some(pdfium_dir.to_string_lossy().as_ref())
        );

        let without_pdfium = native_backend_environment(work_dir, 4242, false, None)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert!(!without_pdfium.contains_key("STIRLING_PDFIUM_LIBRARY_PATH"));
        Ok(())
    }

    #[test]
    fn migrates_and_removes_the_legacy_workspace() -> Result<(), Box<dyn std::error::Error>> {
        let directory = TestDirectory::new()?;
        let legacy = directory.path().join("workspace");
        let nested = legacy.join("customFiles").join("signatures");
        fs::create_dir_all(&nested)?;
        fs::write(nested.join("signature.json"), b"{}")?;

        migrate_legacy_workspace_and_remove(&legacy, directory.path())?;

        assert!(!legacy.exists());
        assert_eq!(
            fs::read(
                directory
                    .path()
                    .join("customFiles/signatures/signature.json")
            )?,
            b"{}"
        );
        Ok(())
    }

    #[test]
    fn lifecycle_distinguishes_ready_waiting_and_early_exit() {
        assert_eq!(
            native_startup_state(Some(31_337), None, true),
            NativeStartupState::Ready(31_337)
        );
        assert_eq!(
            native_startup_state(None, None, true),
            NativeStartupState::Waiting
        );
        assert_eq!(
            native_startup_state(None, Some("spawn failed".to_string()), true),
            NativeStartupState::Failed("spawn failed".to_string())
        );
        assert!(matches!(
            native_startup_state(None, None, false),
            NativeStartupState::Failed(message)
                if message.contains("terminated before reporting its port")
        ));
    }
}
