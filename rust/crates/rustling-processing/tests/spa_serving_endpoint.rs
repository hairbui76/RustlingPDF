//! Integration tests for single-binary SPA serving (`rust/contracts/spa-serving.md`).
//!
//! A synthetic Vite dist directory exercises the full fallback pipeline:
//! index transformation, deep links, API precedence, static assets with
//! upstream cache tiers, precompressed negotiation, traversal/symlink
//! hardening, and the config-unset inertness guarantee.

use std::fs;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use rustling_processing::{
    TimestampSettings, app_with_runtime_config, runtime_config::RuntimeConfig,
};
use tempfile::{TempDir, tempdir};
use tower::ServiceExt;

const INDEX_HTML: &str = "<!doctype html>\n<html>\n  <head>\n    <base href=\"%BASE_URL%\" />\n    <title>Stirling PDF</title>\n  </head>\n  <body>SPA-INDEX-BODY</body>\n</html>\n";

struct SyntheticDist {
    /// Owns configs/, customFiles/, dist/, and the outside-the-root secret.
    root: TempDir,
}

impl SyntheticDist {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let dist = root.path().join("dist");
        fs::create_dir_all(dist.join("assets"))?;
        fs::create_dir_all(dist.join("locales/en-US"))?;
        fs::create_dir_all(dist.join("images"))?;
        fs::write(dist.join("index.html"), INDEX_HTML)?;
        fs::write(dist.join("assets/app-abc123.js"), "plain-js-bundle")?;
        fs::write(dist.join("assets/app-abc123.js.br"), "brotli-bytes")?;
        fs::write(dist.join("assets/app-abc123.css"), "body{}")?;
        fs::write(
            dist.join("locales/en-US/translation.json"),
            "{\"hello\":\"world\"}",
        )?;
        fs::write(dist.join("images/logo.png"), b"\x89PNG-not-really")?;
        fs::write(dist.join("robots.txt"), "User-agent: *\n")?;
        fs::write(dist.join("og-metadata.json"), "{\"og\":true}")?;
        fs::write(dist.join("manifest.json"), "{}")?;
        // A secret OUTSIDE the dist root that must never be reachable.
        fs::write(root.path().join("secret.txt"), "TOP-SECRET")?;
        Ok(Self { root })
    }

    fn dist(&self) -> std::path::PathBuf {
        self.root.path().join("dist")
    }

    fn runtime_config(&self) -> Result<RuntimeConfig, Box<dyn std::error::Error>> {
        let configs = self.root.path().join("configs");
        fs::create_dir_all(&configs)?;
        let settings = configs.join("settings.yml");
        fs::write(
            &settings,
            format!("system:\n  frontendDist: {}\n", self.dist().display()),
        )?;
        Ok(RuntimeConfig::from_files(
            settings,
            configs.join("missing.yml"),
        ))
    }
}

async fn send(
    runtime_config: RuntimeConfig,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> Result<Response, Box<dyn std::error::Error>> {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    Ok(
        app_with_runtime_config(1024 * 1024, TimestampSettings::default(), runtime_config)
            .oneshot(request.body(Body::empty())?)
            .await?,
    )
}

async fn get(
    runtime_config: RuntimeConfig,
    uri: &str,
) -> Result<Response, Box<dyn std::error::Error>> {
    send(runtime_config, "GET", uri, &[]).await
}

async fn body_string(response: Response) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024).await?;
    Ok(String::from_utf8(bytes.to_vec())?)
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

fn assert_index_response(response: &Response) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(response, "content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        header(response, "cache-control"),
        Some("no-cache, must-revalidate")
    );
}

#[tokio::test]
async fn unset_config_keeps_todays_plain_404_behavior() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let settings = directory.path().join("settings.yml");
    fs::write(&settings, "system:\n  defaultLocale: en-US\n")?;
    let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
    for uri in [
        "/",
        "/index.html",
        "/compress-pdf",
        "/auth/callback",
        "/assets/app.js",
    ] {
        let response = get(config.clone(), uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
        assert!(body_string(response).await?.is_empty(), "uri: {uri}");
    }
    Ok(())
}

#[tokio::test]
async fn serves_transformed_index_at_root_and_index_html() -> Result<(), Box<dyn std::error::Error>>
{
    let dist = SyntheticDist::new()?;
    for uri in ["/", "/index.html"] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_index_response(&response);
        let body = body_string(response).await?;
        assert!(body.contains("SPA-INDEX-BODY"), "uri: {uri}");
        assert!(body.contains("<base href=\"/\" />"), "uri: {uri}");
        assert!(!body.contains("%BASE_URL%"), "uri: {uri}");
        assert!(
            body.contains("window.RUSTLING_PDF_API_BASE_URL = '/';"),
            "uri: {uri}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn spa_deep_links_and_client_routes_serve_index() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    for uri in [
        "/auth/callback",
        "/share/some-token",
        "/share/token.with.dots",
        "/audit",
        "/compress-pdf",
        "/login",
        "/files",
        "/files/8b3f1f9e-uuid",
        "/some-route?query=1",
    ] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_index_response(&response);
        assert!(
            body_string(response).await?.contains("SPA-INDEX-BODY"),
            "uri: {uri}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn tauri_auth_callback_serves_standalone_completion_page()
-> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    let response = get(dist.runtime_config()?, "/auth/callback/tauri").await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, "content-type"),
        Some("text/html; charset=utf-8")
    );
    // Upstream sets no cache-control header on this page.
    assert_eq!(header(&response, "cache-control"), None);
    let body = body_string(response).await?;
    assert!(body.contains("Authentication complete"));
    assert!(body.contains("stirlingpdf://auth/sso-complete"));
    Ok(())
}

#[tokio::test]
async fn mobile_scanner_serves_index_outside_desktop_mode() -> Result<(), Box<dyn std::error::Error>>
{
    // The desktop (Tauri-mode) variant is covered by unit tests in `src/spa.rs`
    // because flipping RUSTLING_PDF_TAURI_MODE in-process is not allowed here.
    let dist = SyntheticDist::new()?;
    let response = get(dist.runtime_config()?, "/mobile-scanner").await?;
    assert_index_response(&response);
    assert!(body_string(response).await?.contains("SPA-INDEX-BODY"));
    Ok(())
}

#[tokio::test]
async fn api_routes_keep_precedence_and_unknown_api_paths_stay_404()
-> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    // A real API route still answers JSON, not the SPA index.
    let response = get(dist.runtime_config()?, "/api/v1/config/app-config").await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        header(&response, "content-type")
            .is_some_and(|content_type| content_type.starts_with("application/json"))
    );
    // /health keeps working.
    let response = get(dist.runtime_config()?, "/health").await?;
    assert_eq!(response.status(), StatusCode::OK);
    // Unknown API paths must NOT fall back to index.html.
    for uri in ["/api/v1/does-not-exist", "/api/nope", "/api"] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
        assert!(
            !body_string(response).await?.contains("SPA-INDEX-BODY"),
            "uri: {uri}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn static_assets_get_upstream_cache_tiers_and_mime_types()
-> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;

    let response = get(dist.runtime_config()?, "/assets/app-abc123.js").await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-type"), Some("text/javascript"));
    assert_eq!(
        header(&response, "cache-control"),
        Some("max-age=31536000, public, immutable")
    );
    assert_eq!(header(&response, "vary"), Some("Accept-Encoding"));
    assert!(header(&response, "last-modified").is_some());
    assert_eq!(body_string(response).await?, "plain-js-bundle");

    let response = get(dist.runtime_config()?, "/images/logo.png").await?;
    assert_eq!(header(&response, "content-type"), Some("image/png"));
    assert_eq!(
        header(&response, "cache-control"),
        Some("max-age=31536000, public, immutable")
    );

    let response = get(dist.runtime_config()?, "/locales/en-US/translation.json").await?;
    assert_eq!(header(&response, "content-type"), Some("application/json"));
    assert_eq!(
        header(&response, "cache-control"),
        Some("max-age=86400, public, stale-while-revalidate=604800")
    );

    let response = get(dist.runtime_config()?, "/manifest.json").await?;
    assert_eq!(header(&response, "cache-control"), Some("no-store"));

    // A top-level file outside every special tier gets the no-cache catch-all.
    // (/robots.txt itself is answered by the dedicated RobotsController route,
    // which keeps precedence over the SPA fallback.)
    let response = get(dist.runtime_config()?, "/og-metadata.json").await?;
    assert_eq!(header(&response, "cache-control"), Some("no-cache"));
    assert_eq!(header(&response, "content-type"), Some("application/json"));
    assert_eq!(body_string(response).await?, "{\"og\":true}");
    Ok(())
}

#[tokio::test]
async fn precompressed_variant_is_negotiated_via_accept_encoding()
-> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    let response = send(
        dist.runtime_config()?,
        "GET",
        "/assets/app-abc123.js",
        &[("accept-encoding", "gzip, br")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), Some("br"));
    assert_eq!(header(&response, "content-type"), Some("text/javascript"));
    assert_eq!(body_string(response).await?, "brotli-bytes");

    // q=0 disables the encoding; the plain file comes back.
    let response = send(
        dist.runtime_config()?,
        "GET",
        "/assets/app-abc123.js",
        &[("accept-encoding", "br;q=0")],
    )
    .await?;
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(body_string(response).await?, "plain-js-bundle");

    // No compressed sibling: negotiation quietly serves the original.
    let response = send(
        dist.runtime_config()?,
        "GET",
        "/assets/app-abc123.css",
        &[("accept-encoding", "br")],
    )
    .await?;
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(body_string(response).await?, "body{}");
    Ok(())
}

#[tokio::test]
async fn if_modified_since_returns_304_without_body() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    let response = send(
        dist.runtime_config()?,
        "GET",
        "/assets/app-abc123.js",
        &[("if-modified-since", "Fri, 01 Jan 2100 00:00:00 GMT")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(body_string(response).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn traversal_and_encoded_traversal_attempts_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    for uri in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/assets/../../secret.txt",
        "/assets/..%2f..%2fsecret.txt",
        "/assets%2f..%2f..%2fsecret.txt",
        "/assets/..%5c..%5csecret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/assets/%00secret.txt",
        "/assets//secret.txt",
    ] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
        assert!(
            !body_string(response).await?.contains("TOP-SECRET"),
            "uri: {uri}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn symlinks_cannot_escape_the_dist_root() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    // File symlink pointing outside the dist.
    std::os::unix::fs::symlink(
        dist.root.path().join("secret.txt"),
        dist.dist().join("leak.txt"),
    )?;
    // Directory symlink pointing at the parent of the dist.
    std::os::unix::fs::symlink(dist.root.path(), dist.dist().join("escape"))?;
    for uri in ["/leak.txt", "/escape/secret.txt"] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
        assert!(
            !body_string(response).await?.contains("TOP-SECRET"),
            "uri: {uri}"
        );
    }
    // A symlink that stays inside the dist keeps working.
    std::os::unix::fs::symlink(
        dist.dist().join("robots.txt"),
        dist.dist().join("robots-alias.txt"),
    )?;
    let response = get(dist.runtime_config()?, "/robots-alias.txt").await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn directories_are_never_listed_or_served() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    for uri in ["/assets", "/assets/", "/locales/en-US"] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
    }
    Ok(())
}

#[tokio::test]
async fn upstream_forward_exclusions_hold() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    // Excluded prefixes (including the prefix-match quirk) 404 when no file
    // exists rather than serving the SPA index.
    for uri in [
        "/pipeline",
        "/jsx-tool",
        "/Login",
        "/static/whatever",
        "/a/b/c",
    ] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
        assert!(
            !body_string(response).await?.contains("SPA-INDEX-BODY"),
            "uri: {uri}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn non_get_methods_do_not_reach_the_spa_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    for method in ["POST", "PUT", "DELETE"] {
        let response = send(dist.runtime_config()?, method, "/compress-pdf", &[]).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "method: {method}");
        assert!(
            !body_string(response).await?.contains("SPA-INDEX-BODY"),
            "method: {method}"
        );
    }
    // HEAD is answered like GET (the transport strips bodies on the wire).
    let response = send(dist.runtime_config()?, "HEAD", "/", &[]).await?;
    assert_index_response(&response);
    Ok(())
}

#[tokio::test]
async fn missing_index_serves_the_fallback_page() -> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    fs::remove_file(dist.dist().join("index.html"))?;
    let response = get(dist.runtime_config()?, "/").await?;
    assert_index_response(&response);
    let body = body_string(response).await?;
    assert!(body.contains("Stirling PDF is running."));
    assert!(body.contains("window.RUSTLING_PDF_API_BASE_URL"));
    Ok(())
}

#[tokio::test]
async fn custom_files_static_index_overrides_the_dist_index()
-> Result<(), Box<dyn std::error::Error>> {
    let dist = SyntheticDist::new()?;
    let custom_static = dist.root.path().join("customFiles/static");
    fs::create_dir_all(&custom_static)?;
    fs::write(
        custom_static.join("index.html"),
        "<html><head></head><body>CUSTOM-INDEX</body></html>",
    )?;
    let response = get(dist.runtime_config()?, "/").await?;
    assert_index_response(&response);
    let body = body_string(response).await?;
    assert!(body.contains("CUSTOM-INDEX"));
    assert!(!body.contains("SPA-INDEX-BODY"));
    Ok(())
}

#[tokio::test]
async fn encoded_api_spellings_never_reach_the_spa_index() -> Result<(), Box<dyn std::error::Error>>
{
    let dist = SyntheticDist::new()?;
    // Percent-encoded spellings of /api bypass axum route matching but must
    // not fall through to the SPA index (decoded exclusion still applies).
    for uri in [
        "/%61pi/v1/config/app-config",
        "/%61pi/foo",
        "/api%2Fv1",
        "/%2e%2e",
        "/%c0%ae%c0%ae/etc/passwd",
        "/share/%2e%2e",
    ] {
        let response = get(dist.runtime_config()?, uri).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri: {uri}");
        assert!(
            !body_string(response).await?.contains("SPA-INDEX-BODY"),
            "uri: {uri}"
        );
    }
    // A percent-encoded spelling of a mapped SPA page still serves the index,
    // matching servlet decoding semantics upstream.
    let response = get(dist.runtime_config()?, "/index%2Ehtml").await?;
    assert_index_response(&response);
    assert!(body_string(response).await?.contains("SPA-INDEX-BODY"));
    Ok(())
}
