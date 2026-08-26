//! One test binary for the service endpoints.
//!
//! Every file under `cases/` used to be its own integration-test target, which
//! meant its own link of the whole crate — 91 links per `cargo test`, and the
//! reason the local gate spent most of its time in "Compiling". Grouping them
//! as modules of one binary links once per group. Each case keeps its private
//! helpers (the same `require_status` / `response_bytes` copies it always
//! had), so nothing is shared and nothing collides; only the test names gain a
//! module prefix. Cases that touch process-global state stay as standalone
//! targets at the top of `tests/` on purpose: environment variables, the
//! current directory, `current_exe`, and — the one that bit first —
//! binding `PDFium` directly, which `pdfium-render` allows once per process and
//! which the crate's own `PDFIUM` `OnceLock` has usually done already by the
//! time a later module runs.

#[path = "cases/additional_language_endpoint.rs"]
mod additional_language_endpoint;
#[path = "cases/config_endpoints.rs"]
mod config_endpoints;
#[path = "cases/create_pdf_agent_endpoint.rs"]
mod create_pdf_agent_endpoint;
#[path = "cases/filter_endpoints.rs"]
mod filter_endpoints;
#[path = "cases/info_endpoints.rs"]
mod info_endpoints;
#[path = "cases/login_disclaimer_endpoint.rs"]
mod login_disclaimer_endpoint;
#[path = "cases/math_auditor_agent_endpoint.rs"]
mod math_auditor_agent_endpoint;
#[path = "cases/mobile_scanner_endpoints.rs"]
mod mobile_scanner_endpoints;
#[path = "cases/pdf_comment_agent_endpoint.rs"]
mod pdf_comment_agent_endpoint;
#[path = "cases/pipeline_endpoint.rs"]
mod pipeline_endpoint;
#[path = "cases/robots_endpoint.rs"]
mod robots_endpoint;
#[path = "cases/ui_data_endpoints.rs"]
mod ui_data_endpoints;
