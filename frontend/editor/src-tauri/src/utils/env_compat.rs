//! `RUSTLING_*` environment variables with `STIRLING_*` back-compat aliases.
//!
//! Mirror of the processing backend's `env_compat` module for the few
//! variables the desktop launcher itself reads: the `RUSTLING_*` spelling is
//! primary, the legacy `STIRLING_*` spelling keeps working, and `RUSTLING_*`
//! wins when both are set. The launcher does not log the deprecation warning
//! itself — the sidecar inherits the launcher environment and warns once at
//! its own startup.

use std::env;
use std::ffi::OsString;

const PRIMARY_PREFIX: &str = "RUSTLING_";
const LEGACY_PREFIX: &str = "STIRLING_";

/// The legacy `STIRLING_*` spelling of a `RUSTLING_*` variable name, `None`
/// for names outside the product prefix.
fn legacy_alias(name: &str) -> Option<String> {
    name.strip_prefix(PRIMARY_PREFIX)
        .map(|suffix| format!("{LEGACY_PREFIX}{suffix}"))
}

/// [`std::env::var_os`] with the legacy-alias fallback: a `RUSTLING_*` name
/// that is not present in the environment is retried under its `STIRLING_*`
/// spelling.
pub fn var_os(name: &str) -> Option<OsString> {
    env::var_os(name).or_else(|| legacy_alias(name).and_then(env::var_os))
}
