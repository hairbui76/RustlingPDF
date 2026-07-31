//! End-to-end coverage of the built-in Office→PDF engine and its bounds.
//!
//! These tests run the real worker binary, so they exercise the same
//! spawn/watchdog path the HTTP endpoint uses. The worker is named explicitly
//! because `cargo test` runs a harness binary that knows nothing about
//! `--office2pdf-worker`.

use std::{ffi::OsStr, fs, io::Write, path::Path, process::Command};

use rustling_processing::office_builtin_engine::{
    BuiltinEngineError, WORKER_ARGUMENT, convert_with_worker,
};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

fn worker() -> &'static OsStr {
    OsStr::new(env!("CARGO_BIN_EXE_rustling-processing"))
}

fn corpus(name: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/office")
        .join(name);
    path.is_file().then_some(path)
}

fn page_count(path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(lopdf::Document::load(path)?.get_pages().len())
}

/// A real DOCX, XLSX, and PPTX must each come out as a PDF with at least one
/// page — a `200`, or an `Ok`, proves nothing about whether the document
/// survived.
#[test]
fn converts_every_built_in_format_into_a_readable_pdf() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    for (name, extension) in [
        ("sample.docx", "docx"),
        ("sample.xlsx", "xlsx"),
        ("sample.pptx", "pptx"),
    ] {
        let Some(input) = corpus(name) else {
            continue;
        };
        let output = workspace.path().join(format!("{extension}.pdf"));
        let conversion = convert_with_worker(&input, extension, &output, Some(worker()))?;
        let bytes = fs::read(&output)?;
        assert!(
            bytes.starts_with(b"%PDF"),
            "{name} did not produce a PDF header"
        );
        assert!(
            page_count(&output)? >= 1,
            "{name} produced a PDF with no pages"
        );
        assert!(
            !conversion.dropped_content,
            "{name} lost content: {:?}",
            conversion.warnings
        );
    }
    Ok(())
}

/// A document the engine cannot parse must come back as a clean error, not as a
/// crash of the caller and not as an empty PDF presented as success.
#[test]
fn a_malformed_package_fails_without_taking_the_caller_down()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let input = workspace.path().join("broken.docx");
    fs::write(&input, b"PK\x03\x04 and then nothing that parses")?;
    let output = workspace.path().join("broken.pdf");

    let result = convert_with_worker(&input, "docx", &output, Some(worker()));
    assert!(
        matches!(
            result,
            Err(BuiltinEngineError::Conversion(_) | BuiltinEngineError::WorkerStopped { .. })
        ),
        "expected a contained failure, got {result:?}"
    );
    assert!(!output.exists(), "a failed conversion must leave no output");
    Ok(())
}

/// The bound that keeps a 100k-row workbook from asking for 7.7 GB. It must
/// trip in the parent, before any worker is spawned.
#[test]
fn a_workbook_past_the_row_bound_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let input = workspace.path().join("huge.xlsx");
    let mut sheet = Vec::from(*b"<worksheet><sheetData>");
    for _ in 0..25_000 {
        sheet.extend_from_slice(b"<row/>");
    }
    sheet.extend_from_slice(b"</sheetData></worksheet>");

    let file = fs::File::create(&input)?;
    let mut archive = ZipWriter::new(file);
    archive.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default())?;
    archive.write_all(&sheet)?;
    archive.finish()?;

    let result = convert_with_worker(
        &input,
        "xlsx",
        &workspace.path().join("huge.pdf"),
        Some(worker()),
    );
    assert!(
        matches!(result, Err(BuiltinEngineError::TooManyRows { .. })),
        "expected a row-bound rejection, got {result:?}"
    );
    Ok(())
}

/// A document that names its embedded font `/tmp/...` or `../../...` must not
/// be able to place a file there. The published `office2pdf` 0.6.5 wrote the
/// font wherever the name pointed; this is the regression test for the pinned
/// fork that sanitises it.
#[test]
fn an_embedded_font_name_cannot_escape_the_engine_temp_dir()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let escape_target = workspace.path().join("PWNED");
    let input = workspace.path().join("hostile.docx");
    build_font_traversal_docx(&input, &escape_target.to_string_lossy())?;

    // The conversion itself may succeed or fail; only the side effect matters.
    let _ = convert_with_worker(
        &input,
        "docx",
        &workspace.path().join("hostile.pdf"),
        Some(worker()),
    );

    for style in ["regular", "bold", "italic", "boldItalic"] {
        let written = workspace.path().join(format!("PWNED-{style}.ttf"));
        assert!(
            !written.exists(),
            "the document placed a file at {}",
            written.display()
        );
    }
    Ok(())
}

/// The evasion a filename-convention scan allowed: OOXML part paths come from
/// relationship targets, so a standards-legal workbook can put its only
/// worksheet outside `xl/worksheets/`. Matching on the conventional prefix made
/// the row bound decorative — the counter saw zero rows and the engine read
/// them all.
#[test]
fn a_relocated_worksheet_part_cannot_evade_the_row_bound() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace = TempDir::new()?;
    let input = workspace.path().join("relocated.xlsx");

    let mut sheet = Vec::from(*b"<worksheet><sheetData>");
    for _ in 0..25_000 {
        sheet.extend_from_slice(b"<row/>");
    }
    sheet.extend_from_slice(b"</sheetData></worksheet>");

    let file = fs::File::create(&input)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive.start_file("xl/workbook.xml", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    )?;
    archive.start_file("xl/_rels/workbook.xml.rels", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="data/s1.xml"/></Relationships>"#,
    )?;
    // Deliberately not under `xl/worksheets/`.
    archive.start_file("xl/data/s1.xml", options)?;
    archive.write_all(&sheet)?;
    archive.finish()?;

    let result = convert_with_worker(
        &input,
        "xlsx",
        &workspace.path().join("relocated.pdf"),
        Some(worker()),
    );
    assert!(
        matches!(result, Err(BuiltinEngineError::TooManyRows { .. })),
        "the row bound must follow the relationship, got {result:?}"
    );
    Ok(())
}

/// Builds a workbook whose 30 000-row worksheet lives at `xl/data/s1.xml`,
/// with a caller-supplied `xl/workbook.xml` and `xl/_rels/workbook.xml.rels`.
fn relocated_workbook(
    path: &Path,
    workbook: &[u8],
    workbook_rels: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    const SLIDE_RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

    let mut sheet = Vec::from(*b"<worksheet><sheetData>");
    for _ in 0..30_000 {
        sheet.extend_from_slice(b"<row/>");
    }
    sheet.extend_from_slice(b"</sheetData></worksheet>");

    let mut archive = ZipWriter::new(fs::File::create(path)?);
    let options = SimpleFileOptions::default();
    archive.start_file("[Content_Types].xml", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/data/s1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
    )?;
    archive.start_file("_rels/.rels", options)?;
    archive.write_all(
        format!(
            r#"<?xml version="1.0"?><Relationships xmlns="{SLIDE_RELS_NS}"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        )
        .as_bytes(),
    )?;
    archive.start_file("xl/workbook.xml", options)?;
    archive.write_all(workbook)?;
    archive.start_file("xl/_rels/workbook.xml.rels", options)?;
    archive.write_all(workbook_rels)?;
    archive.start_file("xl/data/s1.xml", options)?;
    archive.write_all(&sheet)?;
    archive.finish()?;
    Ok(())
}

fn plain_workbook() -> Vec<u8> {
    br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_vec()
}

/// A `.rels` too large for the read cap used to parse as nothing, and the
/// filename fallback then found no worksheet because the real one lives at
/// `xl/data/s1.xml` — the original row-bound evasion, reinstated. Sized as the
/// tester's reproduction: over the old 4 MiB cap, under the office sanitizer's
/// 16 MiB cap, so it reaches the engine through the real HTTP path.
#[test]
fn an_oversized_rels_part_cannot_evade_the_row_bound() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let input = workspace.path().join("padded.xlsx");

    let mut padded = Vec::from(
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="data/s1.xml"/>"#
            .as_slice(),
    );
    padded.extend(std::iter::repeat_n(b' ', 5 * 1024 * 1024));
    padded.extend_from_slice(b"</Relationships>");
    relocated_workbook(&input, &plain_workbook(), &padded)?;

    let result = convert_with_worker(
        &input,
        "xlsx",
        &workspace.path().join("padded.pdf"),
        Some(worker()),
    );
    assert!(
        matches!(result, Err(BuiltinEngineError::TooManyRows { .. })),
        "an oversized rels must not make the bound vanish, got {result:?}"
    );
    Ok(())
}

/// One extra wrapper element defeated the parent-element check while the
/// engine's event-based reader still saw every `<sheet>` and read all 30 000
/// rows.
#[test]
fn an_extra_nesting_level_cannot_evade_the_row_bound() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let input = workspace.path().join("nested.xlsx");
    let nested = br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><wrap><sheet name="Sheet1" sheetId="1" r:id="rId1"/></wrap></sheets></workbook>"#;
    let plain_rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="data/s1.xml"/></Relationships>"#;
    relocated_workbook(&input, nested, plain_rels)?;

    let result = convert_with_worker(
        &input,
        "xlsx",
        &workspace.path().join("nested.pdf"),
        Some(worker()),
    );
    assert!(
        matches!(result, Err(BuiltinEngineError::TooManyRows { .. })),
        "an extra nesting level must not make the bound vanish, got {result:?}"
    );
    Ok(())
}

/// `show = "0"` with spaces is legal XML and the engine hides the slide, so
/// substring-matching `show="0"` reported a faithful conversion as degraded.
#[test]
fn a_spaced_show_attribute_is_still_a_hidden_slide() -> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = corpus("sample.pptx") else {
        return Ok(());
    };
    let workspace = TempDir::new()?;
    let input = workspace.path().join("spaced.pptx");
    rewrite_package(&source, &input, &|name, bytes| {
        if name != "ppt/slides/slide2.xml" {
            return Some((name.to_owned(), bytes.to_vec()));
        }
        let patched = String::from_utf8_lossy(bytes).replacen("<p:sld ", "<p:sld show = \"0\" ", 1);
        Some((name.to_owned(), patched.into_bytes()))
    })?;

    let output = workspace.path().join("spaced.pdf");
    let conversion = convert_with_worker(&input, "pptx", &output, Some(worker()))?;
    assert_eq!(page_count(&output)?, 1, "the engine hides the slide");
    assert!(
        !conversion.dropped_content,
        "a hidden slide is not lost content: {:?}",
        conversion.warnings
    );
    Ok(())
}

/// The reverse: `show="0"` inside an unrelated attribute value is not a `show`
/// attribute. The engine renders the slide, so counting it as hidden removed it
/// from the expected count and took the loss signal with it.
#[test]
fn a_decoy_show_in_another_attribute_does_not_silence_the_loss_signal()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = corpus("sample.pptx") else {
        return Ok(());
    };
    let workspace = TempDir::new()?;
    let input = workspace.path().join("decoy.pptx");
    rewrite_package(&source, &input, &|name, bytes| {
        if name != "ppt/slides/slide2.xml" {
            return Some((name.to_owned(), bytes.to_vec()));
        }
        // Keep a well-formed root start tag carrying the decoy, then make the
        // rest unparseable so the engine really does drop the slide.
        let text = String::from_utf8_lossy(bytes);
        let tag_end = text
            .find("<p:sld ")
            .and_then(|start| text[start..].find('>').map(|offset| start + offset + 1))?;
        let mut patched = text[..tag_end].replacen("<p:sld ", "<p:sld descr='x show=\"0\" y' ", 1);
        patched.push_str("<<<not xml at all >>> &&&");
        Some((name.to_owned(), patched.into_bytes()))
    })?;

    let output = workspace.path().join("decoy.pdf");
    let conversion = convert_with_worker(&input, "pptx", &output, Some(worker()))?;
    assert_eq!(
        page_count(&output)?,
        1,
        "the broken slide should be dropped"
    );
    assert!(
        conversion.dropped_content,
        "a decoy attribute must not hide a real drop: {:?}",
        conversion.warnings
    );
    assert!(
        conversion
            .warnings
            .iter()
            .any(|warning| warning.contains("produced no page")),
        "expected an explicit drop warning: {:?}",
        conversion.warnings
    );
    Ok(())
}

/// A hidden slide producing no page is a faithful conversion — `office2pdf`
/// skips `show="0"` slides on purpose, as `PowerPoint`'s own PDF export does.
/// Reporting that as lost content would fire the degraded signal on ordinary
/// decks and teach callers to ignore it.
#[test]
fn a_hidden_slide_is_not_reported_as_dropped_content() -> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = corpus("sample.pptx") else {
        return Ok(());
    };
    let workspace = TempDir::new()?;
    let input = workspace.path().join("hidden.pptx");
    rewrite_package(&source, &input, &|name, bytes| {
        if name != "ppt/slides/slide2.xml" {
            return Some((name.to_owned(), bytes.to_vec()));
        }
        let patched = String::from_utf8_lossy(bytes).replacen("<p:sld ", "<p:sld show=\"0\" ", 1);
        Some((name.to_owned(), patched.into_bytes()))
    })?;

    let output = workspace.path().join("hidden.pdf");
    let conversion = convert_with_worker(&input, "pptx", &output, Some(worker()))?;
    assert!(
        !conversion.dropped_content,
        "a hidden slide is not lost content: {:?}",
        conversion.warnings
    );
    assert!(
        !conversion
            .warnings
            .iter()
            .any(|warning| warning.contains("produced no page")),
        "no drop warning expected: {:?}",
        conversion.warnings
    );
    // What makes this test meaningful: the deck has two slide parts and renders
    // one page, so the reconciliation really is being exercised. If a future
    // engine starts rendering hidden slides this assertion is where to notice —
    // the test would then be passing for the wrong reason.
    assert_eq!(
        page_count(&output)?,
        1,
        "the hidden slide should be skipped"
    );
    Ok(())
}

/// The dangerous direction: slide parts are named by their relationship target,
/// so a deck whose slides are not called `slideN.xml` used to yield an expected
/// count of zero, which skipped reconciliation entirely and reported a dropped
/// slide as a clean conversion.
#[test]
fn a_dropped_slide_is_detected_with_non_conventional_part_names()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = corpus("sample.pptx") else {
        return Ok(());
    };
    let workspace = TempDir::new()?;
    let input = workspace.path().join("renamed.pptx");
    rewrite_package(&source, &input, &|name, bytes| {
        match name {
            // Rename the second slide part away from the convention, and break
            // it so the engine really does drop it.
            "ppt/slides/slide2.xml" => Some((
                "ppt/slides/d2.xml".to_owned(),
                b"<<<not xml at all >>> &&& <p:sld".to_vec(),
            )),
            "ppt/slides/_rels/slide2.xml.rels" => {
                Some(("ppt/slides/_rels/d2.xml.rels".to_owned(), bytes.to_vec()))
            }
            "ppt/_rels/presentation.xml.rels" | "[Content_Types].xml" => Some((
                name.to_owned(),
                String::from_utf8_lossy(bytes)
                    .replace("slides/slide2.xml", "slides/d2.xml")
                    .into_bytes(),
            )),
            _ => Some((name.to_owned(), bytes.to_vec())),
        }
    })?;

    let output = workspace.path().join("renamed.pdf");
    let conversion = convert_with_worker(&input, "pptx", &output, Some(worker()))?;
    assert_eq!(
        page_count(&output)?,
        1,
        "the broken slide should be dropped"
    );
    assert!(
        conversion.dropped_content,
        "a dropped slide must be reported even when parts are not named by convention: {:?}",
        conversion.warnings
    );
    assert!(
        conversion
            .warnings
            .iter()
            .any(|warning| warning.contains("produced no page")),
        "expected an explicit drop warning: {:?}",
        conversion.warnings
    );
    Ok(())
}

/// What a rewrite rule returns for one entry: its new name and bytes, or
/// `None` to drop it.
type RewrittenEntry = Option<(String, Vec<u8>)>;

/// Rewrites the second slide of the sample deck: `attributes` is inserted into
/// its root start tag, `prologue` is placed before that tag, and `break_body`
/// replaces everything after it with unparseable text.
fn deck_with_patched_slide(
    destination: &Path,
    prologue: &str,
    attributes: &str,
    break_body: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(source) = corpus("sample.pptx") else {
        return Ok(false);
    };
    rewrite_package(&source, destination, &|name, bytes| {
        if name != "ppt/slides/slide2.xml" {
            return Some((name.to_owned(), bytes.to_vec()));
        }
        let text = String::from_utf8_lossy(bytes);
        let open = text.find("<p:sld ")?;
        let close = text[open..].find('>').map(|offset| open + offset + 1)?;
        let tag = text[open..close].replacen("<p:sld ", &format!("<p:sld {attributes} "), 1);
        let mut patched = String::from(prologue);
        patched.push_str(&tag);
        if break_body {
            patched.push_str("<<<not xml at all >>> &&&");
        } else {
            patched.push_str(&text[close..]);
        }
        Some((name.to_owned(), patched.into_bytes()))
    })?;
    Ok(true)
}

/// The scanner disagreeing with the engine is a wrong verdict in one direction
/// or the other, so these assert against what the engine actually produced
/// rather than against this crate's own expectation.
///
/// Each fixture leaves slide 2's body intact, so the only reason it can fail to
/// render is that the engine decided it was hidden — which is a faithful
/// conversion. Whatever the engine decides, nothing here may be reported as
/// lost content.
///
/// Measured engine behaviour, so a future reader knows which fixtures bite
/// here: `quoted-gt`, `bad-attribute`, and `unquoted-attribute` render one page
/// (the engine hides the slide) and used to be reported degraded.
/// `doctype-subset` and `signed-charref` render two, so they pass this test
/// either way — their sharp assertion is in
/// [`a_misread_start_tag_cannot_mask_a_dropped_slide`].
#[test]
fn awkward_start_tags_never_produce_a_false_degraded_verdict()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let cases: &[(&str, &str, &str)] = &[
        // A `>` inside an attribute value used to truncate the tag before
        // `show`, so the engine hid the slide while this pass counted it.
        ("quoted-gt", "", r#"foo="a>b" show="0""#),
        // A malformed attribute used to abandon the whole tag; quick-xml skips
        // it and finds `show`.
        ("bad-attribute", "", r#"bad show="0""#),
        ("unquoted-attribute", "", r#"bad=unquoted show="0""#),
        // A doctype whose internal subset contains a decoy root element.
        (
            "doctype-subset",
            r#"<!DOCTYPE p:sld [<!ENTITY e "A>B<p:sld show='0'/>">]>"#,
            "",
        ),
        // A signed character reference is not a character reference, so the
        // engine renders this slide.
        ("signed-charref", "", r#"show="&#+48;""#),
    ];

    for (label, prologue, attributes) in cases {
        let input = workspace.path().join(format!("{label}.pptx"));
        if !deck_with_patched_slide(&input, prologue, attributes, false)? {
            return Ok(());
        }
        let output = workspace.path().join(format!("{label}.pdf"));
        let conversion = convert_with_worker(&input, "pptx", &output, Some(worker()))?;
        let pages = page_count(&output)?;
        assert!(
            pages == 1 || pages == 2,
            "{label}: unexpected page count {pages}"
        );
        assert!(
            !conversion.dropped_content,
            "{label}: engine rendered {pages} page(s) with the slide body intact, \
             so nothing was lost, but the conversion was reported degraded: {:?}",
            conversion.warnings
        );
    }
    Ok(())
}

/// The dangerous direction. Both fixtures make this pass believe slide 2 is
/// hidden when the engine does not, which lowers the expected page count until
/// `pages >= expected` holds and a genuinely dropped slide goes unreported.
#[test]
fn a_misread_start_tag_cannot_mask_a_dropped_slide() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let cases: &[(&str, &str, &str)] = &[
        // `&#+48;` resolved to `0` here but is rejected by the engine.
        ("masking-signed-charref", "", r#"show="&#+48;""#),
        // A decoy `<p:sld show='0'/>` inside a doctype internal subset used to
        // be mistaken for the root element.
        (
            "masking-doctype-subset",
            r#"<!DOCTYPE p:sld [<!ENTITY e "A>B<p:sld show='0'/>">]>"#,
            "",
        ),
    ];

    for (label, prologue, attributes) in cases {
        let input = workspace.path().join(format!("{label}.pptx"));
        if !deck_with_patched_slide(&input, prologue, attributes, true)? {
            return Ok(());
        }
        let output = workspace.path().join(format!("{label}.pdf"));
        let conversion = convert_with_worker(&input, "pptx", &output, Some(worker()))?;
        assert_eq!(
            page_count(&output)?,
            1,
            "{label}: the broken slide should not have rendered"
        );
        assert!(
            conversion.dropped_content,
            "{label}: a slide was dropped and the conversion was reported clean: {:?}",
            conversion.warnings
        );
    }
    Ok(())
}

/// Copies a ZIP package entry by entry, letting `rewrite` rename an entry,
/// change its bytes, or drop it.
fn rewrite_package(
    source: &Path,
    destination: &Path,
    rewrite: &dyn Fn(&str, &[u8]) -> RewrittenEntry,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut input = zip::ZipArchive::new(fs::File::open(source)?)?;
    let mut output = ZipWriter::new(fs::File::create(destination)?);
    let options = SimpleFileOptions::default();
    for index in 0..input.len() {
        let mut entry = input.by_index(index)?;
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if let Some((new_name, new_bytes)) = rewrite(&name, &bytes) {
            output.start_file(new_name, options)?;
            output.write_all(&new_bytes)?;
        }
    }
    output.finish()?;
    Ok(())
}

/// Runs the worker directly with no arguments to prove the mode exists and
/// refuses malformed invocations instead of falling through into the service.
#[test]
fn the_worker_mode_refuses_a_malformed_invocation() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(worker()).arg(WORKER_ARGUMENT).output()?;
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    Ok(())
}

/// Builds a DOCX whose embedded font declares `<base>` as its name, with an
/// all-zero font key so the XOR deobfuscation is the identity.
fn build_font_traversal_docx(path: &Path, base: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = fs::File::create(path)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    archive.start_file("[Content_Types].xml", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="fntdata" ContentType="application/octet-stream"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
    )?;

    archive.start_file("_rels/.rels", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
    )?;

    archive.start_file("word/document.xml", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>hello</w:t></w:r></w:p></w:body></w:document>"#,
    )?;

    archive.start_file("word/fontTable.xml", options)?;
    archive.write_all(
        format!(
            r#"<?xml version="1.0"?><w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:font w:name="{base}"><w:embedRegular r:id="rId99" w:fontKey="{{00000000-0000-0000-0000-000000000000}}"/></w:font></w:fonts>"#
        )
        .as_bytes(),
    )?;

    archive.start_file("word/_rels/fontTable.xml.rels", options)?;
    archive.write_all(
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/font1.fntdata"/></Relationships>"#,
    )?;

    archive.start_file("word/fonts/font1.fntdata", options)?;
    let mut font = vec![0x00, 0x01, 0x00, 0x00];
    font.extend(std::iter::repeat_n(0x41_u8, 200));
    archive.write_all(&font)?;

    archive.finish()?;
    Ok(())
}
