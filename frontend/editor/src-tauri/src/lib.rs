use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

pub mod commands;
pub mod launch_intent;
mod utils;

use commands::connection::apply_provisioning_if_present;
use commands::{
    add_opened_file, can_install_updates, check_for_update, cleanup_backend, clear_opened_files,
    complete_setup, download_and_install_update, forward_files_to_window,
    forward_files_to_window_with_intent, get_app_version, get_backend_port, get_desktop_os,
    get_opened_files, get_update_mode, is_default_pdf_handler, is_first_launch,
    open_files_in_new_window, open_in_new_window, pop_opened_batches, pop_opened_files,
    pop_window_file_ids, print_pdf_file_native, proxy_local_pdf_request, reset_setup_completion,
    restart_app, set_as_default_pdf_handler, set_update_mode, start_backend, target_window_label,
    MAIN_WINDOW_LABEL,
};
use launch_intent::{debounce_intent_batch, parse_launch_args, INTENT_AGGREGATOR};
use utils::{add_log, get_tauri_logs};

// Buffer an intent-carrying launch (Explorer context menu `--tool <action>`)
// in the process-wide aggregator and schedule the debounce sleeper that may
// flush it. Explorer starts one process per selected file, so N selected
// files become N calls here; exactly one sleeper forwards the merged batch to
// the target window (picked at flush time), as a single `files-changed`.
fn queue_intent_launch(app: &AppHandle, tool: String, files: Vec<String>) {
    let generation = INTENT_AGGREGATOR.add(&tool, files, tokio::time::Instant::now());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(batch) = debounce_intent_batch(&INTENT_AGGREGATOR, &tool, generation).await {
            let label = target_window_label(&app).unwrap_or_else(|| MAIN_WINDOW_LABEL.to_string());
            add_log(format!(
                "📂 Forwarding {} file(s) with intent '{}' to window '{}'",
                batch.len(),
                tool,
                label
            ));
            forward_files_to_window_with_intent(&app, &label, batch, Some(tool));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Runs in the existing instance when a second launch is attempted
            // (e.g. "open with" / double-click while the app is running).
            add_log(format!("📂 Second instance detected with args: {:?}", args));

            let parsed = parse_launch_args(&args);
            // Route to the window the user is in (focused -> main -> any) so opens
            // consolidate into one window instead of spawning a new one.
            let label = target_window_label(app).unwrap_or_else(|| MAIN_WINDOW_LABEL.to_string());

            if parsed.files.is_empty() {
                if let Some(window) = app.get_webview_window(&label) {
                    // No files: just bring the app to the front.
                    let _ = window.set_focus();
                    let _ = window.unminimize();
                }
            } else if let Some(tool) = parsed.tool {
                // Context-menu launch: aggregate multi-select siblings before
                // forwarding (Explorer fires one process per selected file).
                queue_intent_launch(app, tool, parsed.files);
            } else {
                add_log(format!(
                    "📂 Forwarding {} file(s) to existing window '{}'",
                    parsed.files.len(),
                    label
                ));
                forward_files_to_window(app, &label, parsed.files);
            }
        }))
        .setup(|app| {
            add_log("🚀 Tauri app setup started".to_string());

            // Files passed on the command line at first launch load into the main
            // window once the frontend mounts.
            let args: Vec<String> = std::env::args().collect();
            let parsed = parse_launch_args(&args);
            if let Some(tool) = parsed.tool {
                // Cold context-menu launch: route through the same aggregator
                // as the single-instance callback so the sibling processes of
                // a multi-select merge into this batch. The flushed batch
                // lands in the queue; the frontend pops it on mount.
                if !parsed.files.is_empty() {
                    add_log(format!(
                        "📂 Initial intent '{}' with {} file(s) from command line",
                        tool,
                        parsed.files.len()
                    ));
                    queue_intent_launch(app.handle(), tool, parsed.files);
                }
            } else {
                for path in parsed.files {
                    add_log(format!("📂 Initial file from command line: {}", path));
                    add_opened_file(path);
                }
            }

            if let Err(err) = apply_provisioning_if_present(app.handle()) {
                add_log(format!("⚠️ Failed to apply provisioning file: {}", err));
            }

            // Start backend immediately, non-blocking
            let app_handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                add_log("🚀 Starting bundled backend in background".to_string());
                match commands::backend::start_backend(app_handle.clone()).await {
                    Ok(_) => {
                        if let Some(port) = commands::backend::get_backend_port() {
                            let script = format!(
                                "window.localStorage.setItem(\
                                 'rustlingpdf.desktopBackendUrl',\
                                 'http://127.0.0.1:{port}');\
                                 window.location.reload();"
                            );
                            if let Some(window) =
                                app_handle.get_webview_window(MAIN_WINDOW_LABEL)
                            {
                                if let Err(error) = window.eval(&script) {
                                    add_log(format!(
                                        "⚠️ Could not connect the desktop UI to the backend: {error}"
                                    ));
                                }
                            }
                        }
                    }
                    Err(error) => {
                        add_log(format!("⚠️ Backend start failed: {error}"));
                    }
                }
            });

            add_log("🔍 DEBUG: Setup completed".to_string());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_backend,
            get_backend_port,
            get_opened_files,
            pop_opened_files,
            pop_opened_batches,
            clear_opened_files,
            open_in_new_window,
            open_files_in_new_window,
            pop_window_file_ids,
            get_tauri_logs,
            is_default_pdf_handler,
            set_as_default_pdf_handler,
            is_first_launch,
            complete_setup,
            reset_setup_completion,
            proxy_local_pdf_request,
            get_desktop_os,
            print_pdf_file_native,
            can_install_updates,
            check_for_update,
            download_and_install_update,
            get_app_version,
            get_update_mode,
            set_update_mode,
            restart_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                RunEvent::ExitRequested { .. } => {
                    add_log("🔄 App exit requested, cleaning up...".to_string());
                    cleanup_backend();
                    // Use Tauri's built-in cleanup
                    app_handle.cleanup_before_exit();
                }
                RunEvent::WindowEvent {
                    event: WindowEvent::CloseRequested { .. },
                    ..
                } => {
                    add_log(
                        "🔄 Window close requested (will cleanup on actual exit)...".to_string(),
                    );
                    // Don't cleanup here - let JavaScript handler prevent close if needed
                    // Backend cleanup happens in ExitRequested when window actually closes
                }
                RunEvent::WindowEvent {
                    event: WindowEvent::DragDrop(drag_drop_event),
                    label,
                    ..
                } => {
                    use tauri::DragDropEvent;
                    if let DragDropEvent::Drop { paths, .. } = drag_drop_event {
                        add_log(format!(
                            "📂 Files dropped on window '{}': {:?}",
                            label, paths
                        ));
                        let file_paths: Vec<String> = paths
                            .iter()
                            .filter_map(|p| p.to_str().map(|s| s.to_string()))
                            .collect();

                        // Route to the window the file was actually dropped on.
                        if !file_paths.is_empty() {
                            forward_files_to_window(app_handle, &label, file_paths);
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                RunEvent::Opened { urls } => {
                    use urlencoding::decode;

                    add_log(format!("📂 Tauri file opened event: {:?}", urls));
                    let file_paths: Vec<String> = urls
                        .iter()
                        .filter_map(|url| {
                            let url_str = url.as_str();
                            if !url_str.starts_with("file://") {
                                return None;
                            }
                            let encoded = url_str.strip_prefix("file://").unwrap_or(url_str);
                            // Decode URL-encoded characters (%20 -> space, etc.)
                            match decode(encoded) {
                                Ok(decoded) => Some(decoded.into_owned()),
                                Err(e) => {
                                    add_log(format!(
                                        "⚠️ Failed to decode file path: {} - {}",
                                        encoded, e
                                    ));
                                    Some(encoded.to_string())
                                }
                            }
                        })
                        .collect();

                    if !file_paths.is_empty() {
                        // Route to the window the user is in (focused -> main -> any).
                        let label = target_window_label(app_handle)
                            .unwrap_or_else(|| MAIN_WINDOW_LABEL.to_string());
                        forward_files_to_window(app_handle, &label, file_paths);
                    }
                }
                _ => {
                    // Only log unhandled events in debug mode to reduce noise
                    // #[cfg(debug_assertions)]
                    // add_log(format!("🔍 DEBUG: Unhandled event: {:?}", event));
                }
            }
        });
}
