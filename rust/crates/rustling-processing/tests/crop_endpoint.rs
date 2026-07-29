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

/// `PDFium` rebuilds `/Font`, `/ExtGState`, and `/XObject` when it regenerates a
/// page, but leaves `/Pattern` and `/Shading` alone — and the crop rebuild copies
/// the page's `/Resources` verbatim into the new Form `XObject`. An out-of-crop mark
/// painted with a tiling pattern or a shading therefore used to keep its whole
/// subtree reachable, so the pattern's text, the pattern's images, and a shading's
/// sampled-function data all stayed extractable from a file whose caller had asked
/// for them to be deleted.
#[tokio::test]
async fn removes_patterns_and_shadings_only_reachable_from_out_of_crop_marks()
-> Result<(), Box<dyn std::error::Error>> {
    for (label, source, secret) in [
        (
            "tiling pattern painting text",
            pdf_with_out_of_crop_pattern(PatternPayload::Text)?,
            b"PATTERNSECRET".as_slice(),
        ),
        (
            "tiling pattern painting an image",
            pdf_with_out_of_crop_pattern(PatternPayload::Image)?,
            b"PATIMGSECRETABCD".as_slice(),
        ),
        (
            "shading with a sampled function",
            pdf_with_out_of_crop_shading()?,
            b"SHADESAMPLESECRET".as_slice(),
        ),
    ] {
        let response = post_crop(
            "resources.pdf",
            &source,
            &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
        )
        .await?;
        if response.status() == StatusCode::NOT_IMPLEMENTED {
            if rustling_processing::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_some() {
                return Err(std::io::Error::other(
                    "configured PDFium runtime did not execute out-of-crop removal",
                )
                .into());
            }
            return Ok(());
        }
        let response = require_status(response, StatusCode::OK).await?;
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        assert!(
            !document_contains(&bytes, secret)?,
            "{label}: the out-of-crop resource survived"
        );
        assert!(
            document_contains(&bytes, b"KEEPME")?,
            "{label}: the in-crop text was removed too"
        );
    }
    Ok(())
}

/// Resource names are scope-local, so the keep-set has to be keyed by the object a
/// name resolves to, not by the name itself. Here the page's `/Pattern /P0` is the
/// secret and only an out-of-crop mark paints it, while a surviving in-crop Form
/// `XObject` carries its own, different `/P0`. A name-keyed keep-set sees "P0"
/// referenced from inside the form and retains the page's unrelated entry — the
/// same privacy-promise violation the pruning exists to close. Sequential names
/// (`/P0`, `/P1`) collide between a page and its forms routinely.
#[tokio::test]
async fn resolves_pattern_names_per_scope_so_a_collision_cannot_retain_a_secret()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "collide.pdf",
        &pdf_with_colliding_pattern_names()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&bytes, b"COINCIDESECRET")?,
        "the page's out-of-crop pattern was retained because a form referenced the \
         same resource NAME"
    );
    assert!(
        document_contains(&bytes, b"HARMLESSMARK")?,
        "the in-crop form's own pattern was pruned"
    );
    assert!(
        document_contains(&bytes, b"KEEPME")?,
        "in-crop text was removed"
    );
    Ok(())
}

/// A Form `XObject` without its own `/Resources` inherits the enclosing scope
/// (ISO 32000-1 §8.10.1). Treating such a form as having no resources loses the
/// chain — its own `Do` targets stop resolving — and then prunes a pattern the
/// surviving content still paints with, leaving a dangling name and visibly
/// corrupting a spec-valid file. Nothing here is out of crop except the marker
/// text, so this exercises the pruning alone.
#[tokio::test]
async fn honours_form_resource_inheritance_through_a_nested_chain()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "inherit.pdf",
        &pdf_with_inherited_form_resources()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&bytes, b"LIVEPATTERN")?,
        "a pattern painted through an inheriting nested form chain was pruned"
    );
    // The pattern must still be reachable by name, not merely present as bytes:
    // a surviving `scn` naming a resource no dictionary declares is a corrupt file.
    let document = Document::load_mem(&bytes)?;
    assert!(
        document.objects.values().any(|object| object
            .as_dict()
            .is_ok_and(|dictionary| dictionary.get(b"Pattern").is_ok())
            || object.as_stream().is_ok_and(|stream| stream
                .dict
                .get(b"Resources")
                .ok()
                .and_then(|resources| document.dereference(resources).ok())
                .and_then(|(_, resources)| resources.as_dict().ok().cloned())
                .is_some_and(|resources| resources.get(b"Pattern").is_ok()))),
        "the surviving pattern is no longer declared in any resource dictionary"
    );
    Ok(())
}

/// Page `/Pattern /P0` is the secret, painted only out of crop. An in-crop Form
/// `XObject` declares its OWN `/P0` and paints with that.
fn pdf_with_colliding_pattern_names() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let secret_pattern_id = document.add_object(tiling_pattern(font_id, b"COINCIDESECRET"));
    let form_pattern_id = document.add_object(tiling_pattern(font_id, b"HARMLESSMARK"));
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 80.into(), 40.into()],
            "Resources" => dictionary! {
                "Pattern" => dictionary! { "P0" => form_pattern_id },
            },
        },
        b"q /Pattern cs /P0 scn 0 0 80 40 re f Q".to_vec(),
    ));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 20 40 Td (KEEPME) Tj ET\n\
          q /Pattern cs /P0 scn 20 200 160 80 re f Q\n\
          q 1 0 0 1 20 10 cm /Fx Do Q"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => secret_pattern_id },
            "XObject" => dictionary! { "Fx" => form_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// In-crop Form A (no `/Resources`) invokes Form B (no `/Resources`), which paints
/// with the page's inherited `/P0`.
fn pdf_with_inherited_form_resources() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"LIVEPATTERN"));
    let inheriting_form = |content: Vec<u8>| {
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "FormType" => 1,
                "BBox" => vec![0.into(), 0.into(), 80.into(), 40.into()],
            },
            content,
        )
    };
    let inner_form_id = document.add_object(inheriting_form(
        b"q /Pattern cs /P0 scn 0 0 80 40 re f Q".to_vec(),
    ));
    let outer_form_id = document.add_object(inheriting_form(b"q /FormB Do Q".to_vec()));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 20 40 Td (KEEPME) Tj ET\nq 1 0 0 1 20 10 cm /FormA Do Q".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
            "XObject" => dictionary! { "FormA" => outer_form_id, "FormB" => inner_form_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

fn tiling_pattern(font_id: lopdf::ObjectId, payload: &[u8]) -> Stream {
    let mut content = b"BT /F1 8 Tf 1 1 Td (".to_vec();
    content.extend_from_slice(payload);
    content.extend_from_slice(b") Tj ET");
    Stream::new(
        dictionary! {
            "Type" => "Pattern",
            "PatternType" => 1,
            "PaintType" => 1,
            "TilingType" => 1,
            "BBox" => vec![0.into(), 0.into(), 40.into(), 20.into()],
            "XStep" => 40,
            "YStep" => 20,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        content,
    )
}

/// A Form `XObject` whose `/Resources` is an indirect reference to the page's own
/// resource dictionary is spec-legal and real generators emit it. Writing a
/// filtered *copy* of the resources onto the page leaves the form still pointing
/// at the unfiltered original, so the dead pattern stays reachable and
/// `prune_objects` rightly keeps it. The dictionary the holders actually point at
/// has to be the one that gets filtered.
#[tokio::test]
async fn prunes_dead_entries_from_resources_shared_by_indirect_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "shared.pdf",
        &pdf_with_shared_indirect_resources()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&bytes, b"SHAREDRESSECRET")?,
        "the dead pattern stayed reachable through the shared resource dictionary"
    );
    assert!(
        document_contains(&bytes, b"SHAREDRESPAT")?,
        "the live pattern was pruned"
    );
    // Every surviving holder must see the same filtered dictionary: no dictionary
    // anywhere may still list the dead entry.
    let document = Document::load_mem(&bytes)?;
    assert!(
        !document.objects.values().any(|object| object
            .as_dict()
            .is_ok_and(|dictionary| dictionary_lists_pattern(dictionary, b"Psec"))),
        "a resource dictionary still lists the pruned pattern"
    );
    Ok(())
}

fn dictionary_lists_pattern(dictionary: &lopdf::Dictionary, name: &[u8]) -> bool {
    dictionary
        .get(b"Pattern")
        .ok()
        .and_then(|patterns| patterns.as_dict().ok())
        .is_some_and(|patterns| patterns.get(name).is_ok())
}

/// Deeply nested resource scopes must be walked, not bailed on. The previous
/// revision stopped at 32 and kept every entry, so a 31-deep chain in a 6.9 KB
/// file silently returned the secret with a clean `200`.
#[tokio::test]
async fn walks_deeply_nested_resource_scopes_instead_of_giving_up()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "deep.pdf",
        &pdf_with_nested_form_scopes(40)?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&bytes, b"DEEPSECRET")?,
        "a nested scope chain made the walk give up and keep the secret"
    );
    assert!(
        document_contains(&bytes, b"KEEPME")?,
        "in-crop text was removed"
    );
    Ok(())
}

/// When the walk cannot establish what is live it must fail loudly rather than
/// guess in either direction. Keeping everything hands back a `200` and a file
/// that still holds the data the caller asked to delete; pruning anyway leaves a
/// `scn` naming a resource no dictionary declares. Both are undetectable to the
/// caller, so the request errors and no file is produced.
#[tokio::test]
async fn rejects_the_request_when_the_resource_walk_cannot_prove_what_is_live()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "undecodable.pdf",
        &pdf_with_undecodable_form_content()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains("removeDataOutsideCrop=false"),
        "the error must tell the caller how to proceed: {body}"
    );
    assert!(
        !body.starts_with("%PDF"),
        "a file must not be produced when the guarantee cannot be met"
    );
    Ok(())
}

/// Scaling guard for the resource walk.
///
/// Carrying scopes by value made this quadratic in time and memory — 4000
/// inheriting forms cost 15.9 s and 1.8 GB of resident memory on the reference
/// host, versus 1.9 s and no measurable growth once scopes were carried by
/// identity. The budget is deliberately loose so ordinary CI noise cannot trip it
/// while a return of the quadratic behaviour still would.
#[tokio::test]
async fn resource_walk_stays_linear_in_the_number_of_inheriting_forms()
-> Result<(), Box<dyn std::error::Error>> {
    let source = pdf_with_inheriting_forms(4_000)?;
    let started = std::time::Instant::now();
    let response = post_crop(
        "scaling.pdf",
        &source,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "300")],
    )
    .await?;
    let elapsed = started.elapsed();
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    require_status(response, StatusCode::OK).await?;
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "4000 inheriting forms took {elapsed:?}; the walk is no longer linear"
    );
    Ok(())
}

/// A Type 3 font's glyph procedures are content streams, and a Type 3 font
/// without its own `/Resources` resolves names against the page (ISO 32000-1
/// §9.6.5). A walk that only follows `Do` never sees them, so a pattern painted
/// only by a glyph looked dead: it was pruned while the glyph procedure kept
/// emitting `/P0 scn`, leaving a dangling name and a corrupt page.
#[tokio::test]
async fn follows_type3_glyph_procedures_when_deciding_what_is_live()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "type3-pattern.pdf",
        &pdf_with_type3_glyph_painting_a_pattern()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&bytes, b"T3LIVEPAT")?,
        "a pattern painted by a Type 3 glyph procedure was pruned"
    );
    Ok(())
}

/// A soft mask paints a form `XObject` to derive the mask, so that form's content
/// keeps whatever it references alive even though nothing invokes it with `Do`.
#[tokio::test]
async fn follows_soft_mask_groups_when_deciding_what_is_live()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "smask.pdf",
        &pdf_with_soft_mask_group_painting_a_pattern()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&bytes, b"SMASKLIVEPAT")?,
        "a pattern painted by a soft-mask group was pruned"
    );
    Ok(())
}

/// A Type 3 font whose glyph procedure paints the page's inherited pattern, with
/// the text drawn inside the crop rectangle.
fn pdf_with_type3_glyph_painting_a_pattern() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"T3LIVEPAT"));
    let glyph_id = document.add_object(Stream::new(
        dictionary! {},
        b"20 0 0 0 20 20 d1 /Pattern cs /P0 scn 0 0 20 20 re f".to_vec(),
    ));
    let char_procs_id = document.add_object(dictionary! { "S" => glyph_id });
    let encoding_id = document.add_object(dictionary! {
        "Type" => "Encoding",
        "Differences" => vec![83.into(), Object::Name(b"S".to_vec())],
    });
    // Deliberately no `/Resources`: the glyph inherits the page's.
    let type3_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type3",
        "FontBBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        "FontMatrix" => vec![
            Object::Real(0.05), 0.into(), 0.into(), Object::Real(0.05), 0.into(), 0.into(),
        ],
        "CharProcs" => char_procs_id,
        "Encoding" => encoding_id,
        "FirstChar" => 83,
        "LastChar" => 83,
        "Widths" => vec![20.into()],
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /T3 12 Tf 20 30 Td (SSS) Tj ET\nBT /F1 10 Tf 20 60 Td (KEEPME) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id, "T3" => type3_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// An `/ExtGState` soft mask whose group form paints the page's pattern.
fn pdf_with_soft_mask_group_painting_a_pattern() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"SMASKLIVEPAT"));
    let group_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
            "Group" => dictionary! { "S" => "Transparency", "CS" => "DeviceGray" },
        },
        b"q /Pattern cs /P0 scn 0 0 60 30 re f Q".to_vec(),
    ));
    let state_id = document.add_object(dictionary! {
        "Type" => "ExtGState",
        "SMask" => dictionary! { "S" => "Luminosity", "G" => group_id },
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"q /GS0 gs 0 0 1 rg 10 10 60 30 re f Q\n\
          BT /F1 10 Tf 20 60 Td (KEEPME) Tj ET"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
            "ExtGState" => dictionary! { "GS0" => state_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// A long chain of Form `XObjects` that each inherit their enclosing scope must not
/// be walked by recursing once per link: inheriting forms do not grow the scope
/// chain, so the scope-depth bound never fires and the recursion depth is limited
/// only by how many forms the file contains. That is a stack overflow — the same
/// remote process kill this endpoint was already fixed for once.
#[tokio::test]
async fn walks_a_long_inheriting_form_chain_without_exhausting_the_stack()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "chain.pdf",
        &pdf_with_inheriting_form_chain(6_000)?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    // Either outcome is acceptable — the walk may report that the document
    // exceeds its bounds — but the process must survive to answer at all.
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "unexpected status {}",
        response.status()
    );
    Ok(())
}

/// `depth` Form `XObjects` nested one inside the next, none of them carrying
/// `/Resources`, so every link inherits the page scope.
fn pdf_with_inheriting_form_chain(depth: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"CHAINPAT"));
    let mut xobjects = dictionary! {};
    let mut inner_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 4.into(), 2.into()],
        },
        b"q /Pattern cs /P0 scn 0 0 4 2 re f Q".to_vec(),
    ));
    xobjects.set(b"L0".to_vec(), inner_id);
    for level in 1..=depth {
        let previous = format!("L{}", level - 1);
        inner_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
                "BBox" => vec![0.into(), 0.into(), 4.into(), 2.into()],
            },
            format!("q /{previous} Do Q").into_bytes(),
        ));
        xobjects.set(format!("L{level}").into_bytes(), inner_id);
    }
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        format!("BT /F1 10 Tf 20 40 Td (KEEPME) Tj ET\nq 1 0 0 1 20 10 cm /L{depth} Do Q")
            .into_bytes(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
            "XObject" => xobjects,
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// A page and a Form `XObject` that share one indirect `/Resources` dictionary
/// holding both a live pattern and a dead one.
fn pdf_with_shared_indirect_resources() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let resources_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let live_id = document.add_object(tiling_pattern(font_id, b"SHAREDRESPAT"));
    let dead_id = document.add_object(tiling_pattern(font_id, b"SHAREDRESSECRET"));
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
            // The very dictionary the page uses, by reference.
            "Resources" => Object::Reference(resources_id),
        },
        b"q /Pattern cs /Plive scn 0 0 60 30 re f Q".to_vec(),
    ));
    document.objects.insert(
        resources_id,
        Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "Plive" => live_id, "Psec" => dead_id },
            "XObject" => dictionary! { "Fx" => form_id },
        }),
    );
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 10 30 Td (KEEPME) Tj ET\n\
          q 1 0 0 1 5 5 cm /Fx Do Q\n\
          q /Pattern cs /Psec scn 20 200 160 80 re f Q"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => Object::Reference(resources_id),
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// `depth` Form `XObjects` nested one inside the next, each carrying its own
/// `/Resources`, with the page's pattern painted only by an out-of-crop mark.
fn pdf_with_nested_form_scopes(depth: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let secret_id = document.add_object(tiling_pattern(font_id, b"DEEPSECRET"));
    let leaf_pattern_id = document.add_object(tiling_pattern(font_id, b"DEEPLEAF"));
    let mut inner_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 40.into(), 20.into()],
            "Resources" => dictionary! {
                "Pattern" => dictionary! { "P0" => leaf_pattern_id },
            },
        },
        b"q /Pattern cs /P0 scn 0 0 40 20 re f Q".to_vec(),
    ));
    for _ in 0..depth {
        inner_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "FormType" => 1,
                "BBox" => vec![0.into(), 0.into(), 40.into(), 20.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Fi" => inner_id } },
            },
            b"q /Fi Do Q".to_vec(),
        ));
    }
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 20 40 Td (KEEPME) Tj ET\n\
          q /Pattern cs /P0 scn 20 200 160 80 re f Q\n\
          q 1 0 0 1 20 10 cm /Ftop Do Q"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => secret_id },
            "XObject" => dictionary! { "Ftop" => inner_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// A surviving in-crop form whose content stream cannot be parsed, on a page that
/// carries a pattern — so the walk cannot decide what the form paints with.
fn pdf_with_undecodable_form_content() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"UNDECODABLEPAT"));
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 40.into(), 20.into()],
        },
        // A truncated inline image: the content parser cannot recover past it, so
        // whether this form paints `/P0` is unknowable. (`Content::decode` is very
        // tolerant — it accepts unterminated strings and binary garbage — so this
        // is close to the only shape that genuinely defeats it.)
        b"q /Pattern cs /P0 scn BI /W 4 /H 4 ID abc".to_vec(),
    ));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 20 40 Td (KEEPME) Tj ET\nq 1 0 0 1 20 10 cm /Fx Do Q".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
            "XObject" => dictionary! { "Fx" => form_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// `count` distinct Form `XObjects` with no `/Resources` of their own, each painting
/// the page's inherited pattern.
fn pdf_with_inheriting_forms(count: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"LADDERPAT"));
    let mut xobjects = dictionary! {};
    let mut content = Vec::new();
    for index in 0..count {
        let form_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "FormType" => 1,
                "BBox" => vec![0.into(), 0.into(), 4.into(), 2.into()],
            },
            b"q /Pattern cs /P0 scn 0 0 4 2 re f Q".to_vec(),
        ));
        let name = format!("Fm{index}");
        xobjects.set(name.clone().into_bytes(), form_id);
        content.extend_from_slice(
            format!(
                "q 1 0 0 1 {} {} cm /{name} Do Q\n",
                2 + (index % 40),
                2 + ((index / 40) % 20)
            )
            .as_bytes(),
        );
    }
    content.extend_from_slice(b"BT /F1 10 Tf 20 40 Td (KEEPME) Tj ET");
    let content_id = document.add_object(Stream::new(dictionary! {}, content));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
            "XObject" => xobjects,
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// Pins a real fidelity limitation of the removal path, and the escape hatch.
///
/// `FPDFPage_GenerateContent` does not round-trip pattern or shading marks: a
/// pattern fill comes back as a flat colour and an `sh` mark is dropped entirely.
/// That happens inside `PDFium`, before any resource pruning, so on a page that had
/// a removal the pattern/shading is already unreferenced and its bytes go too.
/// `removeDataOutsideCrop=false` never regenerates content, so it preserves both
/// the marks and their resources exactly — which is what makes this a documented
/// trade-off of asking for deletion rather than a silent loss.
#[tokio::test]
async fn removal_path_loses_pattern_marks_that_clip_only_preserves()
-> Result<(), Box<dyn std::error::Error>> {
    let source = pdf_with_in_crop_pattern()?;
    let coordinates = [("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")];

    let mut clip_only = coordinates.to_vec();
    clip_only.push(("removeDataOutsideCrop", "false"));
    let clipped = require_status(
        post_crop("kept-pattern.pdf", &source, &clip_only).await?,
        StatusCode::OK,
    )
    .await?;
    let clipped = to_bytes(clipped.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&clipped, b"KEPTPATTERNPAINT")?,
        "clip-only must preserve a pattern an in-crop mark paints with"
    );

    let removed = post_crop("kept-pattern.pdf", &source, &coordinates).await?;
    if removed.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let removed = require_status(removed, StatusCode::OK).await?;
    let removed = to_bytes(removed.into_body(), usize::MAX).await?;
    // Documented limitation, asserted so a future PDFium upgrade that starts
    // round-tripping patterns makes this test fail loudly rather than silently
    // leaving the contract stale.
    assert!(
        !document_contains(&removed, b"KEPTPATTERNPAINT")?,
        "PDFium now preserves pattern marks through content regeneration — update \
         the crop contract, which documents that it does not"
    );
    assert!(
        !document_contains(&removed, b"DROPME")?,
        "the out-of-crop text must still be removed"
    );
    Ok(())
}

/// A page whose only pattern-painted mark is INSIDE the crop rectangle.
fn pdf_with_in_crop_pattern() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "Pattern",
            "PatternType" => 1,
            "PaintType" => 1,
            "TilingType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 20.into()],
            "XStep" => 60,
            "YStep" => 20,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        b"BT /F1 8 Tf 2 2 Td (KEPTPATTERNPAINT) Tj ET".to_vec(),
    ));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"/Pattern cs /P0 scn 20 20 100 30 re f\nBT /F1 10 Tf 20 250 Td (DROPME) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

#[derive(Clone, Copy)]
enum PatternPayload {
    Text,
    Image,
}

/// A page whose in-crop text is plain, and whose only out-of-crop mark is a
/// rectangle filled with a tiling pattern. The pattern's own content carries the
/// secret, so nothing but the pattern subtree can leak it.
fn pdf_with_out_of_crop_pattern(
    payload: PatternPayload,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = match payload {
        PatternPayload::Text => document.add_object(Stream::new(
            dictionary! {
                "Type" => "Pattern",
                "PatternType" => 1,
                "PaintType" => 1,
                "TilingType" => 1,
                "BBox" => vec![0.into(), 0.into(), 60.into(), 20.into()],
                "XStep" => 60,
                "YStep" => 20,
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            b"BT /F1 8 Tf 2 2 Td (PATTERNSECRET) Tj ET".to_vec(),
        )),
        PatternPayload::Image => {
            let image_id = document.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => 4,
                    "Height" => 4,
                    "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
                    "BitsPerComponent" => 8,
                },
                b"PATIMGSECRETABCD".to_vec(),
            ));
            document.add_object(Stream::new(
                dictionary! {
                    "Type" => "Pattern",
                    "PatternType" => 1,
                    "PaintType" => 1,
                    "TilingType" => 1,
                    "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
                    "XStep" => 20,
                    "YStep" => 20,
                    "Resources" => dictionary! {
                        "XObject" => dictionary! { "PIm" => image_id },
                    },
                },
                b"q 20 0 0 20 0 0 cm /PIm Do Q".to_vec(),
            ))
        }
    };
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"/Pattern cs /P0 scn 20 250 100 30 re f\nBT /F1 10 Tf 20 40 Td (KEEPME) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// A page whose only out-of-crop mark is an `sh` shading whose Type 0 function
/// carries the secret in its sample stream.
fn pdf_with_out_of_crop_shading() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let samples = b"SHADESAMPLESECRET";
    let function_id = document.add_object(Stream::new(
        dictionary! {
            "FunctionType" => 0,
            "Domain" => vec![0.into(), 1.into()],
            "Range" => vec![0.into(), 1.into()],
            "Size" => vec![i64::try_from(samples.len())?.into()],
            "BitsPerSample" => 8,
        },
        samples.to_vec(),
    ));
    let shading_id = document.add_object(dictionary! {
        "ShadingType" => 2,
        "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
        "Coords" => vec![20.into(), 240.into(), 180.into(), 290.into()],
        "Function" => function_id,
        "Extend" => vec![Object::Boolean(true), Object::Boolean(true)],
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"q 20 240 160 50 re W n /Sh0 sh Q\nBT /F1 10 Tf 20 40 Td (KEEPME) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Shading" => dictionary! { "Sh0" => shading_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

fn finish_single_page(
    mut document: Document,
    root_pages_id: lopdf::ObjectId,
    page_id: lopdf::ObjectId,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    document.objects.insert(
        root_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

/// A Type 3 font text run outside the crop rectangle used to take the whole
/// process down with SIGSEGV: the scratch page that re-homes removed objects was
/// re-fetched per removal, which reset its content-regeneration strategy in
/// `pdfium-render`'s shared cache, so every `add_object` ran
/// `FPDFPage_GenerateContent` over it and `PDFium` crashed in
/// `UpdateResourcesDict`. The endpoint is unauthenticated, so that was a
/// remote-triggerable denial of service for every other caller in the process.
#[tokio::test]
async fn removes_type3_font_text_outside_the_crop_without_crashing()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "type3.pdf",
        &pdf_with_type3_text_outside_the_crop()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        if rustling_processing::env_compat::var_os("RUSTLING_PDFIUM_LIBRARY_PATH").is_some() {
            return Err(std::io::Error::other(
                "configured PDFium runtime did not execute out-of-crop removal",
            )
            .into());
        }
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&bytes, b"SSS")?,
        "the out-of-crop Type 3 text survived"
    );
    Ok(())
}

/// One page whose only mark is a Type 3 font text run placed above the crop
/// rectangle, with a glyph procedure and the full Type 3 dictionary shape
/// (`/CharProcs`, `/Encoding`, `/FontMatrix`, `/FontBBox`) `PDFium` walks when it
/// rebuilds a page's resources.
fn pdf_with_type3_text_outside_the_crop() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let glyph_id = document.add_object(Stream::new(
        dictionary! {},
        b"20 0 0 0 20 20 d1 0 0 20 20 re f".to_vec(),
    ));
    let char_procs_id = document.add_object(dictionary! { "S" => glyph_id });
    let encoding_id = document.add_object(dictionary! {
        "Type" => "Encoding",
        "Differences" => vec![83.into(), Object::Name(b"S".to_vec())],
    });
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type3",
        "FontBBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        "FontMatrix" => vec![
            Object::Real(0.05), 0.into(), 0.into(), Object::Real(0.05), 0.into(), 0.into(),
        ],
        "CharProcs" => char_procs_id,
        "Encoding" => encoding_id,
        "FirstChar" => 83,
        "LastChar" => 83,
        "Widths" => vec![20.into()],
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /T3 12 Tf 20 250 Td (SSS) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "T3" => font_id } },
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
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
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
