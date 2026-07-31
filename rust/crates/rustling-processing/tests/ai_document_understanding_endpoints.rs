use std::{
    fs,
    sync::{Arc, Mutex, PoisonError},
};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    routing::post,
};
use lopdf::{Document, Object, Stream, dictionary};
use rustling_processing::{
    TimestampSettings, app_with_runtime_config, runtime_config::RuntimeConfig,
};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};
use tokio::{net::TcpListener, task::JoinHandle};
use tower::ServiceExt as _;

const SUMMARY_PATH: &str = "/api/v1/ai/tools/document-summary";
const EXTRACTION_PATH: &str = "/api/v1/ai/tools/document-extraction";
const TRANSLATION_PATH: &str = "/api/v1/ai/tools/document-translation";

#[tokio::test]
async fn validates_operation_settings_and_disabled_engine_before_pdf_parsing()
-> Result<(), Box<dyn std::error::Error>> {
    let (_directory, app) = processing_app(false, "http://127.0.0.1:1", 200)?;
    let missing_language =
        post_multipart(&app, TRANSLATION_PATH, b"%PDF-1.7\nnot parsed", &[]).await?;
    assert_eq!(missing_language.status(), StatusCode::BAD_REQUEST);

    let duplicate_fields = post_multipart(
        &app,
        EXTRACTION_PATH,
        b"%PDF-1.7\nnot parsed",
        &[(
            "fields",
            r#"[{"key":"x","description":"one","valueType":"string"},{"key":"x","description":"two","valueType":"string"}]"#,
        )],
    )
    .await?;
    assert_eq!(duplicate_fields.status(), StatusCode::BAD_REQUEST);

    for path in [SUMMARY_PATH, EXTRACTION_PATH, TRANSLATION_PATH] {
        let fields = match path {
            EXTRACTION_PATH => vec![(
                "fields",
                r#"[{"key":"invoice","description":"Invoice number","valueType":"string"}]"#,
            )],
            TRANSLATION_PATH => vec![("targetLanguage", "Vietnamese")],
            _ => Vec::new(),
        };
        let response = post_multipart(&app, path, b"%PDF-1.7\nnot parsed", &fields).await?;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), 16_384).await?;
        assert!(String::from_utf8_lossy(&body).contains("AI engine is not enabled"));
    }
    Ok(())
}

#[tokio::test]
async fn proxies_bounded_page_text_for_all_three_stateless_operations()
-> Result<(), Box<dyn std::error::Error>> {
    let captured = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let (engine_url, engine_task) = spawn_engine(Arc::clone(&captured)).await?;
    let (directory, app) = processing_app(true, &engine_url, 1)?;
    let pdf = text_pdf()?;

    let summary = post_multipart(
        &app,
        SUMMARY_PATH,
        &pdf,
        &[("detail", "brief"), ("instructions", "Focus on totals")],
    )
    .await?;
    assert_eq!(summary.status(), StatusCode::OK);
    let summary = response_json(summary).await?;
    assert_eq!(summary["operation"], "summary");
    assert_eq!(summary["source"]["pagesProcessed"], 1);
    assert_eq!(summary["source"]["maxPages"], 1);
    assert_eq!(summary["result"]["keyPoints"][0]["pages"], json!([1]));
    assert!(
        summary["providerDisclosure"]
            .as_str()
            .is_some_and(|value| value.contains("AI provider"))
    );

    let extraction = post_multipart(
        &app,
        EXTRACTION_PATH,
        &pdf,
        &[(
            "fields",
            r#"[{"key":"invoice","description":"Invoice number","valueType":"string","required":true}]"#,
        )],
    )
    .await?;
    assert_eq!(extraction.status(), StatusCode::OK);
    let extraction = response_json(extraction).await?;
    assert_eq!(extraction["operation"], "extraction");
    assert_eq!(extraction["result"]["values"][0]["key"], "invoice");

    let translation = post_multipart(
        &app,
        TRANSLATION_PATH,
        &pdf,
        &[
            ("targetLanguage", "Vietnamese"),
            ("sourceLanguage", "English"),
        ],
    )
    .await?;
    assert_eq!(translation.status(), StatusCode::OK);
    let translation = response_json(translation).await?;
    assert_eq!(translation["operation"], "translation");
    assert_eq!(translation["result"]["pages"][0]["pageNumber"], 1);

    let captured = captured.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(captured.len(), 3);
    for (_path, request) in captured.iter() {
        assert_eq!(request["fileName"], "source.pdf");
        assert_eq!(request["pages"].as_array().map(Vec::len), Some(1));
        assert_eq!(request["pages"][0]["pageNumber"], 1);
        let wire = request.to_string();
        assert!(!wire.contains("%PDF"));
        assert!(!wire.contains("input.pdf"));
    }
    assert_eq!(
        captured
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/api/v1/ai/document/summary",
            "/api/v1/ai/document/extraction",
            "/api/v1/ai/document/translation"
        ]
    );
    drop(captured);

    // The request workspace is temporary; no upload, extracted text, or result
    // appears beside the operator-owned settings file.
    let persisted = fs::read_dir(directory.path().join("configs"))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(persisted, vec![std::ffi::OsString::from("settings.yml")]);

    engine_task.abort();
    Ok(())
}

fn processing_app(
    enabled: bool,
    engine_url: &str,
    max_pages: usize,
) -> Result<(TempDir, Router), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let config_directory = directory.path().join("configs");
    fs::create_dir_all(&config_directory)?;
    let settings_path = config_directory.join("settings.yml");
    fs::write(
        &settings_path,
        format!(
            "aiEngine:\n  enabled: {enabled}\n  url: {engine_url}\n  timeoutSeconds: 5\n  pushConfigToEngine: false\n  limits:\n    maxPages: {max_pages}\n    maxCharacters: 200000\n"
        ),
    )?;
    let config =
        RuntimeConfig::from_files(settings_path, config_directory.join("missing-custom.yml"));
    Ok((
        directory,
        app_with_runtime_config(2 * 1024 * 1024, TimestampSettings::default(), config),
    ))
}

async fn spawn_engine(
    captured: Arc<Mutex<Vec<(String, Value)>>>,
) -> Result<(String, JoinHandle<()>), Box<dyn std::error::Error>> {
    let summary_capture = Arc::clone(&captured);
    let extraction_capture = Arc::clone(&captured);
    let translation_capture = captured;
    let engine = Router::new()
        .route(
            "/api/v1/ai/document/summary",
            post(move |Json(body): Json<Value>| {
                let captured = Arc::clone(&summary_capture);
                async move {
                    captured
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(("/api/v1/ai/document/summary".to_owned(), body));
                    Json(json!({
                        "summary":"First page summary",
                        "keyPoints":[{"text":"First page","pages":[1]}]
                    }))
                }
            }),
        )
        .route(
            "/api/v1/ai/document/extraction",
            post(move |Json(body): Json<Value>| {
                let captured = Arc::clone(&extraction_capture);
                async move {
                    captured
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(("/api/v1/ai/document/extraction".to_owned(), body));
                    Json(json!({
                        "values":[{
                            "key":"invoice",
                            "value":"42",
                            "pages":[1],
                            "confidence":"high"
                        }]
                    }))
                }
            }),
        )
        .route(
            "/api/v1/ai/document/translation",
            post(move |Json(body): Json<Value>| {
                let captured = Arc::clone(&translation_capture);
                async move {
                    captured
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(("/api/v1/ai/document/translation".to_owned(), body));
                    Json(json!({
                        "sourceLanguage":"English",
                        "targetLanguage":"Vietnamese",
                        "pages":[{
                            "pageNumber":1,
                            "blocks":[{
                                "blockId":"p1-b1",
                                "sourceText":"First page",
                                "translatedText":"Trang đầu"
                            }]
                        }]
                    }))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _result = axum::serve(listener, engine).await;
    });
    Ok((format!("http://{address}"), task))
}

async fn post_multipart(
    app: &Router,
    path: &str,
    pdf: &[u8],
    fields: &[(&str, &str)],
) -> Result<axum::response::Response, Box<dyn std::error::Error>> {
    let boundary = format!(
        "rustling-ai-document-{}",
        path.rsplit('/').next().unwrap_or("request")
    );
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
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
    body.extend_from_slice(pdf);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(app
        .clone()
        .oneshot(
            Request::post(path)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?)
}

async fn response_json(
    response: axum::response::Response,
) -> Result<Value, Box<dyn std::error::Error>> {
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024).await?;
    Ok(serde_json::from_slice(&body)?)
}

fn text_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let mut pages = Vec::new();
    for text in ["First page invoice 42", "Second page must stay local"] {
        let content = document.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 10 50 Td ({text}) Tj ET").into_bytes(),
        ));
        pages.push(document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 100.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            "Contents" => content,
        }));
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => pages.into_iter().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => 2,
        }),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}
