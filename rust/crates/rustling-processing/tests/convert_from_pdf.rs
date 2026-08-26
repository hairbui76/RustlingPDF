//! One test binary for the convert from pdf endpoints.
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

#[path = "cases/ocr_pdf_endpoint.rs"]
mod ocr_pdf_endpoint;
#[path = "cases/pdf_markdown_endpoint.rs"]
mod pdf_markdown_endpoint;
#[path = "cases/pdf_text_endpoint.rs"]
mod pdf_text_endpoint;
#[path = "cases/pdf_to_csv_endpoint.rs"]
mod pdf_to_csv_endpoint;
#[path = "cases/pdf_to_ebook_endpoint.rs"]
mod pdf_to_ebook_endpoint;
#[path = "cases/pdf_to_html_endpoint.rs"]
mod pdf_to_html_endpoint;
#[path = "cases/pdf_to_image_endpoint.rs"]
mod pdf_to_image_endpoint;
#[path = "cases/pdf_to_office_endpoint.rs"]
mod pdf_to_office_endpoint;
#[path = "cases/pdf_to_video_endpoint.rs"]
mod pdf_to_video_endpoint;
#[path = "cases/pdf_to_xlsx_endpoint.rs"]
mod pdf_to_xlsx_endpoint;
