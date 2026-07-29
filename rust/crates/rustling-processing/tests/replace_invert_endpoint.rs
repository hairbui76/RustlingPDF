use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use lopdf::{Document, Object, ObjectId, Stream, content::Content, dictionary};
use rustling_processing::app;
use tower::ServiceExt;

#[tokio::test]
async fn full_inversion_rebuilds_pages_as_images() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &single_text_page()?,
        &[("replaceAndInvertOption", "FULL_INVERSION")],
    )
    .await?;
    if !native_pdfium_requested() {
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()?
            .contains("source_inverted.pdf")
    );
    let output = Document::load_mem(&response_bytes(response).await?)?;
    let page = output.get_dictionary(first_page_id(&output)?)?;
    assert_eq!(page.get(b"MediaBox")?.as_array()?.len(), 4);
    let resources = resolve_dictionary(&output, page.get(b"Resources")?)?;
    let xobjects = resolve_dictionary(&output, resources.get(b"XObject")?)?;
    assert!(xobjects.iter().any(|(_, object)| {
        output
            .dereference(object)
            .ok()
            .and_then(|(_, object)| object.as_stream().ok())
            .and_then(|stream| stream.dict.get(b"Subtype").ok())
            .is_some_and(|subtype| subtype.as_name().is_ok_and(|name| name == b"Image"))
    }));
    assert!(output.extract_text(&[1])?.trim().is_empty());
    Ok(())
}

#[tokio::test]
async fn high_contrast_mode_recolors_text_and_prepends_background()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &single_text_page()?,
        &[
            ("replaceAndInvertOption", "HIGH_CONTRAST_COLOR"),
            ("highContrastColorCombination", "YELLOW_TEXT_ON_BLACK"),
        ],
    )
    .await?;
    let output = Document::load_mem(
        &response_bytes(require_status(response, StatusCode::OK).await?).await?,
    )?;
    assert_eq!(output.extract_text(&[1])?.trim(), "Selectable text");
    assert_recolor_operations(&output, [1.0, 1.0, 0.0], [0.0, 0.0, 0.0])?;
    Ok(())
}

#[tokio::test]
async fn custom_color_mode_accepts_java_color_values() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &single_text_page()?,
        &[
            ("replaceAndInvertOption", "CUSTOM_COLOR"),
            ("textColor", "#112233"),
            ("backGroundColor", "0xAABBCC"),
        ],
    )
    .await?;
    let output = Document::load_mem(
        &response_bytes(require_status(response, StatusCode::OK).await?).await?,
    )?;
    assert_recolor_operations(
        &output,
        [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0],
        [170.0 / 255.0, 187.0 / 255.0, 204.0 / 255.0],
    )?;
    Ok(())
}

#[tokio::test]
async fn custom_color_mode_rejects_missing_colors() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &single_text_page()?,
        &[("replaceAndInvertOption", "CUSTOM_COLOR")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn custom_color_mode_recolors_nested_form_text() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &form_text_page()?,
        &[
            ("replaceAndInvertOption", "CUSTOM_COLOR"),
            ("textColor", "#336699"),
            ("backGroundColor", "#FFFFFF"),
        ],
    )
    .await?;
    let output = Document::load_mem(
        &response_bytes(require_status(response, StatusCode::OK).await?).await?,
    )?;
    let page = output.get_dictionary(first_page_id(&output)?)?;
    let resources = resolve_dictionary(&output, page.get(b"Resources")?)?;
    let xobjects = resolve_dictionary(&output, resources.get(b"XObject")?)?;
    let form_id = xobjects.get(b"Fm1")?.as_reference()?;
    let form = output.get_object(form_id)?.as_stream()?;
    let content = Content::decode(&form.decompressed_content()?)?;
    let text_index = content
        .operations
        .iter()
        .position(|operation| operation.operator == "Tj")
        .ok_or("missing Form Tj operation")?;
    assert_eq!(content.operations[text_index - 1].operator, "rg");
    assert_rgb(
        &content.operations[text_index - 1].operands,
        [51.0 / 255.0, 102.0 / 255.0, 153.0 / 255.0],
    )?;
    Ok(())
}

/// `COLOR_SPACE_CONVERSION` is pure Rust: it must succeed with no external tool and
/// must actually turn the page's device colours into `DeviceCMYK` operators.
#[tokio::test]
async fn color_space_conversion_rewrites_device_colors_as_cmyk()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &device_color_page()?,
        &[("replaceAndInvertOption", "COLOR_SPACE_CONVERSION")],
    )
    .await?;
    let response = require_status(response, StatusCode::OK).await?;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    let document = Document::load_mem(&response_bytes(response).await?)?;
    let page_id = first_page_id(&document)?;
    let content = Content::decode(&document.get_page_content(page_id))?;
    let operators = content
        .operations
        .iter()
        .map(|operation| operation.operator.as_str())
        .collect::<Vec<_>>();
    assert!(!operators.contains(&"rg"), "{operators:?}");
    assert!(!operators.contains(&"g"), "{operators:?}");
    assert!(!operators.contains(&"RG"), "{operators:?}");
    assert!(operators.contains(&"k"), "{operators:?}");
    assert!(operators.contains(&"K"), "{operators:?}");
    // Pure red becomes 0 1 1 0 under the ISO 32000-1 device conversion.
    let red = content
        .operations
        .iter()
        .find(|operation| operation.operator == "k")
        .ok_or("no non-stroking CMYK operator")?;
    assert_component(&red.operands, [0.0, 1.0, 1.0, 0.0])?;
    Ok(())
}

/// A `DeviceRGB` image must come back as an 8-bit `DeviceCMYK` image with the
/// converted samples, and its soft mask must keep its own gray colour space.
#[tokio::test]
async fn color_space_conversion_rewrites_rgb_images_and_keeps_soft_masks()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &rgb_image_page()?,
        &[("replaceAndInvertOption", "COLOR_SPACE_CONVERSION")],
    )
    .await?;
    let response = require_status(response, StatusCode::OK).await?;
    let document = Document::load_mem(&response_bytes(response).await?)?;
    let (image, mask) = image_and_mask(&document)?;
    assert_eq!(image.dict.get(b"ColorSpace")?.as_name()?, b"DeviceCMYK");
    assert_eq!(image.dict.get(b"BitsPerComponent")?.as_i64()?, 8);
    // Red, green, blue, white -> CMYK bytes.
    assert_eq!(
        image.decompressed_content()?,
        vec![
            0, 255, 255, 0, // red
            255, 0, 255, 0, // green
            255, 255, 0, 0, // blue
            0, 0, 0, 0, // white
        ]
    );
    assert_eq!(mask.dict.get(b"ColorSpace")?.as_name()?, b"DeviceGray");
    Ok(())
}

/// A page already in `DeviceCMYK` must round-trip unchanged.
#[tokio::test]
async fn color_space_conversion_leaves_existing_cmyk_alone()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(
        &cmyk_color_page()?,
        &[("replaceAndInvertOption", "COLOR_SPACE_CONVERSION")],
    )
    .await?;
    let response = require_status(response, StatusCode::OK).await?;
    let document = Document::load_mem(&response_bytes(response).await?)?;
    let page_id = first_page_id(&document)?;
    let content = Content::decode(&document.get_page_content(page_id))?;
    let cmyk = content
        .operations
        .iter()
        .find(|operation| operation.operator == "k")
        .ok_or("no CMYK operator")?;
    assert_component(&cmyk.operands, [0.1, 0.2, 0.3, 0.4])?;
    Ok(())
}

#[tokio::test]
async fn rejects_an_unknown_option() -> Result<(), Box<dyn std::error::Error>> {
    let response =
        post_replace_invert(&single_text_page()?, &[("replaceAndInvertOption", "SEPIA")]).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn requires_the_option_field() -> Result<(), Box<dyn std::error::Error>> {
    let response = post_replace_invert(&single_text_page()?, &[]).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

fn native_pdfium_requested() -> bool {
    rustling_processing::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_some()
}

async fn response_bytes(response: Response) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(to_bytes(response.into_body(), usize::MAX).await?.to_vec())
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

async fn post_replace_invert(
    pdf: &[u8],
    fields: &[(&str, &str)],
) -> Result<Response, Box<dyn std::error::Error>> {
    let boundary = "stirling-replace-invert-boundary";
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
    Ok(app(1024 * 1024)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/misc/replace-invert-pdf")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?)
}

fn first_page_id(document: &Document) -> Result<ObjectId, Box<dyn std::error::Error>> {
    document
        .get_pages()
        .into_values()
        .next()
        .ok_or_else(|| std::io::Error::other("PDF has no pages").into())
}

fn resolve_dictionary<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<&'a lopdf::Dictionary, lopdf::Error> {
    let (_, object) = document.dereference(object)?;
    object.as_dict()
}

fn assert_recolor_operations(
    document: &Document,
    text_color: [f32; 3],
    background_color: [f32; 3],
) -> Result<(), Box<dyn std::error::Error>> {
    let page_id = first_page_id(document)?;
    let content = Content::decode(&document.get_page_content(page_id))?;
    assert_eq!(
        content
            .operations
            .iter()
            .take(5)
            .map(|operation| operation.operator.as_str())
            .collect::<Vec<_>>(),
        ["q", "rg", "re", "f", "Q"]
    );
    assert_rgb(&content.operations[1].operands, background_color)?;
    let text_index = content
        .operations
        .iter()
        .position(|operation| operation.operator == "Tj")
        .ok_or("missing Tj operation")?;
    assert!(text_index > 0);
    assert_eq!(content.operations[text_index - 1].operator, "rg");
    assert_rgb(&content.operations[text_index - 1].operands, text_color)?;
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn assert_rgb(operands: &[Object], expected: [f32; 3]) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(operands.len(), 3);
    for (operand, expected) in operands.iter().zip(expected) {
        let actual = match operand {
            Object::Integer(value) => *value as f32,
            Object::Real(value) => *value,
            _ => return Err("color operand is not numeric".into()),
        };
        assert!((actual - expected).abs() < 0.0001);
    }
    Ok(())
}

fn single_text_page() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 10 50 Td (Selectable text) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 80.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
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

fn form_text_page() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 80.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        b"BT /F1 12 Tf 10 50 Td (Nested text) Tj ET".to_vec(),
    ));
    let content_id = document.add_object(Stream::new(dictionary! {}, b"q /Fm1 Do Q".to_vec()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 80.into()],
        "Resources" => dictionary! { "XObject" => dictionary! { "Fm1" => form_id } },
        "Contents" => content_id,
    });
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn assert_component(
    operands: &[Object],
    expected: [f32; 4],
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = operands
        .iter()
        .map(|operand| match operand {
            Object::Real(value) => Ok(*value),
            Object::Integer(value) => Ok(*value as f32),
            other => Err(format!("non-numeric CMYK operand: {other:?}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(actual.len(), 4, "{actual:?}");
    for (index, expected) in expected.into_iter().enumerate() {
        assert!(
            (actual[index] - expected).abs() < 1e-4,
            "component {index}: {actual:?} != {expected:?}"
        );
    }
    Ok(())
}

fn image_and_mask(
    document: &Document,
) -> Result<(Stream, Stream), Box<dyn std::error::Error>> {
    let mut base = None;
    let mut mask = None;
    for object in document.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        if !stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|subtype| subtype == b"Image")
        {
            continue;
        }
        if stream.dict.get(b"SMask").is_ok() {
            base = Some(stream.clone());
        } else {
            mask = Some(stream.clone());
        }
    }
    Ok((
        base.ok_or("no base image in the converted PDF")?,
        mask.ok_or("no soft mask in the converted PDF")?,
    ))
}

fn single_page_document(
    content: Vec<u8>,
    resources: lopdf::Dictionary,
    extra: impl FnOnce(&mut Document),
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(dictionary! {}, content));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 80.into()],
        "Resources" => resources,
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
    extra(&mut document);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn device_color_page() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    single_page_document(
        b"1 0 0 rg 0 0 10 10 re f 0.5 G 0 0 m 10 10 l S           /CS0 cs 0 0 1 sc 20 20 10 10 re f"
            .to_vec(),
        dictionary! {
            "ColorSpace" => dictionary! { "CS0" => Object::Name(b"DeviceRGB".to_vec()) },
        },
        |_| {},
    )
}

fn cmyk_color_page() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    single_page_document(
        b"0.1 0.2 0.3 0.4 k 0 0 10 10 re f".to_vec(),
        dictionary! {},
        |_| {},
    )
}

fn rgb_image_page() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let mask = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 4,
            "Height" => 1,
            "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
            "BitsPerComponent" => 8,
        },
        vec![255, 255, 255, 255],
    );
    let mask_id = document.add_object(mask);
    let image = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 4,
            "Height" => 1,
            "ColorSpace" => Object::Name(b"DeviceRGB".to_vec()),
            "BitsPerComponent" => 8,
            "SMask" => Object::Reference(mask_id),
        },
        vec![
            255, 0, 0, // red
            0, 255, 0, // green
            0, 0, 255, // blue
            255, 255, 255, // white
        ],
    );
    let image_id = document.add_object(image);
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"q 40 0 0 40 10 10 cm /Im0 Do Q".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 80.into()],
        "Resources" => dictionary! {
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
