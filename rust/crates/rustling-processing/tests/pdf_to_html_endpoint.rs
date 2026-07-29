use std::io::{Cursor, Read};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use lopdf::{Document, Object, Stream, dictionary};
use rustling_processing::{
    TimestampSettings, app, app_with_runtime_config, runtime_config::RuntimeConfig,
};
use tower::ServiceExt;
use zip::ZipArchive;

#[tokio::test]
async fn native_conversion_preserves_pages_text_order_and_images()
-> Result<(), Box<dyn std::error::Error>> {
    if !native_pdfium_configured() {
        return Ok(());
    }
    let response = post_pdf(app(1024 * 1024), &two_page_pdf_with_image()?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()?
            .contains("ToHtml.zip")
    );

    let mut archive = ZipArchive::new(Cursor::new(response_bytes(response).await?))?;
    let names = archive
        .file_names()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    assert!(names.contains(&"source.html".to_owned()));
    assert!(names.contains(&"source.css".to_owned()));
    assert!(names.contains(&"source_page_2_1.png".to_owned()));

    let mut html = String::new();
    archive.by_name("source.html")?.read_to_string(&mut html)?;
    assert!(html.contains("data-renderer=\"native-pdfium\""));
    assert!(html.contains("id=\"page-1\""));
    assert!(html.contains("id=\"page-2\""));
    assert!(html.contains("First page"));
    assert!(html.contains("Second page"));
    assert!(matches!(
        (html.find("First page"), html.find("Second page")),
        (Some(first), Some(second)) if first < second
    ));
    assert!(html.contains("src=\"source_page_2_1.png\""));
    assert!(html.contains("class=\"text-run\""));

    let mut image = Vec::new();
    archive
        .by_name("source_page_2_1.png")?
        .read_to_end(&mut image)?;
    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
    Ok(())
}

/// `/Rotate 90|180|270` pages must land text at the same plausible non-zero offsets as
/// the unrotated baseline and rotate at the CSS level, with the section reserving the
/// rotated layout box. Regression: mixing PDFium's rotation-aware page extents with
/// unrotated text coordinates clamped every run to `top:0.00pt`.
#[tokio::test]
async fn rotated_pages_position_text_and_rotate_the_canvas()
-> Result<(), Box<dyn std::error::Error>> {
    if !native_pdfium_configured() {
        return Ok(());
    }
    let baseline = native_html(&rotated_pdf(None)?).await?;
    assert!(baseline.contains("data-rotation=\"0\""));
    assert!(baseline.contains("style=\"width:612.00pt;height:792.00pt\""));
    assert!(!baseline.contains("rotate("));
    let baseline_tops = text_run_tops(&baseline);
    assert_eq!(
        baseline_tops.len(),
        3,
        "expected three text runs, got: {baseline_tops:?}\n{baseline}"
    );
    assert!(
        baseline_tops.iter().all(|top| *top > 1.0),
        "unrotated baseline collapsed to the top edge: {baseline_tops:?}"
    );
    assert!(
        baseline_tops.windows(2).all(|pair| pair[0] < pair[1]),
        "runs are not top-to-bottom: {baseline_tops:?}"
    );

    for (rotation, section, transform) in [
        (
            90,
            "style=\"width:792.00pt;height:612.00pt\"",
            "transform:translate(792.00pt,0pt) rotate(90deg)",
        ),
        (
            180,
            "style=\"width:612.00pt;height:792.00pt\"",
            "transform:translate(612.00pt,792.00pt) rotate(180deg)",
        ),
        (
            270,
            "style=\"width:792.00pt;height:612.00pt\"",
            "transform:translate(0pt,612.00pt) rotate(270deg)",
        ),
    ] {
        let html = native_html(&rotated_pdf(Some(rotation))?).await?;
        assert!(
            html.contains(&format!("data-rotation=\"{rotation}\"")),
            "/Rotate {rotation} was ignored, got:\n{html}"
        );
        assert!(
            html.contains(section),
            "/Rotate {rotation} must lay out the rotated page box, got:\n{html}"
        );
        assert!(
            html.contains(transform),
            "/Rotate {rotation} must carry its canvas transform, got:\n{html}"
        );
        let tops = text_run_tops(&html);
        assert_eq!(
            tops, baseline_tops,
            "/Rotate {rotation} moved runs inside the canvas"
        );
        assert!(
            matches!(
                (html.find("First line"), html.find("Third line")),
                (Some(first), Some(third)) if first < third
            ),
            "/Rotate {rotation} lost reading order, got:\n{html}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn damaged_pdf_degrades_to_a_typed_http_error() -> Result<(), Box<dyn std::error::Error>> {
    if !native_pdfium_configured() {
        return Ok(());
    }
    let response = post_pdf(
        app(1024 * 1024),
        b"%PDF-1.7\nnot a valid cross-reference table",
    )
    .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn installed_pdftohtml_keeps_external_precedence() -> Result<(), Box<dyn std::error::Error>> {
    if !pdftohtml_present() {
        return Ok(());
    }
    let router = app_with_runtime_config(
        1024 * 1024,
        TimestampSettings::default(),
        RuntimeConfig::from_environment().with_dependency_discovery(),
    );
    let response = post_pdf(router, &single_page_pdf()?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let archive = ZipArchive::new(Cursor::new(response_bytes(response).await?))?;
    let names = archive
        .file_names()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    assert!(names.contains(&"source.html".to_owned()));
    assert!(
        !names.contains(&"source.css".to_owned()),
        "Poppler -c emits its CSS inline; a native CSS asset would show that fallback won"
    );
    Ok(())
}

#[tokio::test]
async fn requires_a_file() -> Result<(), Box<dyn std::error::Error>> {
    let boundary = "stirling-pdf-html-empty";
    let body = format!("--{boundary}--\r\n").into_bytes();
    let response = app(1024 * 1024)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/convert/pdf/html")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

/// Converts through the native renderer and returns the generated HTML.
async fn native_html(pdf: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let response = post_pdf(app(1024 * 1024), pdf).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let mut archive = ZipArchive::new(Cursor::new(response_bytes(response).await?))?;
    let mut html = String::new();
    archive.by_name("source.html")?.read_to_string(&mut html)?;
    assert!(
        html.contains("data-renderer=\"native-pdfium\""),
        "expected the native renderer"
    );
    Ok(html)
}

/// Extracts every `top:` value from the emitted text runs, in DOM order.
fn text_run_tops(html: &str) -> Vec<f32> {
    html.lines()
        .filter(|line| line.contains("class=\"text-run\""))
        .filter_map(|line| {
            let start = line.find(";top:")? + ";top:".len();
            let rest = &line[start..];
            let end = rest.find("pt")?;
            rest[..end].parse::<f32>().ok()
        })
        .collect()
}

/// A single US-Letter page carrying three stacked lines, optionally with `/Rotate`.
fn rotated_pdf(rotation: Option<i64>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 72 720 Td (First line) Tj ET\n\
          BT /F1 12 Tf 72 674 Td (Second line) Tj ET\n\
          BT /F1 12 Tf 72 628 Td (Third line) Tj ET"
            .to_vec(),
    ));
    let mut page = dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => content_id,
    };
    if let Some(rotation) = rotation {
        page.set("Rotate", rotation);
    }
    let page_id = document.add_object(page);
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

fn native_pdfium_configured() -> bool {
    rustling_processing::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_some()
}

fn pdftohtml_present() -> bool {
    let candidates: &[&str] = if cfg!(windows) {
        &["pdftohtml.exe", "pdftohtml"]
    } else {
        &["/usr/bin/pdftohtml", "pdftohtml"]
    };
    candidates.iter().any(|command| {
        std::process::Command::new(command)
            .arg("-v")
            .output()
            .is_ok()
    })
}

async fn response_bytes(response: Response) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(to_bytes(response.into_body(), usize::MAX).await?.to_vec())
}

async fn post_pdf(router: Router, pdf: &[u8]) -> Result<Response, Box<dyn std::error::Error>> {
    let boundary = "stirling-pdf-to-html-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"fileInput\"; filename=\"source.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(pdf);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/convert/pdf/html")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?)
}

fn single_page_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    text_pdf(&[("Hello HTML", 50)])
}

fn two_page_pdf_with_image() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let image_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2,
            "Height" => 1,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        vec![255, 0, 0, 0, 255, 0],
    ));
    let first_content = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 10 140 Td (First page) Tj ET".to_vec(),
    ));
    let second_content = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 10 140 Td (Second page) Tj ET q 40 0 0 20 20 30 cm /Im0 Do Q".to_vec(),
    ));
    let first_page = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => first_content,
    });
    let second_page = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "XObject" => dictionary! { "Im0" => image_id },
        },
        "Contents" => second_content,
    });
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(first_page), Object::Reference(second_page)],
            "Count" => 2,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn text_pdf(pages: &[(&str, i64)]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let page_count = i64::try_from(pages.len())?;
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let mut page_ids = Vec::new();
    for (text, y) in pages {
        let content = format!("BT /F1 12 Tf 10 {y} Td ({text}) Tj ET");
        let content_id = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        page_ids.push(document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => page_tree_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 160.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            "Contents" => content_id,
        }));
    }
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => page_count,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}
