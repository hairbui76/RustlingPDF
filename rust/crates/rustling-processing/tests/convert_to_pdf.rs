//! One test binary for the convert to pdf endpoints.
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

#[path = "cases/comic_book_endpoints.rs"]
mod comic_book_endpoints;
#[path = "cases/ebook_to_pdf_endpoint.rs"]
mod ebook_to_pdf_endpoint;
#[path = "cases/eml_to_pdf_endpoint.rs"]
mod eml_to_pdf_endpoint;
#[path = "cases/file_to_pdf_endpoint.rs"]
mod file_to_pdf_endpoint;
#[path = "cases/html_to_pdf_endpoint.rs"]
mod html_to_pdf_endpoint;
#[path = "cases/markdown_to_pdf_endpoint.rs"]
mod markdown_to_pdf_endpoint;
#[path = "cases/svg_to_pdf_endpoint.rs"]
mod svg_to_pdf_endpoint;
#[path = "cases/url_to_pdf_endpoint.rs"]
mod url_to_pdf_endpoint;
