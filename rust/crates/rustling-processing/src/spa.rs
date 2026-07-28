//! Single-binary SPA serving.
//!
//! Rust port of the Java `ReactRoutingController` + `WebMvcConfig` static
//! resource pipeline. When `RUSTLING_FRONTEND_DIST` (or `system.frontendDist`
//! in settings) names a built Vite `dist/` directory, the binary serves the
//! React SPA itself: `/` and `/index.html` return the (transformed) index
//! page, client-route paths fall back to the index without shadowing `/api/**`
//! or real static assets, and the deep-link entry points (`/auth/callback`,
//! `/auth/callback/tauri`, `/share/{token}`, `/mobile-scanner`) behave like
//! their upstream controller mappings. When the setting is absent the module
//! is inert and unmatched requests keep returning axum's plain 404, so the
//! Vite dev-proxy workflow is untouched.
//!
//! See `rust/contracts/spa-serving.md` for the full behavior contract.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use percent_encoding::percent_decode_str;

use crate::runtime_config::RuntimeConfig;

/// Lightweight page served when the configured dist has no readable
/// `index.html` (upstream `buildFallbackHtml`, context path fixed to `/`).
const FALLBACK_INDEX_HTML: &str = include_str!("spa_pages/fallback_index.html");

/// Standalone desktop OAuth completion page (upstream `buildCallbackHtml`,
/// context path fixed to `/`). Served at `/auth/callback/tauri`.
const TAURI_AUTH_CALLBACK_HTML: &str = include_str!("spa_pages/tauri_auth_callback.html");

/// First path segments that never forward to the SPA index (the negative
/// lookahead alternations of upstream `forwardRootPaths`/`forwardNestedPaths`,
/// minus the dot-containing entries that the no-dot rule already blocks).
/// Upstream's `(?!api|…)` lookahead has prefix semantics — `jsx-anything`
/// starts with `js` and is therefore excluded too — so this list is matched
/// with `starts_with`, case-sensitively, exactly like the Java regex.
const EXCLUDED_FORWARD_PREFIXES: [&str; 18] = [
    "api",
    "static",
    "pipeline",
    "pdfjs",
    "pdfjs-legacy",
    "pdfium",
    "vendor",
    "fonts",
    "images",
    "css",
    "js",
    "assets",
    "locales",
    "modern-logo",
    "classic-logo",
    "Login",
    "og_images",
    "samples",
];

const CACHE_INDEX: &str = "no-cache, must-revalidate";
const CACHE_NO_STORE: &str = "no-store";
const CACHE_IMMUTABLE_ONE_YEAR: &str = "max-age=31536000, public, immutable";
const CACHE_DAILY_SWR: &str = "max-age=86400, public, stale-while-revalidate=604800";
const CACHE_NO_CACHE: &str = "no-cache";

/// Top-level files that must never be stored (service worker + PWA metadata).
const NO_STORE_ROOT_FILES: [&str; 4] = [
    "sw.js",
    "manifest.json",
    "site.webmanifest",
    "browserconfig.xml",
];

/// Directories whose contents are content-hashed or effectively immutable.
const IMMUTABLE_DIRS: [&str; 3] = ["assets", "images", "fonts"];

/// Stable non-fingerprinted asset directories (1 day + stale-while-revalidate).
const DAILY_SWR_DIRS: [&str; 13] = [
    "icons",
    "modern-logo",
    "classic-logo",
    "pdfjs",
    "pdfjs-legacy",
    "pdfium",
    "locales",
    "css",
    "js",
    "vendor",
    "samples",
    "og_images",
    "Login",
];

/// Stable non-fingerprinted top-level files (1 day + stale-while-revalidate).
const DAILY_SWR_ROOT_FILES: [&str; 4] = [
    "apple-touch-icon.png",
    "safari-pinned-tab.svg",
    "3rdPartyLicenses.json",
    "manifest-classic.json",
];

/// The config-gated SPA serving state, computed once at startup exactly like
/// upstream's `@PostConstruct` initialization: the index page (external
/// `customFiles/static/index.html` override first, then `dist/index.html`,
/// then the embedded fallback page) and the optional desktop mobile-upload
/// page are cached for the lifetime of the process.
pub(crate) struct SpaServing {
    dist_root: PathBuf,
    index_html: String,
    mobile_upload_html: Option<String>,
    tauri_mode: bool,
}

impl SpaServing {
    /// Builds the SPA serving state when a frontend dist directory is
    /// configured; returns `None` (module fully inert) otherwise.
    pub(crate) fn from_runtime_config(config: &RuntimeConfig) -> Option<Self> {
        let dist_root = config.frontend_dist_dir()?;
        let external_static = config.custom_static_dir();
        // Same switch the Java side reads as a system property.
        let tauri_mode = crate::env_compat::var("RUSTLING_PDF_TAURI_MODE")
            .is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"));
        Some(Self::new(dist_root, &external_static, tauri_mode))
    }

    pub(crate) fn new(dist_root: PathBuf, external_static: &Path, tauri_mode: bool) -> Self {
        let index_html = [
            external_static.join("index.html"),
            dist_root.join("index.html"),
        ]
        .iter()
        .find_map(|candidate| std::fs::read_to_string(candidate).ok())
        .map_or_else(
            || {
                tracing::warn!(
                    dist = %dist_root.display(),
                    "index.html not found in the configured frontend dist; \
                     serving the lightweight fallback page"
                );
                FALLBACK_INDEX_HTML.to_owned()
            },
            |raw| transform_index_html(&raw),
        );
        let mobile_upload_html = [
            external_static.join("mobile-upload.html"),
            dist_root.join("mobile-upload.html"),
        ]
        .iter()
        .find_map(|candidate| std::fs::read_to_string(candidate).ok());
        Self {
            dist_root,
            index_html,
            mobile_upload_html,
            tauri_mode,
        }
    }

    /// Fallback handler for every request no explicit route matched.
    pub(crate) async fn handle(&self, request: Request) -> Response {
        if request.method() != Method::GET && request.method() != Method::HEAD {
            return StatusCode::NOT_FOUND.into_response();
        }
        let path = request.uri().path();
        if path == "/" {
            return self.index_response();
        }
        let Some(segments) = sanitize_path(path) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        match classify(&segments) {
            SpaRoute::Index => self.index_response(),
            SpaRoute::TauriAuthCallback => html_response(TAURI_AUTH_CALLBACK_HTML, None),
            SpaRoute::MobileScanner => self.mobile_scanner_response(),
            SpaRoute::StaticFile => self
                .serve_static_file(&segments, request.headers())
                .await
                .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response()),
        }
    }

    fn index_response(&self) -> Response {
        html_response(&self.index_html, Some(CACHE_INDEX))
    }

    /// `/mobile-scanner`: the SPA page, except in desktop (Tauri) mode where a
    /// phone scanning the QR code gets the self-contained upload page instead
    /// (when the frontend bundle ships one).
    fn mobile_scanner_response(&self) -> Response {
        if self.tauri_mode
            && let Some(mobile_upload) = &self.mobile_upload_html
        {
            return html_response(mobile_upload, Some(CACHE_INDEX));
        }
        self.index_response()
    }

    /// Serves a real file from the dist root, with negotiated precompressed
    /// variants, upstream-parity cache headers, and `If-Modified-Since`
    /// revalidation. Returns `None` for anything that is not a regular file
    /// physically inside the dist root (missing files, directories, traversal
    /// and symlink escapes).
    async fn serve_static_file(
        &self,
        segments: &[String],
        headers: &HeaderMap,
    ) -> Option<Response> {
        // Defense in depth for the filesystem lookup: `sanitize_path` already
        // rejected separators and dot-segments, additionally refuse segments
        // with drive/stream separators (Windows deployments).
        if segments
            .iter()
            .any(|segment| segment.contains(':') || segment.starts_with('.'))
        {
            return None;
        }
        let mut candidate = self.dist_root.clone();
        for segment in segments {
            candidate.push(segment);
        }
        let dist_root = tokio::fs::canonicalize(&self.dist_root).await.ok()?;

        let mut chosen: Option<(PathBuf, Option<&'static str>)> = None;
        for (encoding, suffix) in [("br", ".br"), ("gzip", ".gz")] {
            if accepts_encoding(headers, encoding) {
                let mut compressed = candidate.clone().into_os_string();
                compressed.push(suffix);
                if let Some(real) = resolve_regular_file(&dist_root, Path::new(&compressed)).await {
                    chosen = Some((real, Some(encoding)));
                    break;
                }
            }
        }
        let (real_path, content_encoding) = match chosen {
            Some(found) => found,
            None => (resolve_regular_file(&dist_root, &candidate).await?, None),
        };

        let metadata = tokio::fs::metadata(&real_path).await.ok()?;
        let modified = metadata.modified().ok();
        let cache_control = static_cache_control(segments);
        let file_name = segments.last().map_or("", String::as_str);

        let mut builder = Response::builder()
            .header(header::CONTENT_TYPE, content_type_for(file_name))
            .header(header::CACHE_CONTROL, cache_control)
            .header(header::VARY, "Accept-Encoding");
        if let Some(modified) = modified
            && let Ok(value) = HeaderValue::from_str(&httpdate::fmt_http_date(modified))
        {
            builder = builder.header(header::LAST_MODIFIED, value);
        }
        if let Some(modified) = modified
            && not_modified_since(headers, modified)
        {
            return builder
                .status(StatusCode::NOT_MODIFIED)
                .body(Body::empty())
                .ok();
        }
        if let Some(encoding) = content_encoding {
            builder = builder.header(header::CONTENT_ENCODING, encoding);
        }
        let contents = tokio::fs::read(&real_path).await.ok()?;
        builder.body(Body::from(contents)).ok()
    }
}

/// Attaches the SPA fallback to a fully-merged router when serving is
/// configured; leaves the router untouched (axum's default 404 fallback)
/// otherwise.
pub(crate) fn attach_fallback(
    router: axum::Router,
    spa: Option<std::sync::Arc<SpaServing>>,
) -> axum::Router {
    match spa {
        Some(spa) => router.fallback(move |request: Request| {
            let spa = std::sync::Arc::clone(&spa);
            async move { spa.handle(request).await }
        }),
        None => router,
    }
}

enum SpaRoute {
    /// Serve the cached SPA index page.
    Index,
    /// Serve the standalone desktop OAuth completion page.
    TauriAuthCallback,
    /// Serve the mobile-scanner entry point (desktop-aware).
    MobileScanner,
    /// Look the path up in the dist directory.
    StaticFile,
}

/// Maps a sanitized request path onto the upstream controller semantics.
///
/// Explicit upstream `@GetMapping`s come first, then the two forward regexes:
/// a single dot-free segment not starting with an excluded token, or two
/// dot-free segments whose first segment is not excluded. Everything else is
/// a static-resource lookup. `/audit` intentionally lands on the generic
/// single-segment forward: the OSS upstream build has no audit web controller
/// and its effective behavior is the SPA index.
fn classify(segments: &[String]) -> SpaRoute {
    let names: Vec<&str> = segments.iter().map(String::as_str).collect();
    match names.as_slice() {
        [] | ["index.html"] | ["auth", "callback"] | ["share", _] => SpaRoute::Index,
        ["auth", "callback", "tauri"] => SpaRoute::TauriAuthCallback,
        ["mobile-scanner"] => SpaRoute::MobileScanner,
        [single] if !single.contains('.') && !is_excluded_forward(single) => SpaRoute::Index,
        [first, second]
            if !is_excluded_forward(first) && !first.contains('.') && !second.contains('.') =>
        {
            SpaRoute::Index
        }
        _ => SpaRoute::StaticFile,
    }
}

fn is_excluded_forward(segment: &str) -> bool {
    EXCLUDED_FORWARD_PREFIXES
        .iter()
        .any(|prefix| segment.starts_with(prefix))
}

/// Splits and percent-decodes a request path into segments, rejecting
/// anything that could address a file outside the dist root: dot-segments,
/// encoded separators, NUL bytes, empty segments (`//`, trailing `/`), and
/// non-UTF-8 escapes. Returns `None` when the path must be refused outright.
fn sanitize_path(path: &str) -> Option<Vec<String>> {
    let relative = path.strip_prefix('/')?;
    let mut segments = Vec::new();
    for raw_segment in relative.split('/') {
        let decoded = percent_decode_str(raw_segment).decode_utf8().ok()?;
        let segment: &str = &decoded;
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('/')
            || segment.contains('\\')
            || segment.contains('\0')
        {
            return None;
        }
        segments.push(segment.to_owned());
    }
    Some(segments)
}

/// Canonicalizes `candidate` and returns it only when it is a regular file
/// physically below the canonicalized dist root — this is what defeats both
/// `..` traversal remnants and symlinks pointing outside the dist.
async fn resolve_regular_file(canonical_root: &Path, candidate: &Path) -> Option<PathBuf> {
    let real = tokio::fs::canonicalize(candidate).await.ok()?;
    if !real.starts_with(canonical_root) {
        return None;
    }
    let metadata = tokio::fs::metadata(&real).await.ok()?;
    metadata.is_file().then_some(real)
}

/// Upstream `ReactRoutingController.processIndexHtml`: pin the base href to
/// the (fixed `/`) context path and expose it to the SPA before any bundle
/// executes.
fn transform_index_html(raw: &str) -> String {
    let mut html = raw.replace("%BASE_URL%", "/");
    html = rewrite_base_href(&html);
    html.replace(
        "</head>",
        "<script>window.RUSTLING_PDF_API_BASE_URL = '/';</script></head>",
    )
}

/// Replaces an existing `<base href="…">` tag (Vite may have baked one in)
/// with `<base href="/" />`, leaving the document untouched when no base tag
/// is present.
fn rewrite_base_href(html: &str) -> String {
    const OPENING: &str = "<base href=\"";
    let Some(start) = html.find(OPENING) else {
        return html.to_owned();
    };
    let after_opening = start + OPENING.len();
    let Some(quote_offset) = html[after_opening..].find('"') else {
        return html.to_owned();
    };
    let mut end = after_opening + quote_offset + 1;
    let bytes = html.as_bytes();
    while end < bytes.len() && (bytes[end] as char).is_ascii_whitespace() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'/' {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'>' {
        end += 1;
    } else {
        return html.to_owned();
    }
    format!("{}<base href=\"/\" />{}", &html[..start], &html[end..])
}

fn html_response(body: &str, cache_control: Option<&'static str>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8");
    if let Some(cache_control) = cache_control {
        builder = builder.header(header::CACHE_CONTROL, cache_control);
    }
    builder
        .body(Body::from(body.to_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// The `WebMvcConfig` resource-handler cache tiers, applied per path shape.
fn static_cache_control(segments: &[String]) -> &'static str {
    match segments {
        [name] => {
            if NO_STORE_ROOT_FILES.contains(&name.as_str()) {
                CACHE_NO_STORE
            } else if DAILY_SWR_ROOT_FILES.contains(&name.as_str())
                || name.starts_with("favicon.")
                || (has_png_extension(name)
                    && (name.starts_with("android-chrome-") || name.starts_with("mstile-")))
            {
                CACHE_DAILY_SWR
            } else {
                CACHE_NO_CACHE
            }
        }
        [first, ..] => {
            if IMMUTABLE_DIRS.contains(&first.as_str()) {
                CACHE_IMMUTABLE_ONE_YEAR
            } else if DAILY_SWR_DIRS.contains(&first.as_str()) {
                CACHE_DAILY_SWR
            } else {
                CACHE_NO_CACHE
            }
        }
        [] => CACHE_NO_CACHE,
    }
}

/// Branding icon check for the daily cache tier (`android-chrome-*.png`,
/// `mstile-*.png` in upstream `WebMvcConfig`).
fn has_png_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

/// Minimal `Accept-Encoding` negotiation: token match with `q=0` opt-out.
fn accepts_encoding(headers: &HeaderMap, wanted: &str) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|entry| {
            let mut parts = entry.split(';');
            let name = parts.next().map_or("", str::trim);
            if !name.eq_ignore_ascii_case(wanted) {
                return false;
            }
            for parameter in parts {
                if let Some(quality) = parameter.trim().strip_prefix("q=") {
                    return quality.trim().parse::<f32>().is_ok_and(|q| q > 0.0);
                }
            }
            true
        })
}

/// `If-Modified-Since` revalidation with second granularity.
fn not_modified_since(headers: &HeaderMap, modified: SystemTime) -> bool {
    let Some(since) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
    else {
        return false;
    };
    let Ok(age) = since.duration_since(SystemTime::UNIX_EPOCH) else {
        return false;
    };
    let Ok(modified_age) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
        return false;
    };
    modified_age.as_secs() <= age.as_secs()
}

/// Extension-based MIME resolution for the bounded set of file types a Vite
/// dist actually contains, with `application/octet-stream` as the default.
fn content_type_for(file_name: &str) -> &'static str {
    let extension = file_name
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension);
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "txt" | "properties" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "xml" => "application/xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CACHE_DAILY_SWR, CACHE_IMMUTABLE_ONE_YEAR, CACHE_NO_CACHE, CACHE_NO_STORE, SpaRoute,
        SpaServing, accepts_encoding, classify, content_type_for, rewrite_base_href, sanitize_path,
        static_cache_control, transform_index_html,
    };
    use axum::{
        body::{Body, to_bytes},
        extract::Request,
        http::{HeaderMap, HeaderValue, header},
    };

    fn segments(path: &str) -> Vec<String> {
        sanitize_path(path).unwrap_or_default()
    }

    #[test]
    fn sanitizes_traversal_and_separator_escapes() {
        assert!(sanitize_path("/../etc/passwd").is_none());
        assert!(sanitize_path("/assets/../secret.txt").is_none());
        assert!(sanitize_path("/assets/%2e%2e/secret.txt").is_none());
        assert!(sanitize_path("/assets/..%2fsecret.txt").is_none());
        assert!(sanitize_path("/assets%2f..%2fsecret.txt").is_none());
        assert!(sanitize_path("/assets/..%5csecret.txt").is_none());
        assert!(sanitize_path("/assets/%00.js").is_none());
        assert!(sanitize_path("/assets//app.js").is_none());
        assert!(sanitize_path("/assets/").is_none());
        assert!(sanitize_path("/a/./b").is_none());
        assert_eq!(
            sanitize_path("/assets/app%20name.js"),
            Some(vec!["assets".to_owned(), "app name.js".to_owned()])
        );
    }

    #[test]
    fn classifies_spa_and_static_paths_like_upstream() {
        assert!(matches!(
            classify(&segments("/index.html")),
            SpaRoute::Index
        ));
        assert!(matches!(
            classify(&segments("/auth/callback")),
            SpaRoute::Index
        ));
        assert!(matches!(
            classify(&segments("/auth/callback/tauri")),
            SpaRoute::TauriAuthCallback
        ));
        assert!(matches!(
            classify(&segments("/share/token.with.dots")),
            SpaRoute::Index
        ));
        assert!(matches!(
            classify(&segments("/mobile-scanner")),
            SpaRoute::MobileScanner
        ));
        assert!(matches!(classify(&segments("/audit")), SpaRoute::Index));
        assert!(matches!(
            classify(&segments("/compress-pdf")),
            SpaRoute::Index
        ));
        assert!(matches!(
            classify(&segments("/files/some-uuid")),
            SpaRoute::Index
        ));
        // Excluded prefixes never forward — including the prefix-match quirk.
        assert!(matches!(
            classify(&segments("/assets")),
            SpaRoute::StaticFile
        ));
        assert!(matches!(
            classify(&segments("/jsx-tool")),
            SpaRoute::StaticFile
        ));
        assert!(matches!(
            classify(&segments("/Login")),
            SpaRoute::StaticFile
        ));
        // Case-sensitive, exactly like the Java regex: lowercase login forwards.
        assert!(matches!(classify(&segments("/login")), SpaRoute::Index));
        // Dots and deep paths go to the static lookup.
        assert!(matches!(
            classify(&segments("/robots.txt")),
            SpaRoute::StaticFile
        ));
        assert!(matches!(
            classify(&segments("/tool/sub.page")),
            SpaRoute::StaticFile
        ));
        assert!(matches!(
            classify(&segments("/a/b/c")),
            SpaRoute::StaticFile
        ));
    }

    #[test]
    fn cache_tiers_mirror_web_mvc_config() {
        assert_eq!(static_cache_control(&segments("/sw.js")), CACHE_NO_STORE);
        assert_eq!(
            static_cache_control(&segments("/manifest.json")),
            CACHE_NO_STORE
        );
        assert_eq!(
            static_cache_control(&segments("/assets/app-abc123.js")),
            CACHE_IMMUTABLE_ONE_YEAR
        );
        assert_eq!(
            static_cache_control(&segments("/fonts/inter.woff2")),
            CACHE_IMMUTABLE_ONE_YEAR
        );
        assert_eq!(
            static_cache_control(&segments("/locales/en-US/translation.json")),
            CACHE_DAILY_SWR
        );
        assert_eq!(
            static_cache_control(&segments("/favicon.ico")),
            CACHE_DAILY_SWR
        );
        assert_eq!(
            static_cache_control(&segments("/android-chrome-192x192.png")),
            CACHE_DAILY_SWR
        );
        assert_eq!(
            static_cache_control(&segments("/robots.txt")),
            CACHE_NO_CACHE
        );
    }

    #[test]
    fn transforms_index_html_like_upstream() {
        let raw =
            "<html><head><base href=\"%BASE_URL%\" /><title>x</title></head><body></body></html>";
        let transformed = transform_index_html(raw);
        assert!(transformed.contains("<base href=\"/\" />"));
        assert!(!transformed.contains("%BASE_URL%"));
        assert!(
            transformed.contains("<script>window.RUSTLING_PDF_API_BASE_URL = '/';</script></head>")
        );
        // A Vite-baked relative base is rewritten too.
        let baked = rewrite_base_href("<base href=\"./\">rest");
        assert_eq!(baked, "<base href=\"/\" />rest");
        // No base tag: untouched.
        assert_eq!(rewrite_base_href("<head></head>"), "<head></head>");
    }

    #[test]
    fn negotiates_accept_encoding_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("gzip, br"),
        );
        assert!(accepts_encoding(&headers, "br"));
        assert!(accepts_encoding(&headers, "gzip"));
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("gzip;q=0, br;q=0.8"),
        );
        assert!(!accepts_encoding(&headers, "gzip"));
        assert!(accepts_encoding(&headers, "br"));
        headers.remove(header::ACCEPT_ENCODING);
        assert!(!accepts_encoding(&headers, "br"));
    }

    async fn page_body(spa: &SpaServing, uri: &str) -> Result<String, Box<dyn std::error::Error>> {
        let request = Request::builder().uri(uri).body(Body::empty())?;
        let response = spa.handle(request).await;
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    #[tokio::test]
    async fn mobile_scanner_prefers_the_upload_page_in_tauri_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let dist = directory.path().join("dist");
        std::fs::create_dir_all(&dist)?;
        std::fs::write(dist.join("index.html"), "<html><head></head>INDEX</html>")?;
        std::fs::write(dist.join("mobile-upload.html"), "MOBILE-UPLOAD-PAGE")?;
        let external = directory.path().join("customFiles/static");

        let desktop = SpaServing::new(dist.clone(), &external, true);
        assert!(
            page_body(&desktop, "/mobile-scanner")
                .await?
                .contains("MOBILE-UPLOAD-PAGE")
        );

        // Outside Tauri mode the SPA page wins even when the upload page exists.
        let browser = SpaServing::new(dist, &external, false);
        assert!(
            page_body(&browser, "/mobile-scanner")
                .await?
                .contains("INDEX")
        );
        Ok(())
    }

    #[tokio::test]
    async fn tauri_mode_without_upload_page_still_serves_the_index()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let dist = directory.path().join("dist");
        std::fs::create_dir_all(&dist)?;
        std::fs::write(dist.join("index.html"), "<html><head></head>INDEX</html>")?;
        let external = directory.path().join("customFiles/static");
        let desktop = SpaServing::new(dist, &external, true);
        assert!(
            page_body(&desktop, "/mobile-scanner")
                .await?
                .contains("INDEX")
        );
        Ok(())
    }

    #[test]
    fn resolves_content_types_for_dist_files() {
        assert_eq!(content_type_for("app-abc.js"), "text/javascript");
        assert_eq!(content_type_for("style.css"), "text/css");
        assert_eq!(content_type_for("pdfium.wasm"), "application/wasm");
        assert_eq!(content_type_for("logo192.PNG"), "image/png");
        assert_eq!(
            content_type_for("eng.traineddata"),
            "application/octet-stream"
        );
        assert_eq!(content_type_for("noextension"), "application/octet-stream");
    }
}
