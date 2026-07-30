use std::fs;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use rustling_processing::{
    TimestampSettings, app_with_runtime_config, runtime_config::RuntimeConfig,
};
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn app_config_loads_base_and_custom_settings() -> Result<(), Box<dyn std::error::Error>> {
    let runtime_config = configured_runtime()?;
    let response = request(
        runtime_config,
        "/api/v1/config/app-config",
        &[("host", "pdf.example.test"), ("x-forwarded-proto", "https")],
    )
    .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let config = json_body(response).await?;
    assert_eq!(config["defaultLocale"], "vi-VN");
    assert_eq!(config["logoStyle"], "classic");
    assert_eq!(config["frontendUrl"], "https://pdf.example.test");
    assert_eq!(config["enableLogin"], false);
    assert_eq!(config["timestampTsaPresets"][0]["label"], "DigiCert");
    Ok(())
}

#[tokio::test]
async fn endpoint_configuration_routes_report_explicit_disabled_endpoints()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime_config = configured_runtime()?;
    let availability = json_body(
        request(
            runtime_config.clone(),
            "/api/v1/config/endpoints-availability?endpoints=compress-pdf,merge-pdfs",
            &[],
        )
        .await?,
    )
    .await?;
    assert_eq!(availability["compress-pdf"]["enabled"], false);
    assert_eq!(availability["compress-pdf"]["reason"], "CONFIG");
    assert_eq!(availability["merge-pdfs"]["enabled"], true);
    assert!(availability["merge-pdfs"]["reason"].is_null());

    let enabled = json_body(
        request(
            runtime_config.clone(),
            "/api/v1/config/endpoints-enabled?endpoints=compress-pdf,merge-pdfs",
            &[],
        )
        .await?,
    )
    .await?;
    assert_eq!(enabled["compress-pdf"], false);
    assert_eq!(enabled["merge-pdfs"], true);

    let status =
        json_body(request(runtime_config, "/api/v1/settings/get-endpoints-status", &[]).await?)
            .await?;
    assert_eq!(status, serde_json::json!({ "compress-pdf": false }));
    Ok(())
}

#[tokio::test]
async fn group_enabled_route_applies_group_configuration() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempdir()?;
    let settings = directory.path().join("settings.yml");
    fs::write(&settings, "endpoints:\n  groupsToRemove: [PageOps]\n")?;
    let runtime_config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
    let response = request(
        runtime_config,
        "/api/v1/config/group-enabled?group=PageOps",
        &[],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await?, false);
    Ok(())
}

#[tokio::test]
async fn interceptor_blocks_disabled_routes_and_marks_api_responses_no_store()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let settings = directory.path().join("settings.yml");
    fs::write(&settings, "endpoints:\n  groupsToRemove: [PageOps]\n")?;
    let runtime_config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
    let response = request(runtime_config, "/api/v1/general/remove-pages", &[]).await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    Ok(())
}

/// A `settings.yml` still carrying the Ghostscript keys of a pre-removal install
/// must keep booting: the keys are accepted and ignored, never refused, and the
/// removed `Ghostscript` group and its endpoints are simply not advertised.
#[tokio::test]
async fn legacy_ghostscript_settings_are_accepted_and_ignored()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let settings = directory.path().join("settings.yml");
    fs::write(
        &settings,
        "processExecutor:\n  \
           sessionLimit:\n    ghostscriptSessionLimit: 8\n    qpdfSessionLimit: 2\n  \
           timeoutMinutes:\n    ghostscriptTimeoutMinutes: 30\n    qpdfTimeoutMinutes: 30\n\
         endpoints:\n  groupsToRemove: [Ghostscript]\n  toRemove: [pdf-to-pdfa, pdf-to-vector]\n",
    )?;
    let runtime_config =
        RuntimeConfig::from_files(&settings, directory.path().join("custom_settings.yml"));
    // The startup warning path must not panic on these keys.
    runtime_config.warn_on_ignored_legacy_settings();

    let availability = json_body(
        request(
            runtime_config.clone(),
            "/api/v1/config/endpoints-availability?endpoints=pdf-to-pdfa,pdf-to-vector,merge-pdfs",
            &[],
        )
        .await?,
    )
    .await?;
    // The removed `Ghostscript` group name in `groupsToRemove` matches no group,
    // so it disables nothing; unrelated endpoints stay enabled.
    assert_eq!(availability["merge-pdfs"]["enabled"], true);
    // The explicit `toRemove` entries for the deleted endpoints are still honoured
    // as opaque keys rather than rejected as unknown.
    for endpoint in ["pdf-to-pdfa", "pdf-to-vector"] {
        assert_eq!(availability[endpoint]["enabled"], false, "{endpoint}");
    }

    // The deleted routes are gone from the router entirely — checked against a
    // configuration that does not disable them, so a 404 can only come from the
    // route being absent rather than from the disabled-endpoint interceptor.
    let plain = directory.path().join("plain.yml");
    fs::write(&plain, "{}\n")?;
    for path in [
        "/api/v1/convert/pdf/pdfa",
        "/api/v1/convert/pdf/vector",
        "/api/v1/convert/vector/pdf",
    ] {
        let response = app_with_runtime_config(
            1024 * 1024,
            TimestampSettings::default(),
            RuntimeConfig::from_files(&plain, directory.path().join("missing.yml")),
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .body(Body::empty())?,
        )
        .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    Ok(())
}

fn configured_runtime() -> Result<RuntimeConfig, Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let settings = directory.path().join("settings.yml");
    let custom = directory.path().join("custom_settings.yml");
    fs::write(
        &settings,
        "system:\n  defaultLocale: en-US\n  enableUrlToPDF: true\nui:\n  logoStyle: classic\nendpoints:\n  toRemove: [compress-pdf]\n",
    )?;
    fs::write(&custom, "system:\n  defaultLocale: vi-VN\n")?;
    Ok(RuntimeConfig::from_files(settings, custom))
}

async fn request(
    runtime_config: RuntimeConfig,
    uri: &str,
    headers: &[(&str, &str)],
) -> Result<Response, Box<dyn std::error::Error>> {
    let mut request = Request::builder().method("GET").uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    Ok(
        app_with_runtime_config(1024 * 1024, TimestampSettings::default(), runtime_config)
            .oneshot(request.body(Body::empty())?)
            .await?,
    )
}

async fn json_body(response: Response) -> Result<Value, Box<dyn std::error::Error>> {
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok(serde_json::from_slice(&body)?)
}
