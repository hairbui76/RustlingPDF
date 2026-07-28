pub mod backend;
pub mod connection;
pub mod default_app;
pub mod files;
pub mod local_proxy;
pub mod platform;
pub mod print;
pub mod updater;
pub mod window;

pub use backend::{cleanup_backend, get_backend_port, start_backend};
pub use connection::{
    complete_setup, get_update_mode, is_first_launch, reset_setup_completion, set_update_mode,
};
pub use default_app::{is_default_pdf_handler, set_as_default_pdf_handler};
pub use files::{add_opened_file, clear_opened_files, get_opened_files, pop_opened_files};
pub use local_proxy::proxy_local_pdf_request;
pub use platform::get_desktop_os;
pub use print::print_pdf_file_native;
pub use updater::{
    can_install_updates, check_for_update, download_and_install_update, get_app_version,
    restart_app,
};
pub use window::{
    forward_files_to_window, open_files_in_new_window, open_in_new_window, pop_window_file_ids,
    target_window_label, MAIN_WINDOW_LABEL,
};
