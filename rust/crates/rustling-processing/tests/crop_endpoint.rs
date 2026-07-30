use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fmt::Write as _,
};

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

/// When the walk cannot read a stream in full it must keep what that stream
/// might still name, and still return the cropped file.
///
/// Two earlier answers were both wrong. Pruning anyway leaves a `scn` naming a
/// resource no dictionary declares — silent destruction of the caller's document,
/// under a `200`, with no way to tell it from a clean one. Refusing with `422`
/// fails a document every viewer renders perfectly well, for a construct 3.6% of
/// real PDFs contain. Keeping the declarations leaves data outside the crop box
/// recoverable, which is the one outcome that is bounded and documented.
#[tokio::test]
async fn keeps_declarations_a_stream_it_cannot_parse_might_still_name()
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
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&bytes, b"UNDECODABLEPAT")?,
        "the pattern the unparsable form may still paint with was pruned"
    );
    assert!(
        document_contains(&bytes, b"KEEPME")?,
        "the in-crop text was removed"
    );
    assert_no_dangling_resource_names(&bytes)?;
    Ok(())
}

/// `lopdf`'s lenient content decoder stops at a `%` comment followed by a blank
/// line and reports success with everything after it missing. Round 7 read that
/// empty operator list as proof that the page painted with nothing, and deleted
/// `/Font` and `/XObject` from a page whose content stream — copied to the output
/// byte for byte — still named them. On a real 1.1 MB Canon scan that destroyed
/// 77% of the page's ink under a clean `200`.
///
/// Two shapes reach the same construct, and both are here because only one of
/// them is visible in the file: the comment and the blank line written into a
/// single stream, and a `/Contents` **array** whose first member ends in a
/// comment — where the blank line is inserted by `get_page_content` joining the
/// members, so no amount of reading the PDF shows it.
#[tokio::test]
async fn keeps_declarations_when_a_comment_truncates_the_content_parse()
-> Result<(), Box<dyn std::error::Error>> {
    for (label, source) in [
        (
            "comment and blank line in one stream",
            pdf_with_truncating_comment()?,
        ),
        (
            "contents array joined into a blank line",
            pdf_with_comment_only_first_content_stream()?,
        ),
    ] {
        let response = post_crop(
            "truncating.pdf",
            &source,
            &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "300")],
        )
        .await?;
        if response.status() == StatusCode::NOT_IMPLEMENTED {
            return Ok(());
        }
        let response = require_status(response, StatusCode::OK).await?;
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        // Nothing lies outside this crop rectangle, so PDFium regenerates no page
        // and the original content stream — with these exact names — is what
        // reaches the output. Round 7 returned it with both entries deleted.
        let document = Document::load_mem(&bytes)?;
        let declared = declared_resource_names(&document);
        for (category, name) in [
            (b"Font".as_slice(), b"F12".as_slice()),
            (b"XObject".as_slice(), b"Obj5".as_slice()),
        ] {
            assert!(
                declared
                    .get(category)
                    .is_some_and(|declared| declared.contains(name)),
                "{label}: /{} /{} was pruned from a page whose content stream still names it",
                String::from_utf8_lossy(category),
                String::from_utf8_lossy(name)
            );
        }
        assert!(
            document_contains(&bytes, b"VISIBLETEXT")?,
            "{label}: the in-crop text was removed"
        );
        assert_no_dangling_resource_names(&bytes)?;
    }
    Ok(())
}

/// A PDF value may always be written indirectly, and four places in the walk read
/// one as if it could not be. These are the four, as fixtures.
///
/// Each was found by an independent tester, and each is the same mistake: raw
/// `Dictionary::get` where the surrounding code used `Document::get_dictionary`,
/// which *does* follow reference chains. The walk contradicted itself about
/// indirection, and — because the checker read values the same way — the suite
/// stayed green through all of it.
#[tokio::test]
async fn follows_resources_written_as_indirect_references() -> Result<(), Box<dyn std::error::Error>>
{
    let mut failures = Vec::new();
    for (label, source, marker) in [
        // `/ExtGState` `/Font` is `[<type3> size]` behind a reference, so matching
        // on `Object::Array` without resolving never fired and the Type 3 glyph
        // procedures — and the pattern one of them paints — were never walked.
        (
            "ExtGState /Font as an indirect array",
            pdf_with_extgstate_font_indirect_array(false)?,
            b"EGFONTPAT".as_slice(),
        ),
        // The same document plus an unexecuted form declaring its own `/P0`. The
        // decoy changes nothing about the walk; it exists because the *previous*
        // checker keyed on "some dictionary somewhere declares this name", so the
        // decoy alone hid the over-deletion from it.
        (
            "the same, with a decoy declaration of the same name",
            pdf_with_extgstate_font_indirect_array(true)?,
            b"EGFONTPAT".as_slice(),
        ),
        // `/Subtype` behind a reference never equalled `Form`, so the form was
        // neither walked nor preserved: over-deletion by omission.
        (
            "Form XObject with an indirect /Subtype",
            pdf_with_form_with_indirect_subtype()?,
            b"INDIRECTSUBTYPEPAT".as_slice(),
        ),
        // `/Pattern` reached through reference -> reference. `category_dictionary`
        // resolved the chain and reported the alias id, `retain_live_entries` wrote
        // through that alias, and the protection keyed on it missed the object it
        // was supposed to protect.
        (
            "shared /Pattern behind a reference chain",
            pdf_with_alias_chain_to_shared_category()?,
            b"ALIASCATPAT".as_slice(),
        ),
    ] {
        let response = post_crop(
            "indirect.pdf",
            &source,
            &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "300")],
        )
        .await?;
        if response.status() == StatusCode::NOT_IMPLEMENTED {
            return Ok(());
        }
        let response = require_status(response, StatusCode::OK).await?;
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        let report = audit_resource_names(&bytes)?;
        // Collected rather than asserted case by case: when this regresses, which
        // of the four indirection shapes broke is the whole diagnosis, and
        // stopping at the first hides the other three.
        if !document_contains(&bytes, marker)? {
            failures.push(format!(
                "{label}: the resource the surviving content still paints with was deleted; \
                 the checker said {report:?}"
            ));
        } else if !report.is_clean() {
            failures.push(format!("{label}: {report:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

/// `/ExtGState` `/Font` written as an indirect reference to the `[<type3> size]`
/// array. With `decoy`, an unexecuted Form `XObject` also declares the name `/P0`.
fn pdf_with_extgstate_font_indirect_array(
    decoy: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"EGFONTPAT"));
    let glyph_id = document.add_object(Stream::new(
        dictionary! {},
        b"20 0 0 0 20 20 d1 q /Pattern cs /P0 scn 0 0 20 20 re f Q".to_vec(),
    ));
    let char_procs_id = document.add_object(dictionary! { "ga" => glyph_id });
    let encoding_id = document.add_object(dictionary! {
        "Type" => "Encoding",
        "Differences" => vec![83.into(), Object::Name(b"ga".to_vec())],
    });
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
    let font_array_id =
        document.add_object(Object::Array(vec![Object::Reference(type3_id), 12.into()]));
    let state_id = document.add_object(dictionary! {
        "Type" => "ExtGState",
        "Font" => Object::Reference(font_array_id),
    });
    let mut page_resources = dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "ExtGState" => dictionary! { "EG" => state_id },
        "Pattern" => dictionary! { "P0" => pattern_id },
    };
    let mut content = b"BT /F1 10 Tf 10 30 Td (KEEPME) Tj ET\n\
          q /EG gs BT 5 5 Td (S) Tj ET Q\n\
          BT /F1 8 Tf 20 250 Td (EGOUTOFCROP) Tj ET"
        .to_vec();
    if decoy {
        let decoy_pattern_id = document.add_object(tiling_pattern(font_id, b"DECOYPAT"));
        let decoy_form_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Resources" => dictionary! {
                    "Pattern" => dictionary! { "P0" => decoy_pattern_id },
                },
            },
            b"q /Pattern cs /P0 scn 0 0 10 10 re f Q".to_vec(),
        ));
        page_resources.set(
            "XObject",
            Object::Dictionary(dictionary! { "Decoy" => decoy_form_id }),
        );
        content.extend_from_slice(b"\nq 1 0 0 1 1 1 cm /Decoy Do Q");
    }
    let content_id = document.add_object(Stream::new(dictionary! {}, content));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => page_resources,
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// A Form `XObject` whose `/Subtype` is an indirect reference to `/Form`.
fn pdf_with_form_with_indirect_subtype() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"INDIRECTSUBTYPEPAT"));
    let subtype_id = document.add_object(Object::Name(b"Form".to_vec()));
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => Object::Reference(subtype_id),
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
        },
        b"q /Pattern cs /P0 scn 0 0 60 30 re f Q".to_vec(),
    ));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"q 1 0 0 1 10 10 cm /Fm1 Do Q\nBT /F1 10 Tf 20 60 Td (KEEPME) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
            "XObject" => dictionary! { "Fm1" => form_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// The shared-category fixture again, with one page reaching the `/Pattern`
/// dictionary through an intermediate object whose value is a reference to it.
fn pdf_with_alias_chain_to_shared_category() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let tile_id = document.add_object(tiling_pattern(font_id, b"ALIASCATPAT"));
    let shared_category_id = document.add_object(dictionary! { "Pkeep" => tile_id });
    let alias_id = document.add_object(Object::Reference(shared_category_id));
    let mut pages = Vec::new();
    for (content, pattern_ref) in [
        (
            b"% c\n\nq /Pattern cs /Pkeep scn 10 10 50 50 re f Q\n\
               BT /F1 10 Tf 10 120 Td (P1TEXT) Tj ET"
                .to_vec(),
            alias_id,
        ),
        (
            b"BT /F1 10 Tf 10 30 Td (P2TEXT) Tj ET".to_vec(),
            shared_category_id,
        ),
    ] {
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        pages.push(Object::Reference(document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => root_pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font_id },
                "Pattern" => Object::Reference(pattern_ref),
            },
            "Contents" => content_id,
        })));
    }
    document.objects.insert(
        root_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => pages, "Count" => 2,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

/// One unbalanced `(` used to consume the rest of the stream, hiding every
/// resource name after it from the oracle — a checker reporting clean because it
/// stopped looking.
#[test]
fn an_unbalanced_delimiter_does_not_blind_the_checker() {
    let blinded = b"BT (unterminated Tj ET q /Pattern cs /P0 scn 0 0 10 10 re f Q";
    let names = resource_names_used(blinded);
    assert!(
        names
            .iter()
            .any(|(category, name)| *category == b"Pattern" && name == b"P0"),
        "an unbalanced '(' hid every later resource name: {names:?}"
    );
}

/// `d0` and `d1` are the only content operators carrying a digit, and every Type 3
/// glyph procedure must open with one (ISO 32000-1 §9.6.5). `lopdf`'s `operator`
/// parser matches `[A-Za-z*'"]+`, so it cannot represent them: `d1` tokenises as
/// operator `d`, and the digit is left over as the **first operand of the next
/// operation**.
///
/// A walk reading `operands.first()` therefore saw `1`, not `/Fglyph`, in
/// `20 0 0 0 20 20 d1 /Fglyph Do`. The `Do` was invisible, `/Fglyph` was judged
/// dead and pruned, and the glyph procedure reached the output still naming it —
/// over-deletion plus a dangling name, the exact failure the strict decode was
/// added to prevent, reached through a stream that decodes *successfully*.
/// Resolving the last name-valued operand instead is immune to the shift.
#[tokio::test]
async fn resolves_a_name_shifted_by_the_unparsable_type3_metrics_operator()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "d1-shift.pdf",
        &pdf_with_a_form_invoked_straight_after_a_type3_metrics_operator()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "200")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&bytes, b"D1GLYPHMARK")?,
        "the form a Type 3 glyph invokes right after `d1` was pruned"
    );
    let document = Document::load_mem(&bytes)?;
    assert!(
        declared_resource_names(&document)
            .get(b"XObject".as_slice())
            .is_some_and(|declared| declared.contains(b"Fglyph".as_slice())),
        "/XObject /Fglyph is no longer declared, so the surviving glyph procedure dangles"
    );
    assert_no_dangling_resource_names(&bytes)?;
    Ok(())
}

/// A Type 3 glyph procedure whose `Do` immediately follows its `d1` metrics
/// operator, so the operand `lopdf` leaves in front of the name is what a
/// first-operand read would return.
fn pdf_with_a_form_invoked_straight_after_a_type3_metrics_operator()
-> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        },
        b"0 0 1 rg 0 0 20 20 re f % D1GLYPHMARK".to_vec(),
    ));
    let glyph_id = document.add_object(Stream::new(
        dictionary! {},
        b"20 0 0 0 20 20 d1 /Fglyph Do".to_vec(),
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
        b"BT /T3 20 Tf 20 20 Td (S) Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "T3" => font_id },
            "XObject" => dictionary! { "Fglyph" => form_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// Preserving a scope is not enough when its category sub-dictionary is a shared
/// object: the *other* holder must not filter it either.
///
/// Two pages hold the same `/Pattern` object. One page's content cannot be
/// parsed, so nothing it names can be proven dead; the other's parses and names
/// no pattern at all. Skipping only the unreadable page's own scope still leaves
/// the readable page filtering the very object both point at — which deletes the
/// entry the unreadable page still names, producing exactly the corruption the
/// fail-safe exists to prevent, by a route that looks safe. Protection has to
/// travel with the category object rather than with the holder.
#[tokio::test]
async fn preserves_a_category_dictionary_a_readable_scope_shares_with_an_unreadable_one()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "shared-category.pdf",
        &pdf_with_a_shared_pattern_dictionary_and_one_unreadable_page()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "300")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        document_contains(&bytes, b"SHAREDCATPAT")?,
        "the readable page filtered the /Pattern object it shares with a page whose \
         content could not be parsed"
    );
    let document = Document::load_mem(&bytes)?;
    assert!(
        declared_resource_names(&document)
            .get(b"Pattern".as_slice())
            .is_some_and(|declared| declared.contains(b"Pkeep".as_slice())),
        "/Pattern /Pkeep is no longer declared, so the unreadable page's `scn` dangles"
    );
    assert_no_dangling_resource_names(&bytes)?;
    Ok(())
}

/// Two pages whose inline `/Resources` both point at one shared `/Pattern`
/// dictionary object. Page 1's content is comment-truncated and paints `/Pkeep`;
/// page 2's parses cleanly and paints no pattern. Everything is inside the crop
/// rectangle, so `PDFium` regenerates neither page and page 1 reaches the output
/// still naming `/Pkeep`.
fn pdf_with_a_shared_pattern_dictionary_and_one_unreadable_page()
-> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let tile_id = document.add_object(tiling_pattern(font_id, b"SHAREDCATPAT"));
    // The one object both pages' `/Pattern` entries resolve to.
    let shared_category_id = document.add_object(dictionary! { "Pkeep" => tile_id });
    let mut pages = Vec::new();
    for content in [
        b"% c\n\nq /Pattern cs /Pkeep scn 10 10 50 50 re f Q\n\
           BT /F1 10 Tf 10 120 Td (P1TEXT) Tj ET"
            .to_vec(),
        b"BT /F1 10 Tf 10 30 Td (P2TEXT) Tj ET".to_vec(),
    ] {
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        pages.push(Object::Reference(document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => root_pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font_id },
                "Pattern" => Object::Reference(shared_category_id),
            },
            "Contents" => content_id,
        })));
    }
    document.objects.insert(
        root_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => pages,
            "Count" => 2,
        }),
    );
    let catalog_id =
        document.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

/// A page whose single content stream opens with a comment and a blank line, so
/// `lopdf`'s lenient decoder returns zero operators for it.
fn pdf_with_truncating_comment() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    pdf_with_comment_truncated_content(&[b"% marker\n\nq 1 0 0 1 20 20 cm /Obj5 Do Q\n\
       BT /F12 14 Tf 20 120 Td (VISIBLETEXT) Tj ET"])
}

/// A page whose `/Contents` is an array whose first member is nothing but a
/// comment. `Document::get_page_content` appends a newline after each member, so
/// the blank line that ends the parse exists only in the joined bytes — this is
/// the shape the real Canon file has.
fn pdf_with_comment_only_first_content_stream() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    pdf_with_comment_truncated_content(&[
        b"% CANON_PFINF_TYPE0_TEXTON\n",
        b"q 1 0 0 1 20 20 cm /Obj5 Do Q\n\
          BT /F12 14 Tf 20 120 Td (VISIBLETEXT) Tj ET",
    ])
}

fn pdf_with_comment_truncated_content(
    streams: &[&[u8]],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let form_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
        },
        b"0 0 1 rg 0 0 60 30 re f".to_vec(),
    ));
    let contents = streams
        .iter()
        .map(|content| {
            Object::Reference(document.add_object(Stream::new(dictionary! {}, content.to_vec())))
        })
        .collect::<Vec<_>>();
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F12" => font_id },
            "XObject" => dictionary! { "Obj5" => form_id },
        },
        "Contents" => contents,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// Scaling guard for the resource walk.
///
/// Carrying scopes by value made this quadratic — on the reference host 500
/// inheriting forms cost 0.66 s and 4000 cost 15.9 s (24x for 8x the work), versus
/// 0.35 s and 1.65 s once scopes were carried by identity.
///
/// The assertion is on the **ratio**, not on absolute wall clock, so it does not
/// depend on how fast the host is: the quadratic version grew 24x for 8x the work,
/// while the current one grows roughly 5x-6x. An absolute budget cannot do this
/// job — the bound this replaces was 60 s at 4000 forms, which the 15.9 s
/// regression it is named for would have passed.
///
/// The threshold is 2.5x the work ratio. A 2x threshold sat too close to the
/// measured ratio on heavier fixtures — a meaningful share of the wall clock is
/// `PDFium`'s own per-form cost, not the walk — while 3x gave a 24x limit that the
/// regression it names measured at 24.06x, i.e. a coin flip. At 2.5x the limit is
/// 20x against a 24x regression: a real 20% margin in both directions.
///
/// This is a heuristic, and the `const size_of::<ResourcesId>()` assertion in
/// `pdf_crop.rs` is the deterministic guard. This test exists to catch a
/// regression that keeps the type small but reintroduces the copying elsewhere.
#[tokio::test]
async fn resource_walk_stays_linear_in_the_number_of_inheriting_forms()
-> Result<(), Box<dyn std::error::Error>> {
    let coordinates = [("x", "0"), ("y", "0"), ("width", "200"), ("height", "300")];
    let mut timings = Vec::new();
    for forms in [500_u32, 4_000] {
        let source = pdf_with_inheriting_forms(forms as usize)?;
        let started = std::time::Instant::now();
        let response = post_crop("scaling.pdf", &source, &coordinates).await?;
        let elapsed = started.elapsed();
        if response.status() == StatusCode::NOT_IMPLEMENTED {
            return Ok(());
        }
        require_status(response, StatusCode::OK).await?;
        timings.push((forms, elapsed));
    }
    let (small_forms, small) = timings[0];
    let (large_forms, large) = timings[1];
    // Guard against a baseline so small that timer noise dominates the ratio.
    if small < std::time::Duration::from_millis(20) {
        return Ok(());
    }
    let growth = large.as_secs_f64() / small.as_secs_f64();
    let work = f64::from(large_forms) / f64::from(small_forms);
    assert!(
        growth < work * 2.5,
        "{small_forms} forms took {small:?} and {large_forms} took {large:?} — {growth:.1}x the \
         time for {work:.0}x the work; the walk is no longer linear"
    );
    Ok(())
}

/// Pins the invariant that lets the resource walk ignore annotation appearance
/// streams: the rebuild does not carry annotations over, so an `/AP` stream is
/// never part of the surviving content and cannot be left naming a pruned
/// resource. If annotation preservation is ever added, this test fails and the
/// walk must start following `/AP` — see the note in `pdf_crop.rs`.
#[tokio::test]
async fn rebuilt_pages_carry_no_annotations() -> Result<(), Box<dyn std::error::Error>> {
    let source = pdf_with_annotation_appearance()?;
    for extra in [
        Vec::new(),
        vec![("removeDataOutsideCrop", "false")],
        vec![("autoCrop", "true")],
    ] {
        let mut fields = if extra.iter().any(|(name, _)| *name == "autoCrop") {
            Vec::new()
        } else {
            vec![("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")]
        };
        fields.extend(extra.iter().copied());
        let response = post_crop("annots.pdf", &source, &fields).await?;
        if response.status() == StatusCode::NOT_IMPLEMENTED {
            continue;
        }
        let response = require_status(response, StatusCode::OK).await?;
        let document = Document::load_mem(&to_bytes(response.into_body(), usize::MAX).await?)?;
        for page_id in document.get_pages().into_values() {
            assert!(
                document.get_dictionary(page_id)?.get(b"Annots").is_err(),
                "a rebuilt page carries /Annots; the resource walk must now follow \
                 annotation appearance streams"
            );
        }
    }
    Ok(())
}

/// A page with an annotation whose appearance stream paints with a pattern of its
/// own, plus ordinary in-crop content.
fn pdf_with_annotation_appearance() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"ANNOTPAT"));
    let appearance_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
            "Resources" => dictionary! {
                "Pattern" => dictionary! { "P0" => pattern_id },
            },
        },
        b"q /Pattern cs /P0 scn 0 0 60 30 re f Q".to_vec(),
    ));
    let annotation_id = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Square",
        "Rect" => vec![20.into(), 20.into(), 80.into(), 50.into()],
        "F" => 4,
        "AP" => dictionary! { "N" => appearance_id },
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 20 60 Td (KEEPME) Tj ET\n0 0 1 rg 10 10 40 20 re f".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => content_id,
        "Annots" => vec![Object::Reference(annotation_id)],
    });
    finish_single_page(document, root_pages_id, page_id)
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

/// A Form `XObject` declared in *another form's own* `/Resources` that nothing
/// ever invokes.
///
/// `PDFium` never rewrites a form's own resource dictionary, so such a declaration
/// survives its rebuild. An earlier revision therefore *traversed* it to avoid
/// leaving a dangling name — which kept the dead stream, and everything it named,
/// in the output: an out-of-crop secret stayed extractable through it, and
/// cross-declaring forms rescanned under many scopes turned a 532 KB upload into
/// 68 s of work.
///
/// The declaration is now pruned instead. That is both directions at once: the
/// dead stream and its pattern leave the file, and nothing is left naming a
/// resource that no dictionary declares. It is also what makes the walk's rule —
/// traverse every content stream that will still be in the output file — true by
/// construction rather than by enumerating paths.
#[tokio::test]
async fn prunes_form_xobjects_declared_but_never_invoked() -> Result<(), Box<dyn std::error::Error>>
{
    let response = post_crop(
        "declared-xobject.pdf",
        &pdf_with_form_declaring_an_uninvoked_form()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&bytes, b"FORMRESINNERPAT")?,
        "the never-invoked form kept its pattern alive in the output"
    );
    assert!(
        !document_contains(&bytes, b"DECLOUTOFCROP")?,
        "the out-of-crop text survived"
    );
    assert!(
        document_contains(&bytes, b"KEEPME")?,
        "the in-crop text was removed"
    );
    assert_no_dangling_resource_names(&bytes)?;
    Ok(())
}

/// An in-crop form whose own `/Resources` declares a second form that nothing
/// invokes, and whose content paints the page's pattern.
fn pdf_with_form_declaring_an_uninvoked_form() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"FORMRESINNERPAT"));
    let uninvoked_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
        },
        b"q /Pattern cs /Pinner scn 0 0 60 30 re f Q".to_vec(),
    ));
    let alive_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 60.into(), 30.into()],
            // Declared but never invoked by this form's content.
            "Resources" => dictionary! {
                "XObject" => dictionary! { "Fdead" => uninvoked_id },
            },
        },
        b"0 0 1 rg 0 0 10 10 re f".to_vec(),
    ));
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 10 30 Td (KEEPME) Tj ET\n\
          q 1 0 0 1 5 5 cm /Alive Do Q\n\
          BT /F1 8 Tf 20 250 Td (DECLOUTOFCROP) Tj ET"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "Pattern" => dictionary! { "Pinner" => pattern_id },
            "XObject" => dictionary! { "Alive" => alive_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
}

/// `/ExtGState` can select a font directly (ISO 32000-1 Table 58), so a Type 3
/// font chosen that way never trips the `Tf` arm — and `PDFium` regenerates the
/// page's operators, dropping the `gs` that invoked the state while keeping the
/// state *declared* with its glyph procedures intact.
///
/// Both the graphics state and the font are pruned, because after regeneration no
/// executed path names either. The glyph procedure and the pattern it painted
/// leave the file with them, so there is nothing left to dangle and nothing left
/// to extract.
#[tokio::test]
async fn prunes_graphics_states_and_fonts_no_executed_path_names()
-> Result<(), Box<dyn std::error::Error>> {
    let response = post_crop(
        "extgstate-font.pdf",
        &pdf_with_type3_font_selected_by_graphics_state()?,
        &[("x", "0"), ("y", "0"), ("width", "200"), ("height", "100")],
    )
    .await?;
    if response.status() == StatusCode::NOT_IMPLEMENTED {
        return Ok(());
    }
    let response = require_status(response, StatusCode::OK).await?;
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(
        !document_contains(&bytes, b"EGFONTPAT")?,
        "the unreferenced graphics state kept its glyph's pattern alive"
    );
    assert!(
        !document_contains(&bytes, b"EGOUTOFCROP")?,
        "the out-of-crop text survived"
    );
    assert!(
        document_contains(&bytes, b"KEEPME")?,
        "the in-crop text was removed"
    );
    assert_no_dangling_resource_names(&bytes)?;
    Ok(())
}

/// A fixture PDF and the label a failure names it by.
type Fixture = (&'static str, Vec<u8>);

/// Every PDF this file builds, so the dangling-name invariant can be asserted
/// across all of them from one place rather than per test.
///
/// Every `pdf_with_*` builder in this file appears here. A new fixture that does
/// not is a fixture the invariant is not checked against.
fn every_fixture() -> Result<Vec<Fixture>, Box<dyn std::error::Error>> {
    Ok(vec![
        ("content", pdf_with_content(&["BASE0", "BASE1"])?),
        (
            "colliding_pattern_names",
            pdf_with_colliding_pattern_names()?,
        ),
        (
            "inherited_form_resources",
            pdf_with_inherited_form_resources()?,
        ),
        ("truncating_comment", pdf_with_truncating_comment()?),
        (
            "comment_only_first_content_stream",
            pdf_with_comment_only_first_content_stream()?,
        ),
        (
            "extgstate_font_indirect_array",
            pdf_with_extgstate_font_indirect_array(false)?,
        ),
        (
            "extgstate_font_indirect_array_with_decoy",
            pdf_with_extgstate_font_indirect_array(true)?,
        ),
        (
            "form_with_indirect_subtype",
            pdf_with_form_with_indirect_subtype()?,
        ),
        (
            "alias_chain_to_shared_category",
            pdf_with_alias_chain_to_shared_category()?,
        ),
        (
            "form_invoked_straight_after_a_type3_metrics_operator",
            pdf_with_a_form_invoked_straight_after_a_type3_metrics_operator()?,
        ),
        (
            "shared_pattern_dictionary_and_one_unreadable_page",
            pdf_with_a_shared_pattern_dictionary_and_one_unreadable_page()?,
        ),
        ("annotation_appearance", pdf_with_annotation_appearance()?),
        (
            "form_declaring_an_uninvoked_form",
            pdf_with_form_declaring_an_uninvoked_form()?,
        ),
        (
            "type3_font_selected_by_graphics_state",
            pdf_with_type3_font_selected_by_graphics_state()?,
        ),
        (
            "type3_glyph_painting_a_pattern",
            pdf_with_type3_glyph_painting_a_pattern()?,
        ),
        (
            "soft_mask_group_painting_a_pattern",
            pdf_with_soft_mask_group_painting_a_pattern()?,
        ),
        ("inheriting_form_chain", pdf_with_inheriting_form_chain(40)?),
        (
            "shared_indirect_resources",
            pdf_with_shared_indirect_resources()?,
        ),
        ("nested_form_scopes", pdf_with_nested_form_scopes(40)?),
        (
            "undecodable_form_content",
            pdf_with_undecodable_form_content()?,
        ),
        ("inheriting_forms", pdf_with_inheriting_forms(50)?),
        ("in_crop_pattern", pdf_with_in_crop_pattern()?),
        (
            "out_of_crop_pattern_text",
            pdf_with_out_of_crop_pattern(PatternPayload::Text)?,
        ),
        (
            "out_of_crop_pattern_image",
            pdf_with_out_of_crop_pattern(PatternPayload::Image)?,
        ),
        ("out_of_crop_shading", pdf_with_out_of_crop_shading()?),
        (
            "type3_text_outside_the_crop",
            pdf_with_type3_text_outside_the_crop()?,
        ),
        (
            "text_inside_and_outside",
            pdf_with_text_inside_and_outside()?,
        ),
    ])
}

/// The invariant that separates this revision from the six before it: whatever
/// the pruning decides, the file it hands back must not contain a content stream
/// naming a resource nothing declares.
///
/// Asserted over every fixture and both modes, because the defects this pruning
/// has shipped were each found by one test that happened to look — six times the
/// reasoning was sound and the code was not. A property checked everywhere does
/// not depend on someone thinking to check it here.
///
/// Several rectangles, not one, and the full-page rectangle is load-bearing. What
/// the crop removes decides whether `PDFium` regenerates a page, and regeneration
/// *hides* pruning defects: it rewrites the page's operators, so a name the
/// original stream used may simply be absent from the output that gets checked. A
/// rectangle that removes nothing regenerates nothing, and the original streams
/// reach the output verbatim, still naming everything they named. A single
/// removing rectangle misses that entire class — verified, not assumed: the shared
/// category-dictionary defect this file also covers directly is invisible at
/// `200x100` and caught at `200x300`.
#[tokio::test]
async fn every_fixture_comes_back_without_a_dangling_resource_name()
-> Result<(), Box<dyn std::error::Error>> {
    for (label, source) in every_fixture()? {
        for rectangle in [
            // Contains every fixture page whole: nothing removed, nothing
            // regenerated, original content streams preserved byte for byte.
            ("0", "0", "200", "300"),
            // Bottom strip, middle window, and a tiny corner: each removes a
            // different set of marks, so a different set of pages is regenerated.
            ("0", "0", "200", "100"),
            ("50", "150", "100", "100"),
            ("0", "0", "50", "50"),
        ] {
            for remove in ["true", "false"] {
                let (x, y, width, height) = rectangle;
                let response = post_crop(
                    "fixture.pdf",
                    &source,
                    &[
                        ("x", x),
                        ("y", y),
                        ("width", width),
                        ("height", height),
                        ("removeDataOutsideCrop", remove),
                    ],
                )
                .await?;
                if response.status() == StatusCode::NOT_IMPLEMENTED {
                    continue;
                }
                let response = require_status(response, StatusCode::OK).await?;
                let bytes = to_bytes(response.into_body(), usize::MAX).await?;
                let report = audit_resource_names(&bytes)?;
                assert!(
                    report.is_clean(),
                    "{label} cropped to {rectangle:?} with removeDataOutsideCrop={remove} \
                     does not resolve against its own scope: {report:?}"
                );
            }
        }
    }
    Ok(())
}

/// Pins the invariant checker itself.
///
/// Every assertion above is worth exactly what this test is worth: a
/// [`audit_resource_names`] that always returned nothing would make all of
/// them pass vacuously, which is the failure mode of a checker nobody checks. The
/// second case is the one that matters most — a document whose content parses
/// only up to a `%` comment. That is precisely where `Content::decode` reports
/// success with an empty operator list, so an oracle built on it would see no
/// names at all and agree that nothing dangles. This one must still find `/Late`.
#[test]
fn the_dangling_name_checker_detects_a_name_no_dictionary_declares()
-> Result<(), Box<dyn std::error::Error>> {
    for (label, content, expected) in [
        (
            "undeclared name in plainly parsable content",
            b"q /Pattern cs /P0 scn 0 0 10 10 re f Q".to_vec(),
            ("Pattern", "P0"),
        ),
        (
            "undeclared name after a comment that ends the lenient parse",
            b"% marker\n\nq 1 0 0 1 5 5 cm /Late Do Q".to_vec(),
            ("XObject", "Late"),
        ),
    ] {
        let mut document = Document::with_version("1.7");
        let root_pages_id = document.new_object_id();
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => root_pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "Resources" => dictionary! {},
            "Contents" => content_id,
        });
        let corrupt = finish_single_page(document, root_pages_id, page_id)?;
        let report = audit_resource_names(&corrupt)?;
        let expected = (expected.0.to_owned(), expected.1.to_owned());
        assert!(
            report.dangling.contains(&expected),
            "{label}: the checker reported {report:?}, missing {expected:?}"
        );
    }
    Ok(())
}

/// Sweeps a directory of real PDFs for the same invariant.
///
/// Skipped unless `RUSTLING_CROP_CORPUS_DIR` names a directory, because the
/// corpus is not something this repository can ship. Synthetic fixtures encode
/// what their author already thought of; the defect this revision fixes was found
/// in a file nobody wrote for the purpose.
///
/// The control is the same document cropped with `removeDataOutsideCrop=false`,
/// which prunes nothing. Only names that dangle in the removal output *and not*
/// in the control are attributable to the pruning; anything dangling in both is
/// pre-existing in the source or an artefact of this tokeniser.
#[tokio::test]
async fn sweeps_a_pdf_corpus_for_dangling_resource_names() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(directory) = rustling_processing::env_compat::var_os("RUSTLING_CROP_CORPUS_DIR")
    else {
        return Ok(());
    };
    let mut swept = 0_usize;
    let mut cropped = 0_usize;
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "pdf") {
            continue;
        }
        swept += 1;
        let Ok(source) = std::fs::read(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut sets = Vec::new();
        for remove in ["true", "false"] {
            let response = post_crop(
                &name,
                &source,
                &[
                    ("x", "0"),
                    ("y", "0"),
                    ("width", "400"),
                    ("height", "400"),
                    ("removeDataOutsideCrop", remove),
                ],
            )
            .await?;
            if response.status() != StatusCode::OK {
                break;
            }
            let bytes = to_bytes(response.into_body(), usize::MAX).await?;
            let Ok(report) = audit_resource_names(&bytes) else {
                break;
            };
            sets.push(report.dangling);
        }
        let [removed, control] = sets.as_slice() else {
            continue;
        };
        cropped += 1;
        let introduced = removed.difference(control).cloned().collect::<Vec<_>>();
        if !introduced.is_empty() {
            failures.push(format!("{name}: {introduced:?}"));
        }
    }
    eprintln!("crop corpus sweep: {swept} files seen, {cropped} cropped in both modes");
    assert!(
        failures.is_empty(),
        "pruning introduced dangling resource names in {} of {cropped} documents:\n{}",
        failures.len(),
        failures.join("\n")
    );
    Ok(())
}

/// The five resource categories the crop rebuild prunes.
const RESOURCE_CATEGORIES: [&[u8]; 5] = [b"XObject", b"Font", b"ExtGState", b"Pattern", b"Shading"];

/// Fails if any surviving content stream names a resource that is not declared
/// **in that stream's own resource scope** — the corruption direction of the
/// pruning.
///
/// Two properties make this worth more than the version it replaces.
///
/// It is **scope-keyed**. The earlier checker unioned every declared name in the
/// document into one set per category and accepted a used name if *any*
/// dictionary anywhere declared it. That accepts exactly the bug it exists to
/// catch: prune `/P0` from the page `/Resources`, leave an unexecuted form that
/// declares its own unrelated `/P0`, and a surviving glyph procedure resolving
/// through the page scope finds nothing while the checker reports clean.
/// Resolution here walks the scope chain a viewer would walk (ISO 32000-1
/// §8.10.1): the stream's own `/Resources` first, then the scopes it inherits.
///
/// It is **deref-aware**. Every value a PDF can write indirectly is read through
/// `Document::dereference`, because the walk under test had four separate places
/// where a raw `Dictionary::get` met a value written as `12 0 R` — and a checker
/// that reads the same way is blind in the same places.
///
/// It also does not go through `Content::decode`, which returns whatever prefix
/// it parsed and discards the rest; [`resource_names_used`] tokenises raw bytes.
fn assert_no_dangling_resource_names(pdf: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let report = audit_resource_names(pdf)?;
    assert!(
        report.is_clean(),
        "surviving content does not resolve against its own scope: {report:?}"
    );
    Ok(())
}

/// What the oracle found, including the ways it could fail to look.
///
/// `unreadable_streams` and `truncated` are reported rather than swallowed: a
/// checker that silently skips what it cannot read reports "clean" for a document
/// it never examined, which is indistinguishable from a real pass.
#[derive(Debug, Default, PartialEq, Eq)]
struct ResourceAudit {
    dangling: BTreeSet<(String, String)>,
    unreadable_streams: usize,
    truncated: bool,
}

impl ResourceAudit {
    fn is_clean(&self) -> bool {
        self.dangling.is_empty() && self.unreadable_streams == 0 && !self.truncated
    }
}

/// One resource scope, identified the way a viewer would have to identify it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Scope {
    /// `/Resources` is an indirect object; the id is the end of the chain.
    Shared(lopdf::ObjectId),
    /// `/Resources` is written inline inside this object.
    InlineIn(lopdf::ObjectId),
}

/// Where a queued content stream comes from.
#[derive(Clone, Copy, Debug)]
enum StreamSource {
    Page(lopdf::ObjectId),
    Object(lopdf::ObjectId),
}

/// Keeps a pathological document from turning the oracle into the thing it is
/// checking. Reaching either bound sets `truncated`, which fails the assertion
/// rather than passing quietly.
const AUDIT_MAX_PAIRS: usize = 200_000;
const AUDIT_MAX_DEPTH: usize = 64;

/// Resolves every resource name every reachable content stream uses, against the
/// scope chain that stream actually executes under.
fn audit_resource_names(pdf: &[u8]) -> Result<ResourceAudit, Box<dyn std::error::Error>> {
    let document = Document::load_mem(pdf)?;
    let mut audit = ResourceAudit::default();
    let mut visited: HashSet<(lopdf::ObjectId, Vec<Scope>)> = HashSet::new();
    let mut queue: VecDeque<(StreamSource, Vec<Scope>)> = VecDeque::new();
    for page_id in document.get_pages().into_values() {
        let Some(scope) = page_scope(&document, page_id) else {
            continue;
        };
        queue.push_back((StreamSource::Page(page_id), vec![scope]));
    }
    while let Some((source, chain)) = queue.pop_front() {
        if visited.len() >= AUDIT_MAX_PAIRS {
            audit.truncated = true;
            break;
        }
        let content = match source {
            StreamSource::Page(page_id) => document.get_page_content(page_id),
            StreamSource::Object(object_id) => {
                let Ok(content) = document
                    .get_object(object_id)
                    .and_then(Object::as_stream)
                    .and_then(Stream::decompressed_content)
                else {
                    audit.unreadable_streams += 1;
                    continue;
                };
                content
            }
        };
        for (category, name) in resource_names_used(&content) {
            let Some(value) = resolve_in_scope(&document, &chain, category, &name) else {
                audit.dangling.insert((
                    String::from_utf8_lossy(category).into_owned(),
                    String::from_utf8_lossy(&name).into_owned(),
                ));
                continue;
            };
            follow_resource(
                &document,
                category,
                &value,
                &chain,
                &mut visited,
                &mut queue,
                &mut audit,
            );
        }
    }
    Ok(audit)
}

/// The scope a page executes in, following `/Parent` to whichever node carries
/// `/Resources`.
fn page_scope(document: &Document, page_id: lopdf::ObjectId) -> Option<Scope> {
    let mut object_id = page_id;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(object_id) {
            return None;
        }
        let (owner, object) = document
            .dereference(document.objects.get(&object_id)?)
            .ok()?;
        let dictionary = object.as_dict().ok()?;
        let owner = owner.unwrap_or(object_id);
        if let Ok(resources) = dictionary.get(b"Resources") {
            return scope_of(document, owner, resources);
        }
        object_id = dictionary.get(b"Parent").ok()?.as_reference().ok()?;
    }
}

fn scope_of(document: &Document, owner: lopdf::ObjectId, resources: &Object) -> Option<Scope> {
    match document.dereference(resources).ok()? {
        (Some(object_id), _) => Some(Scope::Shared(object_id)),
        (None, _) => Some(Scope::InlineIn(owner)),
    }
}

fn scope_dictionary(document: &Document, scope: Scope) -> Option<&lopdf::Dictionary> {
    match scope {
        Scope::Shared(object_id) => document.get_dictionary(object_id).ok(),
        Scope::InlineIn(owner) => {
            let resources = match document.get_object(owner).ok()? {
                Object::Dictionary(dictionary) => dictionary.get(b"Resources").ok()?,
                Object::Stream(stream) => stream.dict.get(b"Resources").ok()?,
                _ => return None,
            };
            dereference_dictionary(document, Some(resources))
        }
    }
}

/// Looks `name` up the scope chain, innermost first, exactly as a viewer would.
fn resolve_in_scope(
    document: &Document,
    chain: &[Scope],
    category: &[u8],
    name: &[u8],
) -> Option<Object> {
    for scope in chain {
        let Some(resources) = scope_dictionary(document, *scope) else {
            continue;
        };
        let Some(entries) = dereference_dictionary(document, resources.get(category).ok()) else {
            continue;
        };
        if let Ok(value) = entries.get(name) {
            return Some(value.clone());
        }
    }
    None
}

/// The scope chain a nested stream executes under: its own `/Resources` first,
/// otherwise the enclosing chain unchanged.
fn child_scope(
    document: &Document,
    chain: &[Scope],
    owner: lopdf::ObjectId,
    resources: Option<&Object>,
) -> Option<Vec<Scope>> {
    let Some(resources) = resources else {
        return Some(chain.to_vec());
    };
    if chain.len() >= AUDIT_MAX_DEPTH {
        return None;
    }
    let scope = scope_of(document, owner, resources)?;
    let mut child = Vec::with_capacity(chain.len() + 1);
    child.push(scope);
    child.extend_from_slice(chain);
    Some(child)
}

/// Queues whatever content a resolved resource reaches, mirroring the five
/// operators a viewer executes.
fn follow_resource(
    document: &Document,
    category: &[u8],
    value: &Object,
    chain: &[Scope],
    visited: &mut HashSet<(lopdf::ObjectId, Vec<Scope>)>,
    queue: &mut VecDeque<(StreamSource, Vec<Scope>)>,
    audit: &mut ResourceAudit,
) {
    let mut enqueue_stream = |object_id: lopdf::ObjectId, chain: Vec<Scope>| {
        if visited.insert((object_id, chain.clone())) {
            queue.push_back((StreamSource::Object(object_id), chain));
        }
    };
    match category {
        b"XObject" | b"Pattern" => {
            let Ok((Some(object_id), object)) = document.dereference(value) else {
                return;
            };
            let Ok(stream) = object.as_stream() else {
                return;
            };
            // An image XObject has no operators; a shading pattern is a
            // dictionary, not a stream, and reaches no content stream either.
            if category == b"XObject"
                && audit_name(document, stream.dict.get(b"Subtype").ok()) != Some(b"Form".to_vec())
            {
                return;
            }
            let Some(child) = child_scope(
                document,
                chain,
                object_id,
                stream.dict.get(b"Resources").ok(),
            ) else {
                audit.truncated = true;
                return;
            };
            enqueue_stream(object_id, child);
        }
        b"Font" => follow_type3_font(document, value, chain, &mut enqueue_stream, audit),
        b"ExtGState" => {
            let Some(state) = dereference_dictionary(document, Some(value)) else {
                return;
            };
            if let Some(mask) = dereference_dictionary(document, state.get(b"SMask").ok())
                && let Ok(group) = mask.get(b"G")
                && let Ok((Some(object_id), object)) = document.dereference(group)
                && let Ok(stream) = object.as_stream()
            {
                if let Some(child) = child_scope(
                    document,
                    chain,
                    object_id,
                    stream.dict.get(b"Resources").ok(),
                ) {
                    enqueue_stream(object_id, child);
                } else {
                    audit.truncated = true;
                }
            }
            if let Ok((_, Object::Array(font))) =
                document.dereference(state.get(b"Font").unwrap_or(&Object::Null))
                && let Some(font) = font.first().cloned()
            {
                follow_type3_font(document, &font, chain, &mut enqueue_stream, audit);
            }
        }
        _ => {}
    }
}

fn follow_type3_font(
    document: &Document,
    value: &Object,
    chain: &[Scope],
    enqueue_stream: &mut impl FnMut(lopdf::ObjectId, Vec<Scope>),
    audit: &mut ResourceAudit,
) {
    let Ok((font_id, object)) = document.dereference(value) else {
        return;
    };
    let Ok(font) = object.as_dict() else {
        return;
    };
    if audit_name(document, font.get(b"Subtype").ok()) != Some(b"Type3".to_vec()) {
        return;
    }
    let child = match (font_id, font.get(b"Resources").ok()) {
        (_, None) => chain.to_vec(),
        (Some(font_id), resources) => {
            let Some(child) = child_scope(document, chain, font_id, resources) else {
                audit.truncated = true;
                return;
            };
            child
        }
        // A directly embedded font declaring its own resources has no id to key a
        // scope on, so the oracle cannot resolve its glyphs and says so.
        (None, Some(_)) => {
            audit.truncated = true;
            return;
        }
    };
    let Some(procedures) = dereference_dictionary(document, font.get(b"CharProcs").ok()) else {
        return;
    };
    for (_, procedure) in procedures {
        if let Ok((Some(object_id), object)) = document.dereference(procedure)
            && object.as_stream().is_ok()
        {
            enqueue_stream(object_id, child.clone());
        }
    }
}

fn audit_name(document: &Document, object: Option<&Object>) -> Option<Vec<u8>> {
    Some(
        document
            .dereference(object?)
            .ok()?
            .1
            .as_name()
            .ok()?
            .to_vec(),
    )
}

fn dereference_dictionary<'a>(
    document: &'a Document,
    object: Option<&'a Object>,
) -> Option<&'a lopdf::Dictionary> {
    document.dereference(object?).ok()?.1.as_dict().ok()
}

/// Every resource name the document declares, per category.
///
/// Deliberately document-scoped, and used only where a test wants to assert that
/// a specific declaration still exists somewhere — never as the dangling-name
/// check, which needs the scope-aware [`audit_resource_names`].
fn declared_resource_names(document: &Document) -> HashMap<&'static [u8], HashSet<Vec<u8>>> {
    let mut declared: HashMap<&'static [u8], HashSet<Vec<u8>>> = HashMap::new();
    for object in document.objects.values() {
        let dictionary = match object {
            Object::Dictionary(dictionary) => dictionary,
            Object::Stream(stream) => &stream.dict,
            _ => continue,
        };
        let held = dereference_dictionary(document, dictionary.get(b"Resources").ok());
        for resources in [Some(dictionary), held].into_iter().flatten() {
            for category in RESOURCE_CATEGORIES {
                let Some(entries) = dereference_dictionary(document, resources.get(category).ok())
                else {
                    continue;
                };
                let declared = declared.entry(category).or_default();
                for (name, _) in entries {
                    declared.insert(name.clone());
                }
            }
        }
    }
    declared
}

/// The resource names a content stream's operators consume, tokenised straight
/// from the raw bytes.
///
/// Independent of `lopdf`'s content parser on purpose — see
/// [`assert_no_dangling_resource_names`]. Comments, literal and hex strings,
/// dictionaries, arrays and inline-image payloads are skipped, so their bytes
/// cannot be mistaken for names or operators, and unlike the lenient decoder this
/// keeps going to the end of the stream instead of stopping at the first
/// construct it does not like.
///
/// For every operator here the name is the **last** name-valued operand, which
/// holds for `scn`/`SCN` (where colour components can precede it) and equally for
/// the single-operand forms — and does not depend on this tokeniser recognising
/// every operator that may have preceded it.
fn resource_names_used(content: &[u8]) -> Vec<(&'static [u8], Vec<u8>)> {
    let mut used = Vec::new();
    let mut operands: Vec<Option<Vec<u8>>> = Vec::new();
    let mut index = 0;
    while index < content.len() {
        match content[index] {
            byte if is_content_space(byte) => index += 1,
            b'%' => {
                while index < content.len() && !matches!(content[index], b'\r' | b'\n') {
                    index += 1;
                }
            }
            b'(' => {
                index = skip_literal_string(content, index);
                operands.push(None);
            }
            b'<' | b'[' | b'{' => {
                index = skip_bracketed(content, index);
                operands.push(None);
            }
            // Unbalanced closers: step over them rather than looping forever.
            b')' | b'>' | b']' | b'}' => index += 1,
            b'/' => {
                let mut end = index + 1;
                while end < content.len() && is_regular(content[end]) {
                    end += 1;
                }
                operands.push(Some(content[index + 1..end].to_vec()));
                index = end;
            }
            _ => {
                let start = index;
                while index < content.len() && is_regular(content[index]) {
                    index += 1;
                }
                if index == start {
                    index += 1;
                    continue;
                }
                let token = &content[start..index];
                let Some(category) = operator_category(token) else {
                    if is_operator(token) {
                        // `ID` hands the rest of the line to the image sampler,
                        // whose bytes are not operators.
                        if token == b"ID" {
                            index = skip_inline_image_data(content, index);
                        }
                        operands.clear();
                    } else {
                        operands.push(None);
                    }
                    continue;
                };
                if let Some(name) = operands.iter().rev().flatten().next() {
                    used.push((category, name.clone()));
                }
                operands.clear();
            }
        }
    }
    used
}

/// The resource category an operator resolves its name operand in.
fn operator_category(token: &[u8]) -> Option<&'static [u8]> {
    match token {
        b"Do" => Some(b"XObject"),
        b"Tf" => Some(b"Font"),
        b"gs" => Some(b"ExtGState"),
        b"sh" => Some(b"Shading"),
        b"scn" | b"SCN" => Some(b"Pattern"),
        _ => None,
    }
}

/// Whether a token ends an operand run.
///
/// `d0`/`d1` are named explicitly: they are the only operators carrying a digit,
/// and treating them as operands would leave a Type 3 glyph's metrics in the run.
/// `true`/`false`/`null` look like operators but are operands.
fn is_operator(token: &[u8]) -> bool {
    if matches!(token, b"d0" | b"d1") {
        return true;
    }
    if matches!(token, b"true" | b"false" | b"null") {
        return false;
    }
    token
        .iter()
        .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'*' | b'\'' | b'"'))
}

fn is_content_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\0' | b'\x0c')
}

fn is_regular(byte: u8) -> bool {
    !is_content_space(byte) && !b"()<>[]{}/%".contains(&byte)
}

/// Skips one literal string.
///
/// An **unbalanced** `(` steps over that one byte instead of consuming the rest of
/// the stream. Running to the end would hide every resource name after it, which
/// is a checker that reports clean because it stopped looking — and one stray
/// parenthesis is all it took.
fn skip_literal_string(content: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while index < content.len() {
        match content[index] {
            b'\\' => index += 2,
            b'(' => {
                depth += 1;
                index += 1;
            }
            b')' => {
                index += 1;
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    start + 1
}

/// Skips one bracketed operand — dictionary, hex string, array, or PostScript
/// procedure — and everything nested inside it.
fn skip_bracketed(content: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while index < content.len() {
        match content[index] {
            b'(' => index = skip_literal_string(content, index),
            b'%' => {
                while index < content.len() && !matches!(content[index], b'\r' | b'\n') {
                    index += 1;
                }
            }
            b'<' if content.get(index + 1) == Some(&b'<') => {
                depth += 1;
                index += 2;
            }
            b'>' if content.get(index + 1) == Some(&b'>') => {
                index += 2;
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index;
                }
            }
            b'<' => {
                index += 1;
                while index < content.len() && content[index] != b'>' {
                    index += 1;
                }
                index = content.len().min(index + 1);
                if depth == 0 {
                    return index;
                }
            }
            b'[' | b'{' => {
                depth += 1;
                index += 1;
            }
            b']' | b'}' => {
                index += 1;
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    // Unbalanced: step over the opener rather than swallowing the stream.
    start + 1
}

/// Skips an inline image's samples, which start one whitespace byte after `ID`
/// and end at a whitespace-delimited `EI`.
fn skip_inline_image_data(content: &[u8], start: usize) -> usize {
    let mut index = start.saturating_add(1);
    while index + 1 < content.len() {
        if content[index] == b'E'
            && content[index + 1] == b'I'
            && content
                .get(index.wrapping_sub(1))
                .is_some_and(|byte| is_content_space(*byte))
            && content
                .get(index + 2)
                .is_none_or(|byte| is_content_space(*byte))
        {
            return index + 2;
        }
        index += 1;
    }
    content.len()
}

/// A page whose `/ExtGState` carries `/Font [<Type 3 font> size]`, with the glyph
/// drawn inside the crop rectangle and painting the page's pattern.
fn pdf_with_type3_font_selected_by_graphics_state() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let root_pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let pattern_id = document.add_object(tiling_pattern(font_id, b"EGFONTPAT"));
    let glyph_id = document.add_object(Stream::new(
        dictionary! {},
        b"20 0 0 0 20 20 d1 q /Pattern cs /P0 scn 0 0 20 20 re f Q".to_vec(),
    ));
    let char_procs_id = document.add_object(dictionary! { "ga" => glyph_id });
    let encoding_id = document.add_object(dictionary! {
        "Type" => "Encoding",
        "Differences" => vec![83.into(), Object::Name(b"ga".to_vec())],
    });
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
    let state_id = document.add_object(dictionary! {
        "Type" => "ExtGState",
        "Font" => vec![Object::Reference(type3_id), 12.into()],
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 10 30 Td (KEEPME) Tj ET\n\
          q /EG gs BT 5 5 Td (S) Tj ET Q\n\
          BT /F1 8 Tf 20 250 Td (EGOUTOFCROP) Tj ET"
            .to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "ExtGState" => dictionary! { "EG" => state_id },
            "Pattern" => dictionary! { "P0" => pattern_id },
        },
        "Contents" => content_id,
    });
    finish_single_page(document, root_pages_id, page_id)
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
