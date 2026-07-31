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
