use std::{fs, process::Command};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use lopdf::{Document, Object, Stream, dictionary};
use rustling_processing::{
    TimestampSettings, app_with_runtime_config, runtime_config::RuntimeConfig,
};
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn requires_at_least_one_language() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_ocr(&single_page_pdf()?, &[("ocrRenderType", "hocr")]).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_response_contains(response, "OCR language options are not specified").await?;
    Ok(())
}

#[tokio::test]
async fn rejects_an_invalid_render_type() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_ocr(
        &single_page_pdf()?,
        &[("languages", "eng"), ("ocrRenderType", "fancy")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_response_contains(
        response,
        "Invalid OCR render type. Must be 'hocr' or 'sandwich'",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn rejects_a_request_when_no_selected_language_is_installed()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_ocr(
        &single_page_pdf()?,
        &[("languages", "fra"), ("languages", "ENG")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_response_contains(
        response,
        "Invalid OCR languages format: none of the selected languages are valid",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn preserves_whitespace_during_java_compatible_validation()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_ocr(
        &single_page_pdf()?,
        &[("languages", " eng "), ("ocrRenderType", "hocr")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_response_contains(
        response,
        "Invalid OCR languages format: none of the selected languages are valid",
    )
    .await?;

    let response = post_ocr(
        &single_page_pdf()?,
        &[("languages", "eng"), ("ocrRenderType", " hocr ")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_response_contains(
        response,
        "Invalid OCR render type. Must be 'hocr' or 'sandwich'",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn ocr_follows_available_tooling() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_ocr(&single_page_pdf()?, &[("languages", "eng")]).await?;
    if ocrmypdf_present() || tesseract_present() {
        let response = require_status(response, StatusCode::OK).await?;
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
        assert!(response_bytes(response).await?.starts_with(b"%PDF"));
    } else {
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }
    Ok(())
}

/// `removeImagesAfter=true` used to shell out to Ghostscript `-dFILTERIMAGE`.
/// It is now pure Rust, so it must work with no external image tool: the OCR'd
/// PDF comes back with every image `XObject` and its bytes gone, while the page's
/// text layer stays. The fixture carries real text next to the image, so the
/// assertion does not depend on what OCR happens to recognise in a synthetic
/// bitmap; `skip-text` keeps that text rather than replacing it.
#[tokio::test]
async fn remove_images_after_strips_images_without_an_external_tool()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_ocr(
        &page_with_image_and_text()?,
        &[
            ("languages", "eng"),
            ("ocrType", "skip-text"),
            ("removeImagesAfter", "true"),
        ],
    )
    .await?;
    // `removeImagesAfter` runs only after a successful ocrmypdf OCR — `run_ocr`
    // gates it on `used_ocrmypdf` — so the pure-Rust image removal this test
    // exercises is reachable only when ocrmypdf is available. Without it there is
    // nothing to assert here: the real server refuses the endpoint outright,
    // because its startup dependency discovery disables the OCR group when no tool
    // is found; this harness deliberately builds a probe-free `RuntimeConfig`
    // (see `RuntimeConfig::with_dependency_discovery`), so the group stays enabled
    // and the endpoint passes the already-text `skip-text` page straight through
    // for a `200` rather than reaching the `501`. Skip rather than assert a status
    // this harness cannot produce; `ocr_follows_available_tooling` covers the
    // tool-absent refusal path.
    if !ocrmypdf_present() {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    let bytes = response_bytes(response).await?;
    assert!(bytes.starts_with(b"%PDF"));
    let document = Document::load_mem(&bytes)?;
    assert!(
        !document
            .objects
            .values()
            .any(|object| object.as_stream().is_ok_and(|stream| stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|subtype| subtype == b"Image"))),
        "an image XObject survived removeImagesAfter=true"
    );
    // The text layer OCR added must still be there: at least one font resource
    // and one text-showing operator.
    let has_text = document.get_pages().into_values().any(|page_id| {
        let content = document.get_page_content(page_id);
        lopdf::content::Content::decode(&content).is_ok_and(|content| {
            content
                .operations
                .iter()
                .any(|operation| matches!(operation.operator.as_str(), "Tj" | "TJ"))
        })
    });
    assert!(
        has_text,
        "the OCR text layer was removed along with the images"
    );
    Ok(())
}

/// A page carrying both a small raster image (to be stripped) and a real text
/// object (to be preserved).
fn page_with_image_and_text() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    // 4x4 checkerboard, 8-bit gray, uncompressed.
    let mut samples = Vec::with_capacity(16);
    for row in 0..4 {
        for column in 0..4 {
            samples.push(if (row + column) % 2 == 0 { 0 } else { 255 });
        }
    }
    let image_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 4,
            "Height" => 4,
            "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
            "BitsPerComponent" => 8,
        },
        samples,
    ));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"q 160 0 0 100 20 50 cm /Im0 Do Q\nBT /F1 14 Tf 20 20 Td (OCRTEXTLAYER) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "XObject" => dictionary! { "Im0" => Object::Reference(image_id) },
        },
        "Contents" => content_id,
    });
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn tesseract_present() -> bool {
    if let Some(command) =
        rustling_processing::environment::var_os("RUSTLING_PROCESSING_TESSERACT_COMMAND")
        && !command.is_empty()
    {
        return Command::new(command).arg("--version").output().is_ok();
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["tesseract.exe", "tesseract"]
    } else {
        &["tesseract"]
    };
    candidates
        .iter()
        .any(|command| Command::new(command).arg("--version").output().is_ok())
}

fn ocrmypdf_present() -> bool {
    if let Some(command) =
        rustling_processing::environment::var_os("RUSTLING_PROCESSING_OCRMYPDF_COMMAND")
        && !command.is_empty()
    {
        return Command::new(command).arg("--version").output().is_ok();
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["ocrmypdf.exe", "ocrmypdf"]
    } else {
        &["ocrmypdf"]
    };
    candidates
        .iter()
        .any(|command| Command::new(command).arg("--version").output().is_ok())
}

async fn response_bytes(response: Response) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(to_bytes(response.into_body(), usize::MAX).await?.to_vec())
}

async fn assert_response_contains(
    response: Response,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = response_bytes(response).await?;
    let body = String::from_utf8_lossy(&body);
    if body.contains(expected) {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "expected response body to contain {expected:?}, received {body}"
    ))
    .into())
}

async fn require_status(
    response: Response,
    expected: StatusCode,
) -> Result<Response, Box<dyn std::error::Error>> {
    if response.status() == expected {
        return Ok(response);
    }
    let status = response.status();
    let body = response_bytes(response).await?;
    Err(std::io::Error::other(format!(
        "expected HTTP {expected}, received {status}: {}",
        String::from_utf8_lossy(&body)
    ))
    .into())
}

async fn post_ocr(
    pdf: &[u8],
    fields: &[(&str, &str)],
) -> Result<Response, Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let tessdata = directory.path().join("tessdata");
    fs::create_dir(&tessdata)?;
    fs::write(tessdata.join("eng.traineddata"), "test")?;
    let settings = directory.path().join("settings.yml");
    fs::write(
        &settings,
        format!(
            "system:\n  tessdataDir: {}\n",
            tessdata.to_string_lossy().replace('\\', "/")
        ),
    )?;
    let runtime_config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));

    let boundary = "rustling-ocr-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"fileInput\"; filename=\"source.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(pdf);
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(
        app_with_runtime_config(1024 * 1024, TimestampSettings::default(), runtime_config)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/misc/ocr-pdf")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))?,
            )
            .await?,
    )
}

fn single_page_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(dictionary! {}, b"".to_vec()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
        "Resources" => dictionary! {},
        "Contents" => content_id,
    });
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}
