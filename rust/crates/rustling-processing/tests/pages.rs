//! One test binary for the pages endpoints.
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

#[path = "cases/auto_split_endpoint.rs"]
mod auto_split_endpoint;
#[path = "cases/booklet_endpoint.rs"]
mod booklet_endpoint;
#[path = "cases/extract_image_scans_endpoint.rs"]
mod extract_image_scans_endpoint;
#[path = "cases/geometry_endpoints.rs"]
mod geometry_endpoints;
#[path = "cases/image_overlay_endpoint.rs"]
mod image_overlay_endpoint;
#[path = "cases/merge_endpoint.rs"]
mod merge_endpoint;
#[path = "cases/overlay_endpoint.rs"]
mod overlay_endpoint;
#[path = "cases/page_numbers_endpoint.rs"]
mod page_numbers_endpoint;
#[path = "cases/poster_endpoint.rs"]
mod poster_endpoint;
#[path = "cases/rearrange_pages_endpoint.rs"]
mod rearrange_pages_endpoint;
#[path = "cases/remove_blanks_endpoint.rs"]
mod remove_blanks_endpoint;
#[path = "cases/remove_pages_endpoint.rs"]
mod remove_pages_endpoint;
#[path = "cases/rotate_endpoint.rs"]
mod rotate_endpoint;
#[path = "cases/scanner_effect_endpoint.rs"]
mod scanner_effect_endpoint;
#[path = "cases/split_by_size_endpoint.rs"]
mod split_by_size_endpoint;
#[path = "cases/split_chapters_endpoint.rs"]
mod split_chapters_endpoint;
#[path = "cases/split_endpoint.rs"]
mod split_endpoint;
#[path = "cases/split_sections_endpoint.rs"]
mod split_sections_endpoint;
