//! Desktop-only cross-origin policy for the processing service.
//!
//! # Why this exists
//!
//! The Tauri desktop shell loads its UI from the webview's own custom protocol
//! (`tauri://localhost`, or `http(s)://tauri.localhost` on Windows and
//! Android) and then calls the sidecar at an absolute
//! `http://127.0.0.1:<ephemeral-port>` base URL. That is genuinely
//! cross-origin, so every request the SPA makes is subject to CORS. The SPA
//! also sends a custom `X-Browser-Id` request header (see
//! [`crate::runtime_metrics`]), which forces a preflight on *every* call. With
//! no CORS layer the preflight `OPTIONS` hits routes registered as `post(..)`
//! only, is rejected with `405`, carries no `Access-Control-Allow-Origin`, and
//! the desktop UI reports a bare "Network error" for every operation.
//!
//! # Why the policy is narrow, and must stay narrow
//!
//! The service has **no authentication of any kind** and listens on loopback.
//! A permissive policy ([`tower_http::cors::CorsLayer::permissive`], or any
//! blanket allowance of `http://localhost:*` / `http://127.0.0.1:*`) would let
//! *any* web page the user happens to visit issue cross-origin requests to
//! their local service **and read the responses** — that is, read, convert and
//! exfiltrate their local documents. Today that is impossible precisely
//! because no CORS headers are ever sent, and browsers therefore withhold the
//! response. Restoring desktop function must not trade that away.
//!
//! Consequently:
//!
//! - only the origins a Tauri v2 webview can present **on the platform this
//!   binary is built for** are allowed, as an exact byte-for-byte list — no
//!   wildcard, no subdomain matching, no suffix matching (see
//!   [`allowed_origins`], which is why the list is platform-gated);
//! - `Origin: null` is *not* allowed: sandboxed iframes and `data:` documents
//!   present it, and an opaque origin cannot be authenticated;
//! - credentials are not allowed (nothing in the service reads a cookie or an
//!   `Authorization` header), which also keeps the browser's stricter
//!   credentialed-CORS rules out of play; and
//! - the layer is installed **only** in desktop (Tauri) mode. A self-hosted
//!   web or Docker deployment serves the SPA same-origin from the same binary
//!   ([`crate::spa`]) and needs no CORS at all, so it must not gain one.
//!
//! # Deliberately absent: the desktop dev-server origin
//!
//! `task desktop:dev` runs the webview against the Vite dev server at
//! `http://localhost:5173` (`build.devUrl` in `tauri.conf.json`) while still
//! talking to the sidecar over an absolute loopback URL, so that workflow is
//! *not* fixed by this layer. Allowing `http://localhost:5173` was rejected:
//! `task desktop:stage-sidecar` stages the **release** binary even for
//! `tauri dev`, so no compile-time gate (`debug_assertions` and friends) can
//! keep such an exception out of a packaged build — it would ship to users,
//! and port 5173 is the default for every Vite project on the machine. Web
//! development (`task dev`) is unaffected: Vite proxies `/api` same-origin.

use axum::{
    Router,
    http::{HeaderName, HeaderValue, Method, header},
};
use tower_http::cors::{AllowPrivateNetwork, CorsLayer};

/// How long a browser may cache a preflight result.
///
/// Every SPA request carries `X-Browser-Id` and therefore preflights; without
/// caching, the desktop would pay two round trips per operation.
const PREFLIGHT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(600);

/// The exact origins a Tauri v2 webview presents **on the platform this binary
/// is built for**.
///
/// Verified against the pinned `tauri` crate (2.11.5, per
/// `src-tauri/Cargo.lock`) rather than assumed. `Manager::tauri_protocol_url`
/// picks the origin at compile time with exactly this condition:
///
/// ```text
/// if cfg!(windows) || cfg!(target_os = "android") { "{scheme}://tauri.localhost" }
/// else                                            { "tauri://localhost" }
/// ```
///
/// The `tauri.localhost` forms are not a stylistic variant: they are `wry`'s
/// workaround for `WebView2` and Android being unable to navigate a non-standard
/// scheme at all (see `wry::custom_protocol_workaround`), where `tauri://…` is
/// rewritten to `http(s)://tauri.localhost/…`. `WebKitGTK` and `WKWebView`
/// handle the real `tauri://` scheme natively and never present the workaround
/// origins.
///
/// So this list is gated the same way. The sidecar is built per platform —
/// `stage-sidecar.sh` stages the host triple from `rustc -vV`, and the desktop
/// release workflow runs a per-OS matrix — so a Linux or macOS build must not
/// carry the Windows origins. That matters: `.localhost` resolves to loopback
/// in Chrome and Firefox, so `http://tauri.localhost` is really `127.0.0.1:80`.
/// Any local process already serving script-capable HTML on port 80 (a
/// `docker -p 80:80`, an nginx dev stack, a reflected XSS in a local app) would
/// otherwise be an allowed origin able to read this service's responses.
///
/// `https` appears alongside `http` because the window can be configured for
/// the HTTPS scheme; both are the same Windows-only workaround host.
#[must_use]
pub fn allowed_origins() -> &'static [&'static str] {
    if cfg!(windows) || cfg!(target_os = "android") {
        &["http://tauri.localhost", "https://tauri.localhost"]
    } else {
        // macOS, Linux, iOS.
        &["tauri://localhost"]
    }
}

/// Request headers the SPA actually sends that are not CORS-safelisted.
///
/// `content-type` covers both `application/json` and the multipart uploads the
/// tool endpoints take. `x-browser-id` is set unconditionally by
/// `apiClientSetup.ts` and consumed by [`crate::runtime_metrics`] for the
/// in-memory active-browser count behind `/api/v1/info/load/*`; dropping it
/// would silently zero that metric. `accept` is safelisted for the values
/// axios sends but is listed explicitly so the allow-list documents the whole
/// request shape.
///
/// This and its two siblings below are functions rather than `const`s because
/// `HeaderName` carries an atomic internally, so a `const` of one trips
/// `clippy::declare_interior_mutable_const`.
fn allowed_request_headers() -> [HeaderName; 3] {
    [
        header::ACCEPT,
        header::CONTENT_TYPE,
        HeaderName::from_static("x-browser-id"),
    ]
}

/// Response headers cross-origin JavaScript must be able to read.
///
/// A browser exposes almost nothing to cross-origin script by default: only
/// the CORS-safelisted response headers (`cache-control`, `content-language`,
/// `content-length`, `content-type`, `expires`, `last-modified`, `pragma`).
/// Everything the SPA or a future UI needs has to be named here, and a missing
/// entry fails *silently* — the header arrives on the wire and the browser
/// hides it.
///
/// # Before adding an endpoint, note `x-job-id`
///
/// `x-job-id` is deliberately absent, but for a narrower reason than it looks.
/// The service sets it in exactly one place — the
/// `/api/v1/pdf-text-editor/metadata` response — and there it is
/// **header-only**: the JSON body does not repeat it. Leaving it unexposed is
/// safe *only* because the SPA never calls that endpoint; the flow it does
/// use, `pdf-text-editor?async=true`, returns `jobId` inside the JSON body,
/// which needs no exposing at all.
///
/// So if that metadata endpoint is ever wired into the UI, `x-job-id` has to be
/// added here. Otherwise it works same-origin in a browser and fails **only on
/// desktop**, silently — header on the wire, invisible to script.
fn exposed_response_headers() -> [HeaderName; 7] {
    [
        // Read today by `toolResponseProcessor.ts`, `fileResponseUtils.ts` and
        // the PDF text editor to recover the backend's download filename.
        // Without it every `preserveBackendFilename` tool quietly falls back to
        // a generated name.
        header::CONTENT_DISPOSITION,
        // The Office conversion pipeline's only channel for reporting a lossy
        // conversion (dropped slides and similar): the body is an opaque PDF,
        // so these headers are the signal — see
        // `rust/contracts/file-to-pdf.md`. Not exposing them would ship a
        // silent-data-loss warning that silently never arrives.
        HeaderName::from_static("x-rustling-conversion-engine"),
        HeaderName::from_static("x-rustling-conversion-degraded"),
        HeaderName::from_static("x-rustling-conversion-warnings"),
        HeaderName::from_static("x-rustling-conversion-warning-detail"),
        // Structured out-of-band report from the PDF comment agent, for the
        // same reason: the response body is the PDF.
        HeaderName::from_static("x-rustling-tool-report"),
        // Sent with the job queue's 503 so a client can back off correctly.
        header::RETRY_AFTER,
    ]
}

/// Methods the routed API actually serves. The router registers only `get`,
/// `post` and `delete`; `HEAD` is answered implicitly for every `GET` route.
/// `OPTIONS` is handled by the layer itself and needs no entry.
fn allowed_methods() -> [Method; 4] {
    [Method::GET, Method::HEAD, Method::POST, Method::DELETE]
}

/// Byte-exact membership test against [`allowed_origins`], mirroring what
/// `AllowOrigin::List` does internally so the private-network predicate can
/// never be laxer than the origin allow-list itself.
fn is_allowed_origin(origin: &HeaderValue) -> bool {
    allowed_origins()
        .iter()
        .any(|allowed| origin.as_bytes() == allowed.as_bytes())
}

/// Builds the desktop CORS layer, or `None` outside desktop mode.
///
/// `tauri_mode` is taken as a parameter rather than read here so both branches
/// are testable without mutating process environment; production callers pass
/// [`crate::environment::tauri_mode_active`].
#[must_use]
pub fn desktop_cors_layer(tauri_mode: bool) -> Option<CorsLayer> {
    if !tauri_mode {
        return None;
    }
    let origins: Vec<HeaderValue> = allowed_origins()
        .iter()
        // Every entry is a compile-time constant of visible ASCII, so this
        // cannot fail; `filter_map` keeps the builder infallible rather than
        // panicking a released desktop build.
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();
    debug_assert_eq!(
        origins.len(),
        allowed_origins().len(),
        "every allowed origin must be a valid header value"
    );
    Some(
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(allowed_methods().to_vec())
            .allow_headers(allowed_request_headers().to_vec())
            .expose_headers(exposed_response_headers().to_vec())
            // Explicit, not merely defaulted: the service has no cookies or
            // sessions, and allowing credentials would additionally forbid the
            // wildcard forms and widen what a hostile page could do.
            .allow_credentials(false)
            // Chromium's Private Network Access check applies to a document
            // reaching a more-private address space, which is the desktop shape
            // (WebView2 document → `127.0.0.1` sidecar).
            //
            // Scoped by predicate, not `true`: tower-http's `Yes` branch
            // returns the header for *any* requester that asks, including one
            // whose origin was rejected and one sending no `Origin` at all.
            // That stays spec-inert — a PNA preflight still needs a matching
            // `Access-Control-Allow-Origin` — but it advertises the service to
            // callers this policy just refused, so the predicate re-checks the
            // allow-list and keeps the wire honest.
            .allow_private_network(AllowPrivateNetwork::predicate(|origin, _parts| {
                is_allowed_origin(origin)
            }))
            .max_age(PREFLIGHT_MAX_AGE),
    )
}

/// Installs [`desktop_cors_layer`] on a fully assembled router.
///
/// Applied as the outermost layer at the router assembly boundary so it wraps
/// every route *and* both fallbacks. That placement is what makes a preflight
/// work at all: `axum`'s `Router::layer` wraps each route's `MethodRouter`, so
/// the layer answers `OPTIONS` with `200` before method routing can reject it
/// with `405`.
pub fn apply_desktop_cors(router: Router, tauri_mode: bool) -> Router {
    match desktop_cors_layer(tauri_mode) {
        Some(layer) => router.layer(layer),
        None => router,
    }
}

#[cfg(test)]
mod tests {
    use super::{allowed_origins, apply_desktop_cors};
    use axum::{
        Router,
        body::Body,
        http::{HeaderName, HeaderValue, Request, StatusCode, header},
        response::{IntoResponse, Response},
        routing::post,
    };
    use tower::ServiceExt as _;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// A stand-in for a real file-returning tool route: registered for `POST`
    /// only (so an unlayered `OPTIONS` would 405, exactly as the shipped
    /// service did) and answering with the custom headers the SPA must read.
    fn test_router(tauri_mode: bool) -> Router {
        apply_desktop_cors(
            Router::new().route("/api/v1/convert/file/pdf", post(conversion_stub)),
            tauri_mode,
        )
    }

    async fn conversion_stub() -> impl IntoResponse {
        (
            StatusCode::OK,
            [
                (header::CONTENT_DISPOSITION, "attachment; filename=out.pdf"),
                (
                    HeaderName::from_static("x-rustling-conversion-engine"),
                    "builtin",
                ),
                (
                    HeaderName::from_static("x-rustling-conversion-degraded"),
                    "true",
                ),
            ],
            "%PDF-1.7",
        )
    }

    fn preflight(origin: &str) -> Result<Request<Body>, Box<dyn std::error::Error>> {
        Ok(Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/convert/file/pdf")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-browser-id")
            .body(Body::empty())?)
    }

    fn header_value(response: &Response, name: header::HeaderName) -> Option<String> {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    /// The exact request the desktop failed on: a preflight from each Tauri
    /// origin must succeed and echo back the origin plus `x-browser-id`.
    #[tokio::test]
    async fn allows_preflight_from_every_tauri_origin() -> TestResult {
        for &origin in allowed_origins() {
            let response = test_router(true).oneshot(preflight(origin)?).await?;
            assert!(
                response.status().is_success(),
                "{origin} preflight status: {}",
                response.status()
            );
            assert_eq!(
                header_value(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
                Some(origin)
            );
            let allowed_headers =
                header_value(&response, header::ACCESS_CONTROL_ALLOW_HEADERS).unwrap_or_default();
            assert!(
                allowed_headers
                    .to_ascii_lowercase()
                    .contains("x-browser-id"),
                "{origin} allow-headers: {allowed_headers}"
            );
            assert!(
                allowed_headers
                    .to_ascii_lowercase()
                    .contains("content-type"),
                "{origin} allow-headers: {allowed_headers}"
            );
            let allowed_methods =
                header_value(&response, header::ACCESS_CONTROL_ALLOW_METHODS).unwrap_or_default();
            assert!(
                allowed_methods.to_ascii_uppercase().contains("POST"),
                "{origin} allow-methods: {allowed_methods}"
            );
        }
        Ok(())
    }

    /// The security boundary: an unrelated page the user happens to visit gets
    /// no `Access-Control-Allow-Origin`, so the browser withholds the response
    /// even though the service itself has no authentication.
    #[tokio::test]
    async fn refuses_preflight_from_a_foreign_origin() -> TestResult {
        for origin in [
            "https://evil.example",
            // Near-misses that a sloppy substring or suffix match would let
            // through.
            "http://tauri.localhost.evil.example",
            "http://localhost:5173",
            "http://127.0.0.1:8081",
            "null",
        ] {
            let response = test_router(true).oneshot(preflight(origin)?).await?;
            assert_eq!(
                header_value(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
                None,
                "{origin} must not be granted an allow-origin"
            );
        }
        Ok(())
    }

    /// A real cross-origin response must carry the full `expose-headers` list,
    /// otherwise the browser hides the download filename and the lossy-
    /// conversion warnings from the SPA.
    #[tokio::test]
    async fn exposes_every_header_the_spa_reads() -> TestResult {
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/convert/file/pdf")
            .header(header::ORIGIN, allowed_origins()[0])
            .body(Body::empty())?;
        let response = test_router(true).oneshot(request).await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            header_value(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
            Some(allowed_origins()[0])
        );
        let exposed = header_value(&response, header::ACCESS_CONTROL_EXPOSE_HEADERS)
            .unwrap_or_default()
            .to_ascii_lowercase();
        for expected in [
            "content-disposition",
            "x-rustling-conversion-engine",
            "x-rustling-conversion-degraded",
            "x-rustling-conversion-warnings",
            "x-rustling-conversion-warning-detail",
            "x-rustling-tool-report",
            "retry-after",
        ] {
            assert!(
                exposed.contains(expected),
                "expose-headers missing {expected}"
            );
        }
        // Credentialed CORS is not requested, so the browser's stricter rules
        // never apply.
        assert_eq!(
            header_value(&response, header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            None
        );
        Ok(())
    }

    /// Outside desktop mode the service behaves exactly as it did before this
    /// layer existed: no CORS headers at all, and `OPTIONS` on a `post`-only
    /// route still 405s. A self-hosted deployment therefore gains nothing a
    /// hostile page could use.
    #[tokio::test]
    async fn emits_no_cors_headers_outside_desktop_mode() -> TestResult {
        let response = test_router(false)
            .oneshot(preflight(allowed_origins()[0])?)
            .await?;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        for name in [
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            header::ACCESS_CONTROL_ALLOW_METHODS,
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
        ] {
            assert_eq!(
                response.headers().get(&name),
                None,
                "{name} must be absent outside desktop mode"
            );
        }

        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/convert/file/pdf")
            .header(header::ORIGIN, allowed_origins()[0])
            .body(Body::empty())?;
        let response = test_router(false).oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            None
        );
        Ok(())
    }

    /// Every listed origin is a syntactically valid header value, so the
    /// builder never silently drops one.
    #[test]
    fn every_allowed_origin_is_a_valid_header_value() {
        for &origin in allowed_origins() {
            assert!(HeaderValue::from_str(origin).is_ok(), "{origin}");
        }
    }

    /// The platform gate. Tauri picks the webview origin at compile time, and
    /// the sidecar is built per platform, so a build must allow only the
    /// origins its *own* webview can present. On Linux and macOS the Windows
    /// `tauri.localhost` workaround origins must be refused: `.localhost`
    /// resolves to loopback, so allowing them would make any local process
    /// serving HTML on port 80 an allowed origin.
    #[tokio::test]
    async fn allow_list_is_scoped_to_this_platforms_webview() -> TestResult {
        let (expected, foreign): (&[&str], &[&str]) =
            if cfg!(windows) || cfg!(target_os = "android") {
                (
                    &["http://tauri.localhost", "https://tauri.localhost"],
                    &["tauri://localhost"],
                )
            } else {
                (
                    &["tauri://localhost"],
                    &["http://tauri.localhost", "https://tauri.localhost"],
                )
            };
        assert_eq!(allowed_origins(), expected);

        for &origin in foreign {
            let response = test_router(true).oneshot(preflight(origin)?).await?;
            assert_eq!(
                header_value(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
                None,
                "{origin} belongs to another platform and must be refused"
            );
        }
        Ok(())
    }

    /// `access-control-allow-private-network` must track the origin allow-list.
    /// `tower-http`'s unconditional `Yes` branch answers any requester that
    /// asks — including one whose origin was just rejected, and one sending no
    /// `Origin` at all — so the policy uses an origin-scoped predicate instead.
    #[tokio::test]
    async fn private_network_header_tracks_the_origin_allow_list() -> TestResult {
        let allow_private_network = HeaderName::from_static("access-control-allow-private-network");

        let allowed = allowed_origins()[0];
        let response = test_router(true)
            .oneshot(private_network_preflight(Some(allowed))?)
            .await?;
        assert_eq!(
            header_value(&response, allow_private_network.clone()).as_deref(),
            Some("true"),
            "an allowed origin that asks must be granted private-network access"
        );

        for origin in [Some("https://evil.example"), None] {
            let response = test_router(true)
                .oneshot(private_network_preflight(origin)?)
                .await?;
            assert_eq!(
                header_value(&response, allow_private_network.clone()),
                None,
                "{origin:?} must not be told the private network is reachable"
            );
        }
        Ok(())
    }

    /// A preflight that also asks Chromium's Private Network Access question,
    /// optionally without an `Origin` at all.
    fn private_network_preflight(
        origin: Option<&str>,
    ) -> Result<Request<Body>, Box<dyn std::error::Error>> {
        let mut builder = Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/convert/file/pdf")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header("access-control-request-private-network", "true");
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        Ok(builder.body(Body::empty())?)
    }
}
