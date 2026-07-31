use std::fs;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use rustling_processing::{
    TimestampSettings, app_with_runtime_config, runtime_config::RuntimeConfig,
};
use tempfile::{TempDir, tempdir};
use tower::ServiceExt as _;

const LABELS_PATH: &str = "/api/v1/classification/labels";
const CLASSIFY_PATH: &str = "/api/v1/ai/tools/classify-and-label";

#[tokio::test]
async fn label_crud_routes_are_gone_and_classification_without_labels_passes_through()
-> Result<(), Box<dyn std::error::Error>> {
    let (_directory, app) = open_app("aiEngine:\n  enabled: false\n")?;
    // The server-side label vocabulary was removed with server state: the CRUD
    // routes must not exist in any configuration.
    for request in [
        Request::get(LABELS_PATH).body(Body::empty())?,
        Request::put(LABELS_PATH)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"labels":[]}"#))?,
        Request::delete(LABELS_PATH).body(Body::empty())?,
    ] {
        let response = app.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // Without a client-supplied vocabulary there is nothing to classify
    // against; the PDF passes through unchanged.
    let source = b"%PDF-1.4\npass-through fixture\n%%EOF\n";
    let response = post_pdf(&app, source, None).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        &to_bytes(response.into_body(), usize::MAX).await?[..],
        source
    );
    Ok(())
}

#[tokio::test]
async fn client_supplied_labels_validate_and_require_the_engine()
-> Result<(), Box<dyn std::error::Error>> {
    let (_directory, app) = open_app("aiEngine:\n  enabled: false\n")?;

    // Malformed vocabularies are rejected before any engine call.
    for labels in ["{}", r#"{"labels":null}"#, "not-json"] {
        let invalid = post_pdf(&app, b"%PDF-1.4\n%%EOF\n", Some(labels)).await?;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST, "{labels}");
    }
    let duplicate = r#"{"labels":[{"id":"a","name":"Invoice"},{"id":"a","name":"Contract"}]}"#;
    let invalid = post_pdf(&app, b"%PDF-1.4\n%%EOF\n", Some(duplicate)).await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    // An empty vocabulary means nothing to classify: pass-through.
    let source = b"%PDF-1.4\nempty labels\n%%EOF\n";
    let empty = post_pdf(&app, source, Some(r#"{"labels":[]}"#)).await?;
    assert_eq!(empty.status(), StatusCode::OK);
    assert_eq!(&to_bytes(empty.into_body(), usize::MAX).await?[..], source);

    // A real vocabulary requires the AI engine, which is disabled here.
    let vocabulary = r#"{"labels":[{"id":"invoice","name":"Invoice","icon":"receipt-long"}]}"#;
    let disabled_engine = post_pdf(&app, b"%PDF-1.4\n%%EOF\n", Some(vocabulary)).await?;
    assert_eq!(disabled_engine.status(), StatusCode::SERVICE_UNAVAILABLE);
    // The bare-array spelling is accepted too.
    let bare = r#"[{"id":"invoice","name":"Invoice"}]"#;
    let disabled_engine = post_pdf(&app, b"%PDF-1.4\n%%EOF\n", Some(bare)).await?;
    assert_eq!(disabled_engine.status(), StatusCode::SERVICE_UNAVAILABLE);
    Ok(())
}

fn open_app(settings_yaml: &str) -> Result<(TempDir, Router), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let config_directory = directory.path().join("configs");
    fs::create_dir_all(&config_directory)?;
    let settings = config_directory.join("settings.yml");
    fs::write(&settings, settings_yaml)?;
    let config = RuntimeConfig::from_files(settings, config_directory.join("missing.yml"));
    Ok((
        directory,
        app_with_runtime_config(1024 * 1024, TimestampSettings::default(), config),
    ))
}

async fn post_pdf(
    app: &Router,
    source: &[u8],
    labels: Option<&str>,
) -> Result<Response, Box<dyn std::error::Error>> {
    let boundary = "rustling-classification-boundary";
    let mut body = Vec::new();
    if let Some(labels) = labels {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"labels\"\r\n\r\n{labels}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"fileInput\"; filename=\"source.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(source);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let request = Request::post(CLASSIFY_PATH)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))?;
    Ok(app.clone().oneshot(request).await?)
}
