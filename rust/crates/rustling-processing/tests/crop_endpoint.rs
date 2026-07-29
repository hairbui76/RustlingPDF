use std::fmt::Write as _;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use lopdf::{Document, Object, Stream, dictionary};
use rustling_processing::app;
use tower::ServiceExt;

#[tokio::test]
async fn crops_every_page_to_the_requested_nonzero_media_box()
-> Result<(), Box<dyn std::error::Error>> {
    let response = require_status(
        post_crop(
            "manual.pdf",
            &pdf_with_content(&["BASE0", "BASE1"])?,
            &[
                ("x", "10"),
                ("y", "20"),
                ("width", "100"),
                ("height", "150"),
                ("removeDataOutsideCrop", "false"),
            ],
        )
        .await?,
        StatusCode::OK,
    )
    .await?;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()?
            .contains("manual_cropped.pdf")
    );
    let output = response_document(response).await?;
    assert_eq!(output.get_pages().len(), 2);
    for page_id in output.get_pages().into_values() {
        assert_box_close(page_box(&output, page_id)?, [10.0, 20.0, 110.0, 170.0]);
        let content = output.get_page_content(page_id);
        assert!(find_bytes(&content, b"10 20 100 150 re W n").is_some());
        assert!(find_bytes(&content, b"/Fm0 Do").is_some());
    }
    assert!(output.catalog()?.get(b"AcroForm").is_err());
    Ok(())
}

#[tokio::test]
async fn rejects_manual_crop_without_all_coordinates() -> Result<(), Box<dyn std::error::Error>> {
    let response = require_status(
        post_crop(
            "missing.pdf",
            &pdf_with_content(&["BASE"])?,
            &[("x", "10"), ("y", "20"), ("width", "100")],
        )
        .await?,
        StatusCode::BAD_REQUEST,
    )
    .await?;
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(String::from_utf8_lossy(&body).contains("/api/v1/general/crop"));
    Ok(())
}

#[tokio::test]
async fn auto_crop_detects_rendered_content_when_pdfium_is_available()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "auto.pdf",
        &pdf_with_content(&["0 0 0 rg 50 60 100 120 re f"])?,
        &[("autoCrop", "true")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        assert!(String::from_utf8_lossy(&body).contains("PDFium"));
        if rustling_processing::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_some() {
            return Err(std::io::Error::other(
                "configured PDFium runtime did not execute auto-crop",
            )
            .into());
        }
        return Ok(());
    }
    let output = response_document(require_status(response, StatusCode::OK).await?).await?;
    let page_id = output
        .get_pages()
        .into_values()
        .next()
        .ok_or("missing page")?;
    let bounds = page_box(&output, page_id)?;
    assert_approximately(bounds[0], 50.0, 2.0);
    assert_approximately(bounds[1], 60.0, 2.0);
    assert_approximately(bounds[2] - bounds[0], 100.0, 2.0);
    assert_approximately(bounds[3] - bounds[1], 120.0, 2.0);
    Ok(())
}

/// `removeDataOutsideCrop=true` is a privacy promise: text outside the crop
/// rectangle must be absent from the returned bytes, not merely clipped. With
/// `false` the same text must still be there, so the flag is demonstrably load
/// bearing rather than decorative.
#[tokio::test]
async fn remove_data_outside_crop_discards_out_of_crop_text()
-> Result<(), Box<dyn std::error::Error>> {
    let source = pdf_with_text_inside_and_outside()?;
    let coordinates = [("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")];

    let mut clipped_only = coordinates.to_vec();
    clipped_only.push(("removeDataOutsideCrop", "false"));
    let clipped = require_status(
        post_crop("privacy.pdf", &source, &clipped_only).await?,
        StatusCode::OK,
    )
    .await?;
    let clipped = to_bytes(clipped.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&clipped, b"OUTSIDECROP")?,
        "clip-only mode must keep the original marks in the file"
    );

    let removed = post_crop("privacy.pdf", &source, &coordinates).await?;
    if removed.status() == StatusCode::NOT_IMPLEMENTED {
        let body = to_bytes(removed.into_body(), usize::MAX).await?;
        assert!(String::from_utf8_lossy(&body).contains("PDFium"));
        if rustling_processing::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_some() {
            return Err(std::io::Error::other(
                "configured PDFium runtime did not execute out-of-crop removal",
            )
            .into());
        }
        // Without PDFium the route refuses rather than silently returning a file
        // that still contains the data the caller asked to remove.
        return Ok(());
    }
    let removed = require_status(removed, StatusCode::OK).await?;
    let removed = to_bytes(removed.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&removed, b"OUTSIDECROP")?,
        "out-of-crop text survived removeDataOutsideCrop=true"
    );
    assert!(
        document_contains(&removed, b"INSIDECROP")?,
        "in-crop text was removed too"
    );
    let document = Document::load_mem(&removed)?;
    let page_id = document
        .get_pages()
        .into_values()
        .next()
        .ok_or("missing page")?;
    assert_box_close(page_box(&document, page_id)?, [0.0, 0.0, 200.0, 100.0]);
    Ok(())
}

/// Whether `marker` appears anywhere in the document.
///
/// Both encodings that matter are searched, in raw bytes and inside every
/// decompressed stream: the crop rebuild Flate-compresses its output, and
/// `PDFium` re-emits text-showing operands as hex strings when it regenerates a
/// page's content. A naive ASCII scan of the raw bytes would therefore report
/// every marker as absent and let the removal assertion pass vacuously.
fn document_contains(pdf: &[u8], marker: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
    let mut hex = String::with_capacity(marker.len() * 2);
    for byte in marker {
        write!(hex, "{byte:02X}")?;
    }
    let hex = hex.into_bytes();
    let contains = |haystack: &[u8]| {
        find_bytes(haystack, marker).is_some() || find_bytes(haystack, &hex).is_some()
    };
    if contains(pdf) {
        return Ok(true);
    }
    let document = Document::load_mem(pdf)?;
    Ok(document.objects.values().any(|object| {
        object.as_stream().is_ok_and(|stream| {
            stream
                .decompressed_content()
                .is_ok_and(|content| contains(&content))
        })
    }))
}

/// One page with a text run well inside the crop rectangle and another well
/// outside it.
fn pdf_with_text_inside_and_outside() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 20 40 Td (INSIDECROP) Tj ET\n\
          BT /F1 12 Tf 20 250 Td (OUTSIDECROP) Tj ET"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => content_id,
    });
    document.objects.insert(
        root_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => root_pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn page_box(
    document: &Document,
    page_id: lopdf::ObjectId,
) -> Result<[f32; 4], Box<dyn std::error::Error>> {
    let media_box = document
        .get_dictionary(page_id)?
        .get(b"MediaBox")?
        .as_array()?;
    Ok([
        media_box[0].as_float()?,
        media_box[1].as_float()?,
        media_box[2].as_float()?,
        media_box[3].as_float()?,
    ])
}

fn assert_box_close(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_approximately(actual, expected, 0.01);
    }
}

fn assert_approximately(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} ± {tolerance}, received {actual}"
    );
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn response_document(response: Response) -> Result<Document, Box<dyn std::error::Error>> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(Document::load_mem(&bytes)?)
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

async fn post_crop(
    filename: &str,
    pdf: &[u8],
    fields: &[(&str, &str)],
) -> Result<Response, Box<dyn std::error::Error>> {
    let boundary = "stirling-crop-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"fileInput\"; filename=\"{filename}\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(pdf);
    body.extend_from_slice(b"\r\n");
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(app(1024 * 1024)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/general/crop")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?)
}

fn pdf_with_content(contents: &[&str]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let mut pages = Vec::with_capacity(contents.len());
    for content in contents {
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => root_pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "Contents" => content_id,
        });
        pages.push(Object::Reference(page_id));
    }
    document.objects.insert(
        root_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => pages,
            "Count" => i64::try_from(contents.len())?,
            "Resources" => dictionary! {},
        }),
    );
    let acroform_id = document.add_object(dictionary! { "Fields" => Vec::<Object>::new() });
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => root_pages_id,
        "AcroForm" => acroform_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}
