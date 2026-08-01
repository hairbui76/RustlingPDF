//! Cross-origin policy over the *real* assembled router.
//!
//! `src/cors.rs` unit-tests the layer against a stand-in route; these tests
//! check the production wiring: that the policy is off by default, and that
//! when it is on it survives the transport guardrails and the SPA fallback and
//! answers a preflight on a route registered for `POST` only.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use rustling_processing::{app, cors::apply_desktop_cors};
use tower::ServiceExt;

const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

/// The exact route and headers the shipped desktop build failed on.
const PREFLIGHT_URI: &str = "/api/v1/convert/file/pdf";
const TAURI_ORIGIN: &str = "http://tauri.localhost";

fn preflight(origin: &str) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    Ok(Request::builder()
        .method("OPTIONS")
        .uri(PREFLIGHT_URI)
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-browser-id")
        .body(Body::empty())?)
}

fn header_text(response: &Response, name: header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Desktop mode: the preflight that used to 405 now succeeds, names the Tauri
/// origin, and permits the SPA's unconditional `X-Browser-Id` header.
#[tokio::test]
async fn desktop_router_answers_the_preflight_that_broke_the_shipped_app()
-> Result<(), Box<dyn std::error::Error>> {
    let router: Router = apply_desktop_cors(app(MAX_UPLOAD_BYTES), true);
    let response = router.oneshot(preflight(TAURI_ORIGIN)?).await?;

    assert!(
        response.status().is_success(),
        "preflight status: {}",
        response.status()
    );
    assert_eq!(
        header_text(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some(TAURI_ORIGIN)
    );
    let allowed = header_text(&response, header::ACCESS_CONTROL_ALLOW_HEADERS)
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(allowed.contains("x-browser-id"), "allow-headers: {allowed}");
    assert!(allowed.contains("content-type"), "allow-headers: {allowed}");
    Ok(())
}

/// The security boundary on the real router: an unrelated page gets no
/// `Access-Control-Allow-Origin`, so the browser withholds the response even
/// though the service itself authenticates nobody.
#[tokio::test]
async fn desktop_router_refuses_a_foreign_origin() -> Result<(), Box<dyn std::error::Error>> {
    for origin in [
        "https://evil.example",
        "http://tauri.localhost.evil.example",
        "http://localhost:5173",
        "null",
    ] {
        let router: Router = apply_desktop_cors(app(MAX_UPLOAD_BYTES), true);
        let response = router.oneshot(preflight(origin)?).await?;
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            None,
            "{origin} must not be granted an allow-origin"
        );
    }
    Ok(())
}

/// A real cross-origin GET carries the whole `expose-headers` list, so the SPA
/// can read the download filename and the lossy-conversion warnings.
#[tokio::test]
async fn desktop_router_exposes_the_headers_the_spa_reads() -> Result<(), Box<dyn std::error::Error>>
{
    let router: Router = apply_desktop_cors(app(MAX_UPLOAD_BYTES), true);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/info/status")
                .header(header::ORIGIN, TAURI_ORIGIN)
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_text(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some(TAURI_ORIGIN)
    );
    let exposed = header_text(&response, header::ACCESS_CONTROL_EXPOSE_HEADERS)
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
    Ok(())
}

/// The production default. `app` reads `RUSTLING_PDF_TAURI_MODE` from the
/// environment, which is unset for a server deployment (and for this test
/// process), so the assembled router must be byte-for-byte the pre-CORS
/// service: preflights still 405 and no `Access-Control-*` header is ever
/// emitted. This is what proves the wiring in `finish_router` is gated rather
/// than always-on.
#[tokio::test]
async fn server_router_emits_no_cors_headers() -> Result<(), Box<dyn std::error::Error>> {
    let response = app(MAX_UPLOAD_BYTES)
        .oneshot(preflight(TAURI_ORIGIN)?)
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    let response = app(MAX_UPLOAD_BYTES)
        .oneshot(
            Request::builder()
                .uri("/api/v1/info/status")
                .header(header::ORIGIN, TAURI_ORIGIN)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    for name in [
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
    ] {
        assert_eq!(
            response.headers().get(&name),
            None,
            "{name} must be absent for a server deployment"
        );
    }
    Ok(())
}
