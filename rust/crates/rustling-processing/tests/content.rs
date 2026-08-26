//! One test binary for the content endpoints.
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
//! binding PDFium directly, which pdfium-render allows once per process and
//! which the crate's own `PDFIUM` OnceLock has usually done already by the
//! time a later module runs.

#[path = "cases/accessibility_endpoints.rs"]
mod accessibility_endpoints;
#[path = "cases/analysis_endpoints.rs"]
mod analysis_endpoints;
#[path = "cases/attachments_endpoints.rs"]
mod attachments_endpoints;
#[path = "cases/auto_rename_endpoint.rs"]
mod auto_rename_endpoint;
#[path = "cases/classification_endpoint.rs"]
mod classification_endpoint;
#[path = "cases/comments_endpoint.rs"]
mod comments_endpoint;
#[path = "cases/compress_endpoint.rs"]
mod compress_endpoint;
#[path = "cases/document_navigation_endpoints.rs"]
mod document_navigation_endpoints;
#[path = "cases/document_ops_endpoints.rs"]
mod document_ops_endpoints;
#[path = "cases/edit_text_endpoint.rs"]
mod edit_text_endpoint;
#[path = "cases/extract_images_endpoint.rs"]
mod extract_images_endpoint;
#[path = "cases/form_fields_endpoints.rs"]
mod form_fields_endpoints;
#[path = "cases/pdf_text_editor_endpoint.rs"]
mod pdf_text_editor_endpoint;
#[path = "cases/pdf_text_editor_lazy_endpoint.rs"]
mod pdf_text_editor_lazy_endpoint;
#[path = "cases/pdf_text_editor_metadata_endpoint.rs"]
mod pdf_text_editor_metadata_endpoint;
#[path = "cases/replace_invert_endpoint.rs"]
mod replace_invert_endpoint;
