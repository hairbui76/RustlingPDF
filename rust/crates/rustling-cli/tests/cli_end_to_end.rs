use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use lopdf::{Document, Object, Stream, dictionary};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn runs_single_operation_and_existing_pipeline_shape_locally()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let input = workspace.path().join("input.pdf");
    let single_output = workspace.path().join("single.pdf");
    let pipeline_output = workspace.path().join("pipeline.pdf");
    fs::write(&input, pdf_with_rotation(0)?)?;

    let single = command()
        .args([
            "run",
            "general-rotate-pdf",
            "--input",
            path_text(&input)?,
            "--output",
            path_text(&single_output)?,
            "--param",
            "angle=90",
        ])
        .output()?;
    require_exit(&single, 0)?;
    assert!(single.stdout.is_empty(), "binary output leaked to stdout");
    assert_eq!(page_rotation(&single_output)?, 90);

    let spec = workspace.path().join("pipeline.json");
    fs::write(
        &spec,
        serde_json::to_vec_pretty(&json!({
            "pipeline": [
                {"operation": "general-rotate-pdf", "parameters": {"angle": 90}},
                {"operation": "/api/v1/general/rotate-pdf", "parameters": {"angle": 90}}
            ]
        }))?,
    )?;
    let pipeline = command()
        .args([
            "pipeline",
            "--spec",
            path_text(&spec)?,
            "--input",
            path_text(&input)?,
            "--output",
            path_text(&pipeline_output)?,
        ])
        .output()?;
    require_exit(&pipeline, 0)?;
    assert!(pipeline.stdout.is_empty(), "binary output leaked to stdout");
    assert_eq!(page_rotation(&pipeline_output)?, 180);
    Ok(())
}

#[test]
fn writes_binary_stdout_only_when_explicitly_requested() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TempDir::new()?;
    let input = workspace.path().join("input.pdf");
    fs::write(&input, pdf_with_rotation(0)?)?;
    let output = command()
        .args([
            "run",
            "general-rotate-pdf",
            "--input",
            path_text(&input)?,
            "--output",
            "-",
            "--param",
            "angle=270",
        ])
        .output()?;
    require_exit(&output, 0)?;
    assert!(output.stdout.starts_with(b"%PDF-"));
    assert_eq!(page_rotation_bytes(&output.stdout)?, 270);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("to stdout"),
        "stdout mode should still report status on stderr"
    );
    Ok(())
}

#[test]
fn executable_exit_codes_distinguish_usage_io_rejection_and_dependency()
-> Result<(), Box<dyn std::error::Error>> {
    let usage = command()
        .args([
            "run",
            "general-rotate-pdf",
            "--input",
            "missing.pdf",
            "--output",
            "unused.pdf",
            "--param",
            "angle=45",
        ])
        .output()?;
    require_exit(&usage, 2)?;

    let local_io = command()
        .args([
            "run",
            "general-rotate-pdf",
            "--input",
            "missing.pdf",
            "--output",
            "unused.pdf",
            "--param",
            "angle=90",
        ])
        .output()?;
    require_exit(&local_io, 3)?;

    let workspace = TempDir::new()?;
    let input = workspace.path().join("input.pdf");
    fs::write(&input, pdf_with_rotation(0)?)?;
    let rejected_output = workspace.path().join("rejected.pdf");
    let rejected = command()
        .args([
            "run",
            "general-crop",
            "--input",
            path_text(&input)?,
            "--output",
            path_text(&rejected_output)?,
        ])
        .output()?;
    require_exit(&rejected, 4)?;

    let unavailable_output = workspace.path().join("unavailable.pdf");
    let unavailable = command()
        .env(
            "RUSTLING_PROCESSING_SOFFICE_COMMAND",
            workspace.path().join("missing-soffice"),
        )
        .args([
            "run",
            "convert-file-pdf",
            "--input",
            path_text(&input)?,
            "--output",
            path_text(&unavailable_output)?,
        ])
        .output()?;
    require_exit(&unavailable, 5)?;
    Ok(())
}

#[test]
fn operations_json_is_machine_readable_catalog_metadata() -> Result<(), Box<dyn std::error::Error>>
{
    let output = command().args(["operations", "--json"]).output()?;
    require_exit(&output, 0)?;
    let operations: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let operations = operations
        .as_array()
        .ok_or("operations output must be an array")?;
    assert_eq!(operations.len(), 67);
    assert!(operations.iter().any(|operation| {
        operation["id"] == "general-rotate-pdf" && operation["path"] == "/api/v1/general/rotate-pdf"
    }));
    Ok(())
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlingpdf"))
}

fn require_exit(output: &Output, expected: i32) -> Result<(), Box<dyn std::error::Error>> {
    if output.status.code() == Some(expected) {
        return Ok(());
    }
    Err(format!(
        "expected exit {expected}, received {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .into())
}

fn path_text(path: &Path) -> Result<&str, Box<dyn std::error::Error>> {
    path.to_str()
        .ok_or_else(|| format!("test path is not UTF-8: {}", path.display()).into())
}

fn pdf_with_rotation(rotation: i64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut document = Document::with_version("1.7");
    let page_tree_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => page_tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Rotate" => rotation,
    });
    document.objects.insert(
        page_tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => page_tree_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn page_rotation(path: &Path) -> Result<i64, Box<dyn std::error::Error>> {
    let document = Document::load(path)?;
    first_page_rotation(&document)
}

fn page_rotation_bytes(bytes: &[u8]) -> Result<i64, Box<dyn std::error::Error>> {
    let document = Document::load_mem(bytes)?;
    first_page_rotation(&document)
}

fn first_page_rotation(document: &Document) -> Result<i64, Box<dyn std::error::Error>> {
    let page_id = document
        .get_pages()
        .into_values()
        .next()
        .ok_or("output PDF has no pages")?;
    Ok(document.get_dictionary(page_id)?.get(b"Rotate")?.as_i64()?)
}
