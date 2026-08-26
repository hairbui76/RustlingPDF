use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use rustling_processing::app;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn checks_accessibility_without_claiming_conformance()
-> Result<(), Box<dyn std::error::Error>> {
    let (pdf, figure_id) = accessibility_pdf()?;
    let response = require_status(
        post_multipart("/api/v1/accessibility/check", Some(&pdf), None).await?,
        StatusCode::OK,
    )
    .await?;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let report: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;

    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["document"]["pageCount"], 1);
    assert_eq!(report["document"]["figureCount"], 1);
    assert_eq!(report["document"]["formFieldCount"], 1);
    assert_eq!(report["document"]["structureOrder"][0]["role"], "Figure");
    let figure = report["findings"]
        .as_array()
        .and_then(|findings| {
            findings
                .iter()
                .find(|finding| finding["ruleId"] == "figure.alternative-text")
        })
        .ok_or("missing Figure finding")?;
    assert_eq!(figure["status"], "fail");
    assert_eq!(figure["objectNumber"], figure_id.0);
    assert_eq!(report["summary"]["manualReview"], 1);
    assert!(report.get("compliant").is_none());
    Ok(())
}

#[tokio::test]
async fn remediates_and_rechecks_the_returned_pdf() -> Result<(), Box<dyn std::error::Error>> {
    let (pdf, figure_id) = accessibility_pdf()?;
    let repairs = json!({
        "documentLanguage": "en-US",
        "markAsTagged": true,
        "structureTabOrderPages": [0],
        "alternativeTexts": [{
            "objectNumber": figure_id.0,
            "generation": figure_id.1,
            "text": "Revenue by quarter"
        }],
        "formFieldTooltips": [{
            "fieldName": "customer.name",
            "text": "Customer name"
        }]
    })
    .to_string();
    let response = require_status(
        post_multipart(
            "/api/v1/accessibility/remediate",
            Some(&pdf),
            Some(&repairs),
        )
        .await?,
        StatusCode::OK,
    )
    .await?;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"source_accessible.pdf\""
    );
    let remediated = to_bytes(response.into_body(), usize::MAX).await?;
    let response = require_status(
        post_multipart("/api/v1/accessibility/check", Some(&remediated), None).await?,
        StatusCode::OK,
    )
    .await?;
    let report: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(report["summary"]["failed"], 0);
    assert_eq!(report["summary"]["manualReview"], 1);
    assert_eq!(report["document"]["language"], "en-US");
    assert_eq!(report["document"]["marked"], true);
    assert_eq!(
        report["document"]["structureOrder"][0]["alternativeText"],
        "Revenue by quarter"
    );
    Ok(())
}

#[tokio::test]
async fn validates_files_json_and_atomic_targets() -> Result<(), Box<dyn std::error::Error>> {
    let missing = post_multipart("/api/v1/accessibility/check", None, None).await?;
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);

    let (pdf, _) = accessibility_pdf()?;
    for repairs in [
        None,
        Some("not-json"),
        Some(r#"{"documentLanguage":"en-US","unexpected":true}"#),
        Some(
            r#"{"documentLanguage":"en-US","alternativeTexts":[{"objectNumber":999999,"generation":0,"text":"Description"}]}"#,
        ),
    ] {
        let response =
            post_multipart("/api/v1/accessibility/remediate", Some(&pdf), repairs).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        assert_eq!(error["path"], "/api/v1/accessibility/remediate");
    }
    Ok(())
}

async fn post_multipart(
    path: &str,
    pdf: Option<&[u8]>,
    repairs: Option<&str>,
) -> Result<Response, Box<dyn std::error::Error>> {
    let boundary = "rustling-accessibility-boundary";
    let mut body = Vec::new();
    if let Some(pdf) = pdf {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"fileInput\"; filename=\"source.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(pdf);
        body.extend_from_slice(b"\r\n");
    }
    if let Some(repairs) = repairs {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"repairs\"\r\nContent-Type: application/json\r\n\r\n{repairs}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(app(1024 * 1024)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?)
}

async fn require_status(
    response: Response,
    expected: StatusCode,
) -> Result<Response, Box<dyn std::error::Error>> {
    if response.status() == expected {
        return Ok(response);
    }
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    Err(std::io::Error::other(format!(
        "expected HTTP {expected}, received {status}: {}",
        String::from_utf8_lossy(&body)
    ))
    .into())
}

fn accessibility_pdf() -> Result<(Vec<u8>, ObjectId), Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let leaf_page_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
    let field_id = document.new_object_id();
    let widget_id = document.new_object_id();
    let figure_id = document.new_object_id();

    document.objects.insert(
        widget_id,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Parent" => field_id,
            "P" => leaf_page_id,
            "Rect" => vec![10.into(), 10.into(), 110.into(), 30.into()],
        }),
    );
    document.objects.insert(
        field_id,
        Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("customer.name"),
            "Kids" => vec![widget_id.into()],
        }),
    );
    document.objects.insert(
        leaf_page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => page_tree_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
            "Contents" => content_id,
            "Resources" => dictionary! {},
            "Annots" => vec![widget_id.into()],
            "Tabs" => "A",
        }),
    );
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![leaf_page_id.into()],
            "Count" => 1,
        }),
    );
    document.objects.insert(
        figure_id,
        Object::Dictionary(dictionary! {
            "Type" => "StructElem",
            "S" => "Figure",
            "Pg" => leaf_page_id,
            "K" => 0,
        }),
    );
    let structure_root_id = document.add_object(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => vec![figure_id.into()],
    });
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => page_tree_id,
        "StructTreeRoot" => structure_root_id,
        "AcroForm" => dictionary! { "Fields" => vec![field_id.into()] },
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok((bytes, figure_id))
}
