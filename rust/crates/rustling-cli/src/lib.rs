//! Local, catalog-backed command-line automation for RustlingPDF.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fmt::{self, Display, Formatter},
    io,
    path::{Path, PathBuf},
    pin::Pin,
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{
    body::{Body, Bytes, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use clap::{Args, Parser, Subcommand};
use futures_util::{Stream, StreamExt, stream};
use rustling_processing::{ProcessingRuntime, max_upload_bytes_from_environment};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tempfile::Builder;
use tokio::{
    fs::File,
    io::{AsyncWrite, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;
use tower::ServiceExt;

const PIPELINE_PATH: &str = "/api/v1/pipeline/handleData";
const ERROR_BODY_LIMIT_BYTES: usize = 64 * 1024;
const STDOUT_PATH: &str = "-";
static BOUNDARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// One operation binding generated from the committed operation catalog.
#[derive(Clone, Copy, Debug)]
pub struct OperationBinding {
    pub id: &'static str,
    pub path: &'static str,
    pub title: &'static str,
    pub schema_json: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/operation_bindings.rs"));

/// RustlingPDF's local command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "rustlingpdf",
    version,
    about = "Run RustlingPDF operations locally without starting a server"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List catalog-backed operations.
    Operations {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one operation's parameter schema.
    Describe {
        /// Generated operation ID or canonical /api/v1/... path.
        operation: String,
        /// Emit a machine-readable object instead of the schema alone.
        #[arg(long)]
        json: bool,
    },
    /// Run one catalog operation on local input files.
    Run(RunArguments),
    /// Run an existing RustlingPDF pipeline JSON file on local input files.
    Pipeline(PipelineArguments),
}

#[derive(Debug, Args)]
pub struct RunArguments {
    /// Generated operation ID or canonical /api/v1/... path.
    pub operation: String,
    /// Local input file. Repeat for multi-input operations.
    #[arg(short = 'i', long = "input", required = true)]
    pub inputs: Vec<PathBuf>,
    /// Output file, or '-' for binary stdout.
    #[arg(short = 'o', long = "output")]
    pub output: PathBuf,
    /// Parameter as key=value. Values use JSON types when valid; repeat a key for an array.
    #[arg(short = 'p', long = "param", value_name = "KEY=VALUE")]
    pub parameters: Vec<String>,
    /// Base parameter object as inline JSON or @path/to/parameters.json.
    #[arg(long, value_name = "JSON|@FILE")]
    pub params_json: Option<String>,
    /// Atomically replace an existing output file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct PipelineArguments {
    /// Pipeline JSON using the existing {"pipeline":[...]} API shape.
    #[arg(long)]
    pub spec: PathBuf,
    /// Local input file. Repeat for multiple initial inputs.
    #[arg(short = 'i', long = "input", required = true)]
    pub inputs: Vec<PathBuf>,
    /// Output file, or '-' for binary stdout.
    #[arg(short = 'o', long = "output")]
    pub output: PathBuf,
    /// Atomically replace an existing output file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FailureKind {
    Usage,
    Io,
    Rejected,
    Unavailable,
    Internal,
}

impl FailureKind {
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Usage => 2,
            Self::Io => 3,
            Self::Rejected => 4,
            Self::Unavailable => 5,
            Self::Internal => 6,
        }
    }
}

#[derive(Debug)]
pub struct CliError {
    kind: FailureKind,
    message: String,
}

impl CliError {
    fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> FailureKind {
        self.kind
    }

    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.kind.exit_code())
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

#[derive(Debug, Deserialize, Serialize)]
struct PipelineDocument {
    pipeline: Vec<PipelineStep>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PipelineStep {
    operation: String,
    #[serde(default)]
    parameters: BTreeMap<String, Value>,
    #[serde(
        default,
        rename = "fileParameters",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    file_parameters: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct OperationDescription<'a> {
    id: &'a str,
    path: &'a str,
    title: &'a str,
    schema: Value,
}

/// Run a parsed CLI request.
///
/// # Errors
///
/// Returns a classified error whose stable exit code is suitable for scripts.
pub async fn execute(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Operations { json } => print_operations(json),
        Command::Describe { operation, json } => describe_operation(&operation, json),
        Command::Run(arguments) => run_operation(arguments).await,
        Command::Pipeline(arguments) => run_pipeline(arguments).await,
    }
}

fn print_operations(json: bool) -> Result<(), CliError> {
    if json {
        let descriptions = GENERATED_OPERATIONS
            .iter()
            .map(operation_description)
            .collect::<Result<Vec<_>, _>>()?;
        print_json(&descriptions)?;
    } else {
        for operation in GENERATED_OPERATIONS {
            println!("{}\t{}", operation.id, operation.path);
        }
    }
    Ok(())
}

fn describe_operation(reference: &str, json: bool) -> Result<(), CliError> {
    let operation = resolve_operation(reference)?;
    if json {
        print_json(&operation_description(operation)?)?;
    } else {
        let schema = parse_schema(operation)?;
        print_json(&schema)?;
    }
    Ok(())
}

async fn run_operation(arguments: RunArguments) -> Result<(), CliError> {
    let operation = resolve_operation(&arguments.operation)?;
    let parameters =
        build_parameters(arguments.params_json.as_deref(), &arguments.parameters).await?;
    validate_parameters(operation, &parameters)?;
    let document = PipelineDocument {
        pipeline: vec![PipelineStep {
            operation: operation.path.to_owned(),
            parameters,
            file_parameters: BTreeMap::new(),
        }],
    };
    execute_pipeline(
        &arguments.inputs,
        &arguments.output,
        arguments.force,
        &document,
    )
    .await
}

async fn run_pipeline(arguments: PipelineArguments) -> Result<(), CliError> {
    let text = tokio::fs::read_to_string(&arguments.spec)
        .await
        .map_err(|error| {
            CliError::new(
                FailureKind::Io,
                format!(
                    "cannot read pipeline spec '{}': {error}",
                    arguments.spec.display()
                ),
            )
        })?;
    let mut document: PipelineDocument = serde_json::from_str(&text).map_err(|error| {
        CliError::new(
            FailureKind::Usage,
            format!(
                "pipeline spec '{}' is invalid: {error}",
                arguments.spec.display()
            ),
        )
    })?;
    validate_pipeline(&mut document)?;
    execute_pipeline(
        &arguments.inputs,
        &arguments.output,
        arguments.force,
        &document,
    )
    .await
}

fn validate_pipeline(document: &mut PipelineDocument) -> Result<(), CliError> {
    if document.pipeline.is_empty() {
        return Err(CliError::new(
            FailureKind::Usage,
            "pipeline must contain at least one operation",
        ));
    }
    for (index, step) in document.pipeline.iter_mut().enumerate() {
        if !step.file_parameters.is_empty() {
            return Err(CliError::new(
                FailureKind::Usage,
                format!(
                    "pipeline step {} uses fileParameters, which the local CLI does not accept",
                    index + 1
                ),
            ));
        }
        let operation = resolve_operation(&step.operation).map_err(|error| {
            CliError::new(
                error.kind,
                format!("pipeline step {}: {}", index + 1, error.message),
            )
        })?;
        validate_parameters(operation, &step.parameters).map_err(|error| {
            CliError::new(
                error.kind,
                format!("pipeline step {}: {}", index + 1, error.message),
            )
        })?;
        operation.path.clone_into(&mut step.operation);
    }
    Ok(())
}

fn resolve_operation(reference: &str) -> Result<&'static OperationBinding, CliError> {
    GENERATED_OPERATIONS
        .iter()
        .find(|operation| operation.id == reference || operation.path == reference)
        .ok_or_else(|| {
            CliError::new(
                FailureKind::Usage,
                format!(
                    "unknown operation '{reference}'; run 'rustlingpdf operations' to list supported operations"
                ),
            )
        })
}

fn operation_description(
    operation: &'static OperationBinding,
) -> Result<OperationDescription<'static>, CliError> {
    Ok(OperationDescription {
        id: operation.id,
        path: operation.path,
        title: operation.title,
        schema: parse_schema(operation)?,
    })
}

fn parse_schema(operation: &OperationBinding) -> Result<Value, CliError> {
    serde_json::from_str(operation.schema_json).map_err(|error| {
        CliError::new(
            FailureKind::Internal,
            format!(
                "compiled schema for operation '{}' is invalid: {error}",
                operation.id
            ),
        )
    })
}

fn validate_parameters(
    operation: &OperationBinding,
    parameters: &BTreeMap<String, Value>,
) -> Result<(), CliError> {
    let schema = parse_schema(operation)?;
    let instance = serde_json::to_value(parameters).map_err(|error| {
        CliError::new(
            FailureKind::Internal,
            format!("cannot serialize parameters for validation: {error}"),
        )
    })?;
    jsonschema::validate(&schema, &instance).map_err(|error| {
        CliError::new(
            FailureKind::Usage,
            format!("parameters for '{}' are invalid: {error}", operation.id),
        )
    })
}

async fn build_parameters(
    params_json: Option<&str>,
    flags: &[String],
) -> Result<BTreeMap<String, Value>, CliError> {
    let mut parameters = match params_json {
        Some(source) => read_parameter_object(source).await?,
        None => BTreeMap::new(),
    };
    let mut flag_names = BTreeSet::new();
    for flag in flags {
        let (name, raw_value) = flag.split_once('=').ok_or_else(|| {
            CliError::new(
                FailureKind::Usage,
                format!("parameter '{flag}' must use key=value syntax"),
            )
        })?;
        if name.is_empty() {
            return Err(CliError::new(
                FailureKind::Usage,
                "parameter name cannot be empty",
            ));
        }
        let value = parse_parameter_value(raw_value);
        if flag_names.insert(name.to_owned()) {
            parameters.insert(name.to_owned(), value);
        } else {
            append_parameter_value(&mut parameters, name, value);
        }
    }
    Ok(parameters)
}

async fn read_parameter_object(source: &str) -> Result<BTreeMap<String, Value>, CliError> {
    let text = if let Some(path) = source.strip_prefix('@') {
        if path.is_empty() {
            return Err(CliError::new(
                FailureKind::Usage,
                "--params-json @FILE requires a file path",
            ));
        }
        tokio::fs::read_to_string(path).await.map_err(|error| {
            CliError::new(
                FailureKind::Io,
                format!("cannot read parameter file '{path}': {error}"),
            )
        })?
    } else {
        source.to_owned()
    };
    let object: Map<String, Value> = serde_json::from_str(&text).map_err(|error| {
        CliError::new(
            FailureKind::Usage,
            format!("--params-json must be a JSON object: {error}"),
        )
    })?;
    Ok(object.into_iter().collect())
}

fn parse_parameter_value(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned()))
}

fn append_parameter_value(parameters: &mut BTreeMap<String, Value>, name: &str, value: Value) {
    match parameters.get_mut(name) {
        Some(Value::Array(values)) => values.push(value),
        Some(existing) => {
            let first = std::mem::replace(existing, Value::Null);
            *existing = Value::Array(vec![first, value]);
        }
        None => {
            parameters.insert(name.to_owned(), value);
        }
    }
}

async fn execute_pipeline(
    inputs: &[PathBuf],
    output: &Path,
    force: bool,
    document: &PipelineDocument,
) -> Result<(), CliError> {
    validate_inputs(inputs).await?;
    validate_output(output, force)?;
    let config = serde_json::to_string(document).map_err(|error| {
        CliError::new(
            FailureKind::Internal,
            format!("cannot serialize pipeline request: {error}"),
        )
    })?;
    let request = build_pipeline_request(inputs, &config).await?;
    let runtime = ProcessingRuntime::from_environment_with_dependency_discovery(
        max_upload_bytes_from_environment(),
    );
    for step in &document.pipeline {
        let availability = runtime.endpoint_availability_for_uri(&step.operation);
        if !availability.is_enabled() {
            let kind = if availability.reason() == Some("DEPENDENCY") {
                FailureKind::Unavailable
            } else {
                FailureKind::Rejected
            };
            return Err(CliError::new(
                kind,
                format!(
                    "operation '{}' is disabled ({})",
                    step.operation,
                    availability.reason().unwrap_or("CONFIG")
                ),
            ));
        }
    }
    let response = runtime
        .into_router()
        .oneshot(request)
        .await
        .map_err(|error| match error {})?;
    handle_response(response, output, force).await
}

async fn validate_inputs(inputs: &[PathBuf]) -> Result<(), CliError> {
    for input in inputs {
        let metadata = tokio::fs::metadata(input).await.map_err(|error| {
            CliError::new(
                FailureKind::Io,
                format!("cannot read input '{}': {error}", input.display()),
            )
        })?;
        if !metadata.is_file() {
            return Err(CliError::new(
                FailureKind::Io,
                format!("input '{}' is not a regular file", input.display()),
            ));
        }
    }
    Ok(())
}

fn validate_output(output: &Path, force: bool) -> Result<(), CliError> {
    if output == Path::new(STDOUT_PATH) {
        return Ok(());
    }
    if output.is_dir() {
        return Err(CliError::new(
            FailureKind::Io,
            format!("output '{}' is a directory", output.display()),
        ));
    }
    if output.exists() && !force {
        return Err(CliError::new(
            FailureKind::Io,
            format!(
                "output '{}' already exists; pass --force to replace it",
                output.display()
            ),
        ));
    }
    let parent = output.parent().filter(|path| !path.as_os_str().is_empty());
    if parent.is_some_and(|path| !path.is_dir()) {
        return Err(CliError::new(
            FailureKind::Io,
            format!("output directory for '{}' does not exist", output.display()),
        ));
    }
    Ok(())
}

async fn build_pipeline_request(
    inputs: &[PathBuf],
    config: &str,
) -> Result<Request<Body>, CliError> {
    let boundary = format!(
        "rustlingpdf-cli-{}-{}",
        std::process::id(),
        BOUNDARY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut parts = Vec::new();
    for input in inputs {
        let filename = multipart_filename(input);
        parts.push(bytes_part(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"fileInput\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )));
        let file = File::open(input).await.map_err(|error| {
            CliError::new(
                FailureKind::Io,
                format!("cannot open input '{}': {error}", input.display()),
            )
        })?;
        parts.push(Box::pin(ReaderStream::new(file)));
        parts.push(bytes_part("\r\n"));
    }
    parts.push(bytes_part(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"json\"\r\nContent-Type: application/json\r\n\r\n{config}\r\n--{boundary}--\r\n"
    )));
    Request::builder()
        .method("POST")
        .uri(PIPELINE_PATH)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from_stream(stream::iter(parts).flatten()))
        .map_err(|error| {
            CliError::new(
                FailureKind::Internal,
                format!("cannot build local pipeline request: {error}"),
            )
        })
}

type MultipartStream = Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>;

fn bytes_part(value: impl Into<Vec<u8>>) -> MultipartStream {
    let bytes = Bytes::from(value.into());
    stream::once(async move { Ok(bytes) }).boxed()
}

fn multipart_filename(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("input.bin")
        .chars()
        .map(|character| match character {
            '"' | '\r' | '\n' => '_',
            other => other,
        })
        .collect()
}

async fn handle_response(response: Response, output: &Path, force: bool) -> Result<(), CliError> {
    let status = response.status();
    if !status.is_success() {
        return Err(response_error(response, status).await);
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let suggested_filename = response_filename(response.headers());
    let mut body = response.into_body().into_data_stream();
    if output == Path::new(STDOUT_PATH) {
        let mut stdout = tokio::io::stdout();
        copy_response(&mut body, &mut stdout).await?;
        stdout.flush().await.map_err(|error| output_error(&error))?;
        eprintln!("RustlingPDF wrote {content_type} to stdout");
        return Ok(());
    }

    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = Builder::new()
        .prefix(".rustlingpdf-")
        .tempfile_in(parent)
        .map_err(|error| output_error(&error))?;
    let writer = temporary.reopen().map_err(|error| output_error(&error))?;
    let mut writer = File::from_std(writer);
    copy_response(&mut body, &mut writer).await?;
    writer.flush().await.map_err(|error| output_error(&error))?;
    writer
        .sync_all()
        .await
        .map_err(|error| output_error(&error))?;
    drop(writer);
    if force {
        temporary
            .persist(output)
            .map_err(|error| output_error(&error.error))?;
    } else {
        temporary
            .persist_noclobber(output)
            .map_err(|error| output_error(&error.error))?;
    }
    if let Some(filename) = suggested_filename {
        eprintln!(
            "RustlingPDF wrote '{}' ({content_type}; server filename: {filename})",
            output.display()
        );
    } else {
        eprintln!("RustlingPDF wrote '{}' ({content_type})", output.display());
    }
    Ok(())
}

async fn copy_response(
    body: &mut (impl Stream<Item = Result<Bytes, axum::Error>> + Unpin),
    output: &mut (impl AsyncWrite + Unpin),
) -> Result<(), CliError> {
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| {
            CliError::new(
                FailureKind::Internal,
                format!("cannot read local processing response: {error}"),
            )
        })?;
        output
            .write_all(&chunk)
            .await
            .map_err(|error| output_error(&error))?;
    }
    Ok(())
}

async fn response_error(response: Response, status: StatusCode) -> CliError {
    let body = to_bytes(response.into_body(), ERROR_BODY_LIMIT_BYTES)
        .await
        .map_or_else(
            |error| format!("cannot read error response: {error}"),
            |bytes| String::from_utf8_lossy(&bytes).trim().to_owned(),
        );
    let kind = if status == StatusCode::SERVICE_UNAVAILABLE {
        FailureKind::Unavailable
    } else if status.is_client_error() {
        FailureKind::Rejected
    } else {
        FailureKind::Internal
    };
    let detail = if body.is_empty() {
        status
            .canonical_reason()
            .unwrap_or("processing failed")
            .to_owned()
    } else {
        body
    };
    CliError::new(
        kind,
        format!("local processing returned {status}: {detail}"),
    )
}

fn response_filename(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()?;
    value.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        name.eq_ignore_ascii_case("filename")
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn output_error(error: &io::Error) -> CliError {
    CliError::new(FailureKind::Io, format!("cannot write output: {error}"))
}

fn print_json(value: &impl Serialize) -> Result<(), CliError> {
    let output = serde_json::to_string_pretty(value).map_err(|error| {
        CliError::new(
            FailureKind::Internal,
            format!("cannot serialize CLI metadata: {error}"),
        )
    })?;
    println!("{output}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, FailureKind, GENERATED_OPERATIONS, PipelineDocument, append_parameter_value,
        build_parameters, parse_parameter_value, resolve_operation, validate_parameters,
        validate_pipeline,
    };
    use clap::Parser;
    use serde_json::{Value, json};

    #[test]
    fn golden_run_invocation_parses() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::try_parse_from([
            "rustlingpdf",
            "run",
            "general-rotate-pdf",
            "--input",
            "one.pdf",
            "-i",
            "two.pdf",
            "--output",
            "rotated.zip",
            "--param",
            "angle=90",
            "--force",
        ])?;
        let super::Command::Run(arguments) = cli.command else {
            return Err("expected run command".into());
        };
        assert_eq!(arguments.inputs.len(), 2);
        assert_eq!(arguments.parameters, ["angle=90"]);
        assert!(arguments.force);
        Ok(())
    }

    #[test]
    fn generated_bindings_match_committed_catalog() -> Result<(), Box<dyn std::error::Error>> {
        let committed: serde_json::Map<String, Value> = serde_json::from_str(include_str!(
            "../../rustling-ai-engine/src/operation_catalog.json"
        ))?;
        assert_eq!(GENERATED_OPERATIONS.len(), committed.len());
        for operation in GENERATED_OPERATIONS {
            assert_eq!(
                serde_json::from_str::<Value>(operation.schema_json)?,
                committed[operation.path]
            );
        }
        Ok(())
    }

    #[test]
    fn generated_operation_ids_are_unique_and_resolvable() -> Result<(), Box<dyn std::error::Error>>
    {
        let ids = GENERATED_OPERATIONS
            .iter()
            .map(|operation| operation.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), GENERATED_OPERATIONS.len());
        for operation in GENERATED_OPERATIONS {
            assert_eq!(resolve_operation(operation.id)?.path, operation.path);
            assert_eq!(resolve_operation(operation.path)?.id, operation.id);
        }
        Ok(())
    }

    #[tokio::test]
    async fn parameter_flags_use_json_types_and_repeated_arrays()
    -> Result<(), Box<dyn std::error::Error>> {
        let parameters = build_parameters(
            Some(r#"{"angle":180,"ignored":"base"}"#),
            &[
                "angle=90".to_owned(),
                "pageNumbers=1".to_owned(),
                "pageNumbers=3".to_owned(),
                "word=hello".to_owned(),
            ],
        )
        .await?;
        assert_eq!(parameters["angle"], 90);
        assert_eq!(parameters["pageNumbers"], json!([1, 3]));
        assert_eq!(parameters["word"], "hello");
        assert_eq!(parse_parameter_value("false"), false);
        Ok(())
    }

    #[test]
    fn schema_validation_uses_catalog_required_and_enum_rules()
    -> Result<(), Box<dyn std::error::Error>> {
        let operation = resolve_operation("general-rotate-pdf")?;
        let valid = serde_json::from_value(json!({"angle": 90}))?;
        assert!(validate_parameters(operation, &valid).is_ok());

        let invalid = serde_json::from_value(json!({"angle": 45}))?;
        let error = match validate_parameters(operation, &invalid) {
            Ok(()) => return Err("45 must be rejected by the catalog schema".into()),
            Err(error) => error,
        };
        assert_eq!(error.kind(), FailureKind::Usage);
        Ok(())
    }

    #[test]
    fn invalid_pipeline_step_has_usage_exit_code() -> Result<(), Box<dyn std::error::Error>> {
        let mut document: PipelineDocument = serde_json::from_value(json!({
            "pipeline": [{"operation": "does-not-exist"}]
        }))?;
        let error = match validate_pipeline(&mut document) {
            Ok(()) => return Err("unknown operation must be rejected".into()),
            Err(error) => error,
        };
        assert_eq!(error.kind().exit_code(), 2);
        Ok(())
    }

    #[test]
    fn appends_repeated_values_without_losing_first_value() {
        let mut parameters = std::collections::BTreeMap::new();
        append_parameter_value(&mut parameters, "pages", json!(1));
        append_parameter_value(&mut parameters, "pages", json!(2));
        assert_eq!(parameters["pages"], json!([1, 2]));
    }

    #[test]
    fn exit_code_policy_is_stable() {
        assert_eq!(FailureKind::Usage.exit_code(), 2);
        assert_eq!(FailureKind::Io.exit_code(), 3);
        assert_eq!(FailureKind::Rejected.exit_code(), 4);
        assert_eq!(FailureKind::Unavailable.exit_code(), 5);
        assert_eq!(FailureKind::Internal.exit_code(), 6);
    }
}
