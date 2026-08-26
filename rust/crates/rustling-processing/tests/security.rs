//! One test binary for the security endpoints.
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

#[path = "cases/auto_redaction_endpoint.rs"]
mod auto_redaction_endpoint;
#[path = "cases/cert_sign_endpoint.rs"]
mod cert_sign_endpoint;
#[path = "cases/flatten_endpoint.rs"]
mod flatten_endpoint;
#[path = "cases/hardware_signing_endpoint.rs"]
mod hardware_signing_endpoint;
#[path = "cases/metadata_endpoint.rs"]
mod metadata_endpoint;
#[path = "cases/password_endpoints.rs"]
mod password_endpoints;
#[path = "cases/pdf_info_endpoint.rs"]
mod pdf_info_endpoint;
#[path = "cases/pdf_redaction_endpoint.rs"]
mod pdf_redaction_endpoint;
#[path = "cases/redact_execute_endpoint.rs"]
mod redact_execute_endpoint;
#[path = "cases/sanitize_endpoint.rs"]
mod sanitize_endpoint;
#[path = "cases/stamp_endpoint.rs"]
mod stamp_endpoint;
#[path = "cases/validate_signature_endpoint.rs"]
mod validate_signature_endpoint;
#[path = "cases/verify_pdf_endpoint.rs"]
mod verify_pdf_endpoint;
#[path = "cases/watermark_endpoint.rs"]
mod watermark_endpoint;
