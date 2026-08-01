//! Environment access for the processing runtime.

pub use std::env::{var, var_os};

/// The switch the Tauri launcher sets on the sidecar it spawns.
pub const TAURI_MODE_VARIABLE: &str = "RUSTLING_PDF_TAURI_MODE";

/// Whether this process runs as the desktop (Tauri) sidecar.
///
/// This is the single definition of "desktop mode" for the whole runtime:
/// the desktop-only CORS policy ([`crate::cors`]), SPA mobile-scanner
/// behaviour, hardware signing, and the settings bootstrap all gate on it, and
/// they must agree. Only the exact (case-insensitive, trimmed) string `true`
/// enables it, so an unset, empty, or `false` variable — every server and
/// container deployment — leaves desktop behaviour off.
#[must_use]
pub fn tauri_mode_active() -> bool {
    tauri_mode_enabled(var(TAURI_MODE_VARIABLE).ok().as_deref())
}

/// The pure predicate behind [`tauri_mode_active`], so callers can unit-test
/// both branches without mutating process environment.
#[must_use]
pub fn tauri_mode_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

#[cfg(test)]
mod tests {
    use super::tauri_mode_enabled;

    #[test]
    fn recognizes_only_the_explicit_true_switch() {
        assert!(tauri_mode_enabled(Some("true")));
        assert!(tauri_mode_enabled(Some(" TRUE ")));
        for value in [None, Some(""), Some("false"), Some("1"), Some("yes")] {
            assert!(!tauri_mode_enabled(value));
        }
    }
}
