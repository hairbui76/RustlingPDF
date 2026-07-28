//! `RUSTLING_*` environment variables with `STIRLING_*` back-compat aliases.
//!
//! RustlingPDF was ported from Stirling-PDF, and its configuration variables
//! historically used the `STIRLING_*` prefix. The `RUSTLING_*` spelling is the
//! primary one; every legacy `STIRLING_*` spelling keeps working as a
//! deprecated alias so existing deployments do not break. When both spellings
//! are set, the `RUSTLING_*` one wins. Names outside the `RUSTLING_` prefix
//! behave exactly like `std::env`, so every configuration read can be routed
//! through this module unconditionally.
//!
//! The binaries call [`warn_once_on_legacy_environment`] at startup so a
//! process configured through legacy spellings logs a single deprecation line
//! instead of one warning per read.

use std::env::{self, VarError};
use std::ffi::OsString;
use std::sync::Once;

const PRIMARY_PREFIX: &str = "RUSTLING_";
const LEGACY_PREFIX: &str = "STIRLING_";

/// The legacy `STIRLING_*` spelling of a `RUSTLING_*` variable name, `None`
/// for names outside the product prefix.
fn legacy_alias(name: &str) -> Option<String> {
    name.strip_prefix(PRIMARY_PREFIX)
        .map(|suffix| format!("{LEGACY_PREFIX}{suffix}"))
}

/// [`std::env::var`] with the legacy-alias fallback: a `RUSTLING_*` name that
/// is not present in the environment is retried under its `STIRLING_*`
/// spelling. A present-but-invalid primary value is reported as-is and never
/// falls back.
///
/// # Errors
///
/// Exactly the [`std::env::var`] errors: [`VarError::NotPresent`] when neither
/// spelling is set, [`VarError::NotUnicode`] when the value that won the
/// lookup is not valid Unicode.
// The closure is load-bearing, not redundant: `std::env::var` is generic over
// its key type, so the plain fn item only implements `Fn` for one specific
// lifetime and cannot be passed where a higher-ranked `Fn(&str)` is needed.
#[allow(clippy::redundant_closure)]
pub fn var(name: &str) -> Result<String, VarError> {
    var_with(|candidate| env::var(candidate), name)
}

/// [`std::env::var_os`] with the same legacy-alias fallback as [`var`].
#[must_use]
pub fn var_os(name: &str) -> Option<OsString> {
    env::var_os(name).or_else(|| legacy_alias(name).and_then(|alias| env::var_os(alias)))
}

/// [`var`] against an injectable environment, so the precedence rules stay
/// unit-testable without mutating real process state.
fn var_with(
    lookup: impl Fn(&str) -> Result<String, VarError>,
    name: &str,
) -> Result<String, VarError> {
    match lookup(name) {
        Err(VarError::NotPresent) => match legacy_alias(name) {
            Some(alias) => lookup(&alias),
            None => Err(VarError::NotPresent),
        },
        primary => primary,
    }
}

/// Logs one deprecation line (to stderr, so it is visible regardless of
/// `RUST_LOG`) when legacy `STIRLING_*` variables are present in the process
/// environment. Called by the binaries at startup; every later call is a
/// no-op, so a legacy-configured process warns once — never per read.
pub fn warn_once_on_legacy_environment() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let mut legacy: Vec<String> = env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .filter(|name| name.starts_with(LEGACY_PREFIX))
            .collect();
        if legacy.is_empty() {
            return;
        }
        legacy.sort_unstable();
        eprintln!(
            "deprecated: {} environment variable spellings are deprecated ({}); use the RUSTLING_* spellings — legacy values keep working, and RUSTLING_* wins when both are set",
            LEGACY_PREFIX.trim_end_matches('_'),
            legacy.join(", ")
        );
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::env::VarError;

    use super::{legacy_alias, var_with};

    fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Result<String, VarError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned().ok_or(VarError::NotPresent)
    }

    #[test]
    fn primary_spelling_wins_over_legacy() {
        let lookup = environment(&[("RUSTLING_PORT", "8080"), ("STIRLING_PORT", "9090")]);
        assert_eq!(var_with(lookup, "RUSTLING_PORT").ok(), Some("8080".into()));
    }

    #[test]
    fn legacy_spelling_backfills_missing_primary() {
        let lookup = environment(&[("STIRLING_PORT", "9090")]);
        assert_eq!(var_with(lookup, "RUSTLING_PORT").ok(), Some("9090".into()));
    }

    #[test]
    fn absent_both_spellings_is_not_present() {
        let lookup = environment(&[]);
        assert_eq!(var_with(lookup, "RUSTLING_PORT"), Err(VarError::NotPresent));
    }

    #[test]
    fn names_outside_the_product_prefix_have_no_alias() {
        assert_eq!(legacy_alias("SERVER_PORT"), None);
        assert_eq!(legacy_alias("SECURITY_ENABLELOGIN"), None);
        let lookup = environment(&[("STIRLING_SERVER_PORT", "1")]);
        assert_eq!(var_with(lookup, "SERVER_PORT"), Err(VarError::NotPresent));
    }

    #[test]
    fn every_product_name_maps_to_its_legacy_spelling() {
        assert_eq!(
            legacy_alias("RUSTLING_PDFIUM_LIBRARY_PATH").as_deref(),
            Some("STIRLING_PDFIUM_LIBRARY_PATH")
        );
        assert_eq!(
            legacy_alias("RUSTLING_PDF_TAURI_MODE").as_deref(),
            Some("STIRLING_PDF_TAURI_MODE")
        );
    }
}
