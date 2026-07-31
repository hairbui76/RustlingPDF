//! Client-supplied document-classification labels and PDF labelling workflow.
//!
//! The label vocabulary is owned by the client and supplied with each request.
//! The source PDF stays in this process: only a bounded first/last-page text
//! window and the supplied label names are sent to the configured Rust AI
//! engine.

use std::{collections::BTreeSet, fs, io::Read as _, path::Path, sync::Arc};

use axum::{
    Router,
    extract::{Extension, Multipart},
    http::StatusCode,
    response::Response,
    routing::post,
};
use reqwest::{
    blocking::Client,
    header::{ACCEPT, CONTENT_TYPE},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::TempDir;
use thiserror::Error;
use tokio::task;

use crate::{
    AI_TOOL_MAX_INPUT_BYTES, ApiError, drain_field, file_response,
    pdf_ai_comments::AiCommentEngineSettings,
    pdf_metadata::set_classification_metadata_to_file,
    pdfium_backend::{
        PdfiumAiPageWindowAttempt, PdfiumTextError, try_extract_ai_page_window_content,
    },
    safe_filename, write_field_to_file_bounded,
};

pub(crate) const CLASSIFY_AND_LABEL_PATH: &str = "/api/v1/ai/tools/classify-and-label";

const MAX_LABELS: usize = 500;
const MAX_TEXT_LENGTH: usize = 128;
const WINDOW_PAGES_PER_EDGE: usize = 2;
const MAX_TEXT_CHARACTERS_PER_PAGE: usize = 4_000;
const MAX_ENGINE_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_ENGINE_RESPONSE_BYTES_U64: u64 = 256 * 1024;
const CLASSIFY_ENGINE_PATH: &str = "/api/v1/documents/classify";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClassificationLabel {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClassificationLabels {
    labels: Vec<ClassificationLabel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClassificationLabelsRequest {
    labels: Option<Vec<ClassificationLabel>>,
}

#[derive(Debug, Error)]
pub(crate) enum ClassificationError {
    #[error("Labels are required")]
    LabelsRequired,
    #[error("Too many labels (max {MAX_LABELS})")]
    TooManyLabels,
    #[error("{field} must not be blank")]
    BlankText { field: &'static str },
    #[error("{field} is too long (max {MAX_TEXT_LENGTH} characters)")]
    TextTooLong { field: &'static str },
    #[error("Invalid label icon: {0}")]
    InvalidIcon(String),
    #[error("Duplicate label id: {0}")]
    DuplicateId(String),
    #[error("Duplicate label name: {0}")]
    DuplicateName(String),
    #[error("AI engine is not enabled")]
    EngineDisabled,
    #[error("AI engine URL is invalid: {0}")]
    EngineUrl(String),
    #[error("could not configure AI engine HTTP client: {0}")]
    EngineClient(String),
    #[error("AI engine timed out")]
    EngineTimedOut,
    #[error("AI engine is unreachable: {0}")]
    EngineUnavailable(String),
    #[error("AI engine returned client error {status}: {message}")]
    EngineClientResponse { status: u16, message: String },
    #[error("AI engine returned server error {0}")]
    EngineServerResponse(u16),
    #[error("AI engine response exceeds the maximum size")]
    EngineResponseTooLarge,
    #[error("AI engine returned invalid classification JSON: {0}")]
    EngineJson(#[source] serde_json::Error),
    #[error("AI engine returned an invalid classification response")]
    EngineUnexpectedResponse,
    #[error("the configured PDFium runtime is unavailable: {0}")]
    PdfiumUnavailable(String),
    #[error(transparent)]
    Pdfium(#[from] PdfiumTextError),
    #[error("could not copy unclassified PDF: {0}")]
    Copy(#[source] std::io::Error),
    #[error("could not write PDF classification metadata: {0}")]
    Metadata(#[source] crate::pdf_metadata::MetadataError),
}

fn validate_labels(labels: &ClassificationLabels) -> Result<(), ClassificationError> {
    if labels.labels.len() > MAX_LABELS {
        return Err(ClassificationError::TooManyLabels);
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for label in &labels.labels {
        validate_text(&label.id, "Label id")?;
        validate_text(&label.name, "Label name")?;
        if let Some(icon) = label.icon.as_deref().filter(|icon| !icon.is_empty()) {
            if icon.encode_utf16().count() > MAX_TEXT_LENGTH {
                return Err(ClassificationError::TextTooLong {
                    field: "Label icon",
                });
            }
            if !icon
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(ClassificationError::InvalidIcon(icon.to_owned()));
            }
        }
        let id = label.id.trim().to_owned();
        if !ids.insert(id) {
            return Err(ClassificationError::DuplicateId(label.id.clone()));
        }
        let name = label.name.trim().to_lowercase();
        if !names.insert(name) {
            return Err(ClassificationError::DuplicateName(label.name.clone()));
        }
    }
    Ok(())
}

fn parse_labels_request(body: &[u8]) -> Result<ClassificationLabels, ClassificationError> {
    if let Ok(labels) = serde_json::from_slice::<Vec<ClassificationLabel>>(body) {
        return Ok(ClassificationLabels { labels });
    }
    let request = serde_json::from_slice::<ClassificationLabelsRequest>(body)
        .map_err(|_| ClassificationError::LabelsRequired)?;
    Ok(ClassificationLabels {
        labels: request.labels.ok_or(ClassificationError::LabelsRequired)?,
    })
}

fn validate_text(value: &str, field: &'static str) -> Result<(), ClassificationError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ClassificationError::BlankText { field });
    }
    if value.encode_utf16().count() > MAX_TEXT_LENGTH {
        return Err(ClassificationError::TextTooLong { field });
    }
    Ok(())
}

pub(crate) struct ClassificationService {
    page_extractor: Arc<dyn ClassificationPageExtractor>,
}

trait ClassificationPageExtractor: Send + Sync {
    fn extract(
        &self,
        input_path: &Path,
        filename: &str,
        max_text_characters_per_page: usize,
        pages_per_edge: usize,
    ) -> Result<PdfiumAiPageWindowAttempt, PdfiumTextError>;
}

struct PdfiumClassificationPageExtractor;

impl ClassificationPageExtractor for PdfiumClassificationPageExtractor {
    fn extract(
        &self,
        input_path: &Path,
        filename: &str,
        max_text_characters_per_page: usize,
        pages_per_edge: usize,
    ) -> Result<PdfiumAiPageWindowAttempt, PdfiumTextError> {
        try_extract_ai_page_window_content(
            input_path,
            filename,
            max_text_characters_per_page,
            pages_per_edge,
        )
    }
}

impl ClassificationService {
    pub(crate) fn new() -> Self {
        Self {
            page_extractor: Arc::new(PdfiumClassificationPageExtractor),
        }
    }

    #[cfg(test)]
    fn with_page_extractor(page_extractor: Arc<dyn ClassificationPageExtractor>) -> Self {
        Self { page_extractor }
    }

    fn classify_pdf(
        &self,
        labels: Option<ClassificationLabels>,
        input_path: &Path,
        filename: &str,
        settings: &AiCommentEngineSettings,
        output_path: &Path,
    ) -> Result<(), ClassificationError> {
        // No client-supplied vocabulary means there is nothing to classify
        // against; the PDF passes through unchanged, mirroring the previous
        // behavior for a team without stored labels.
        let Some(labels) = labels else {
            fs::copy(input_path, output_path).map_err(ClassificationError::Copy)?;
            return Ok(());
        };
        if labels.labels.is_empty() {
            fs::copy(input_path, output_path).map_err(ClassificationError::Copy)?;
            return Ok(());
        }
        if !settings.enabled() {
            return Err(ClassificationError::EngineDisabled);
        }
        let pages = match self.page_extractor.extract(
            input_path,
            filename,
            MAX_TEXT_CHARACTERS_PER_PAGE,
            WINDOW_PAGES_PER_EDGE,
        )? {
            PdfiumAiPageWindowAttempt::Extracted(pages) => pages,
            PdfiumAiPageWindowAttempt::Unavailable {
                explicitly_configured,
                details,
            } => {
                let details = if explicitly_configured {
                    format!("configured runtime could not be loaded: {details}")
                } else {
                    details
                };
                return Err(ClassificationError::PdfiumUnavailable(details));
            }
        };
        let pages = pages
            .into_iter()
            .filter_map(|page| {
                (!page.content.text.trim().is_empty()).then_some(EnginePageText {
                    page_number: page.page_number,
                    text: page.content.text,
                })
            })
            .collect();
        let allowed = labels
            .labels
            .into_iter()
            .map(|label| EngineLabel {
                id: label.id,
                name: label.name,
            })
            .collect();
        let request = EngineClassifyRequest {
            file_name: filename.to_owned(),
            pages,
            labels: allowed,
        };
        let metadata = request_classification(settings, &request)?;
        let metadata = serde_json::to_string(&metadata).map_err(ClassificationError::EngineJson)?;
        set_classification_metadata_to_file(input_path, filename, &metadata, output_path)
            .map_err(ClassificationError::Metadata)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineClassifyRequest {
    file_name: String,
    pages: Vec<EnginePageText>,
    labels: Vec<EngineLabel>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnginePageText {
    page_number: usize,
    text: String,
}

#[derive(Debug, Serialize)]
struct EngineLabel {
    id: String,
    name: String,
}

fn request_classification(
    settings: &AiCommentEngineSettings,
    payload: &EngineClassifyRequest,
) -> Result<Value, ClassificationError> {
    let base_url = settings.base_url().trim().trim_end_matches('/');
    let endpoint = reqwest::Url::parse(&format!("{base_url}{CLASSIFY_ENGINE_PATH}"))
        .map_err(|error| ClassificationError::EngineUrl(error.to_string()))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(ClassificationError::EngineUrl(
            "URL scheme must be http or https".to_owned(),
        ));
    }
    let client = Client::builder()
        .connect_timeout(settings.timeout())
        .timeout(settings.timeout())
        .build()
        .map_err(|error| ClassificationError::EngineClient(error.to_string()))?;
    let request_body = serde_json::to_vec(payload).map_err(ClassificationError::EngineJson)?;
    let mut request = client
        .post(endpoint)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .body(request_body);
    if let Some(secret) = settings.shared_secret() {
        request = request.header("X-Engine-Auth", secret);
    }
    let mut response = request
        .send()
        .map_err(|error| map_engine_transport_error(&error))?;
    let status = response.status();
    let mut body = Vec::new();
    response
        .by_ref()
        .take(MAX_ENGINE_RESPONSE_BYTES_U64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| ClassificationError::EngineUnavailable(error.to_string()))?;
    if body.len() > MAX_ENGINE_RESPONSE_BYTES {
        return Err(ClassificationError::EngineResponseTooLarge);
    }
    let body_text = String::from_utf8_lossy(&body);
    if status.is_server_error() {
        return Err(ClassificationError::EngineServerResponse(status.as_u16()));
    }
    if status.is_client_error() {
        return Err(ClassificationError::EngineClientResponse {
            status: status.as_u16(),
            message: body_text.chars().take(500).collect(),
        });
    }
    let mut value =
        serde_json::from_slice::<Value>(&body).map_err(ClassificationError::EngineJson)?;
    let object = value
        .as_object_mut()
        .ok_or(ClassificationError::EngineUnexpectedResponse)?;
    object.remove("outcome");
    if !object.get("labels").is_some_and(Value::is_array) {
        return Err(ClassificationError::EngineUnexpectedResponse);
    }
    Ok(value)
}

fn map_engine_transport_error(error: &reqwest::Error) -> ClassificationError {
    if error.is_timeout() {
        ClassificationError::EngineTimedOut
    } else {
        ClassificationError::EngineUnavailable(error.to_string())
    }
}

pub(crate) fn routes(service: Arc<ClassificationService>) -> Router {
    Router::new()
        .route(CLASSIFY_AND_LABEL_PATH, post(classify_and_label))
        .layer(Extension(service))
}

async fn classify_and_label(
    Extension(service): Extension<Arc<ClassificationService>>,
    Extension(settings): Extension<Arc<AiCommentEngineSettings>>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let temp_dir = TempDir::new()
        .map_err(|error| ApiError::internal_at(CLASSIFY_AND_LABEL_PATH, error.to_string()))?;
    let mut input = None;
    let mut labels = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request_at(CLASSIFY_AND_LABEL_PATH, error.body_text()))?
    {
        match field.name().unwrap_or_default() {
            "fileInput" => {
                if !field
                    .content_type()
                    .is_some_and(|value| value.eq_ignore_ascii_case("application/pdf"))
                {
                    return Err(ApiError::bad_request_at(
                        CLASSIFY_AND_LABEL_PATH,
                        "Only application/pdf uploads are supported",
                    ));
                }
                let filename = safe_filename(field.file_name());
                let path = temp_dir.path().join("input.pdf");
                write_field_to_file_bounded(
                    &mut field,
                    &path,
                    CLASSIFY_AND_LABEL_PATH,
                    AI_TOOL_MAX_INPUT_BYTES,
                )
                .await?;
                input = Some((filename, path));
            }
            "labels" => {
                let body = field.bytes().await.map_err(|error| {
                    ApiError::bad_request_at(CLASSIFY_AND_LABEL_PATH, error.body_text())
                })?;
                let parsed = parse_labels_request(&body)
                    .and_then(|labels| validate_labels(&labels).map(|()| labels))
                    .map_err(|error| classification_api_error(&error, CLASSIFY_AND_LABEL_PATH))?;
                labels = Some(parsed);
            }
            _ => drain_field(&mut field, CLASSIFY_AND_LABEL_PATH).await?,
        }
    }
    let (filename, input_path) = input.ok_or_else(|| {
        ApiError::bad_request_at(CLASSIFY_AND_LABEL_PATH, "fileInput is required")
    })?;
    let output_path = temp_dir.path().join("classified.pdf");
    let blocking_output_path = output_path.clone();
    let blocking_filename = filename.clone();
    task::spawn_blocking(move || {
        service.classify_pdf(
            labels,
            &input_path,
            &blocking_filename,
            &settings,
            &blocking_output_path,
        )
    })
    .await
    .map_err(|error| {
        ApiError::internal_at(
            CLASSIFY_AND_LABEL_PATH,
            format!("classification task failed: {error}"),
        )
    })?
    .map_err(|error| classification_api_error(&error, CLASSIFY_AND_LABEL_PATH))?;
    file_response(
        output_path,
        temp_dir,
        &filename,
        CLASSIFY_AND_LABEL_PATH,
        "application/pdf",
    )
    .await
}

fn classification_api_error(error: &ClassificationError, path: &'static str) -> ApiError {
    match error {
        ClassificationError::LabelsRequired
        | ClassificationError::TooManyLabels
        | ClassificationError::BlankText { .. }
        | ClassificationError::TextTooLong { .. }
        | ClassificationError::InvalidIcon(_)
        | ClassificationError::DuplicateId(_)
        | ClassificationError::DuplicateName(_)
        | ClassificationError::Pdfium(_) => ApiError::bad_request_at(path, error.to_string()),
        ClassificationError::EngineDisabled
        | ClassificationError::EngineUrl(_)
        | ClassificationError::EngineClient(_)
        | ClassificationError::EngineUnavailable(_) => {
            ApiError::service_unavailable_at(path, error.to_string())
        }
        ClassificationError::EngineTimedOut => {
            ApiError::gateway_timeout_at(path, error.to_string())
        }
        ClassificationError::EngineClientResponse { status, .. } => ApiError {
            status: StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
            message: error.to_string(),
            path,
        },
        ClassificationError::PdfiumUnavailable(_) => {
            ApiError::unsupported_at(path, error.to_string())
        }
        ClassificationError::EngineServerResponse(_)
        | ClassificationError::EngineResponseTooLarge
        | ClassificationError::EngineJson(_)
        | ClassificationError::EngineUnexpectedResponse => ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
            path,
        },
        ClassificationError::Copy(_) | ClassificationError::Metadata(_) => {
            ApiError::internal_at(path, error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::TcpListener,
        sync::Arc,
        thread,
        time::Duration,
    };

    use super::{
        ClassificationLabel, ClassificationLabels, ClassificationPageExtractor,
        ClassificationService, parse_labels_request, validate_labels,
    };
    use crate::{
        pdf_ai_comments::AiCommentEngineSettings,
        pdfium_backend::{
            PdfiumAiPageContent, PdfiumAiPageWindowAttempt, PdfiumAiPageWindowContent,
            PdfiumTextError,
        },
    };
    use lopdf::{Document, Object, dictionary};
    use serde_json::Value;
    use tempfile::tempdir;

    fn labels() -> ClassificationLabels {
        ClassificationLabels {
            labels: vec![
                ClassificationLabel {
                    id: "invoice".to_owned(),
                    name: "Invoice".to_owned(),
                    icon: Some("receipt-long".to_owned()),
                },
                ClassificationLabel {
                    id: "contract".to_owned(),
                    name: "Contract".to_owned(),
                    icon: None,
                },
            ],
        }
    }

    #[test]
    fn validates_label_invariants() {
        assert!(validate_labels(&labels()).is_ok());
        assert!(validate_labels(&ClassificationLabels { labels: Vec::new() }).is_ok());

        let mut duplicate = labels();
        duplicate.labels[1].id = "invoice".to_owned();
        assert!(validate_labels(&duplicate).is_err());
        let mut duplicate_name = labels();
        duplicate_name.labels[1].name = "INVOICE".to_owned();
        assert!(validate_labels(&duplicate_name).is_err());
        let mut invalid_icon = labels();
        invalid_icon.labels[0].icon = Some("Receipt Long".to_owned());
        assert!(validate_labels(&invalid_icon).is_err());
        let mut blank = labels();
        blank.labels[0].name = " ".to_owned();
        assert!(validate_labels(&blank).is_err());
        let mut utf16_too_long = labels();
        utf16_too_long.labels[0].name = "😀".repeat(65);
        assert!(validate_labels(&utf16_too_long).is_err());
        let mut utf16_boundary = labels();
        utf16_boundary.labels[0].name = "😀".repeat(64);
        assert!(validate_labels(&utf16_boundary).is_ok());
    }

    #[test]
    fn rejects_missing_or_null_label_arrays_but_accepts_an_empty_array() {
        assert!(parse_labels_request(br"{}").is_err());
        assert!(parse_labels_request(br#"{"labels":null}"#).is_err());
        assert!(matches!(
            parse_labels_request(br#"{"labels":[]}"#),
            Ok(ClassificationLabels { labels }) if labels.is_empty()
        ));
        // The client may also send the label array directly.
        assert!(matches!(
            parse_labels_request(br"[]"),
            Ok(ClassificationLabels { labels }) if labels.is_empty()
        ));
        assert!(matches!(
            parse_labels_request(br#"[{"id":"invoice","name":"Invoice"}]"#),
            Ok(ClassificationLabels { labels }) if labels.len() == 1
        ));
    }

    struct StubPageExtractor;

    impl ClassificationPageExtractor for StubPageExtractor {
        fn extract(
            &self,
            _input_path: &std::path::Path,
            _filename: &str,
            max_text_characters_per_page: usize,
            pages_per_edge: usize,
        ) -> Result<PdfiumAiPageWindowAttempt, PdfiumTextError> {
            assert_eq!(max_text_characters_per_page, 4_000);
            assert_eq!(pages_per_edge, 2);
            Ok(PdfiumAiPageWindowAttempt::Extracted(
                [1, 2, 4, 5]
                    .into_iter()
                    .map(|page_number| PdfiumAiPageWindowContent {
                        page_number,
                        content: PdfiumAiPageContent {
                            text: format!("page {page_number}"),
                            has_images: false,
                        },
                    })
                    .collect(),
            ))
        }
    }

    #[test]
    fn classification_stub_receives_only_edge_text_and_preserves_pdf_metadata()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let engine_url = format!("http://{}", listener.local_addr()?);
        let engine = thread::spawn(move || stub_engine_request(&listener));

        let directory = tempdir()?;
        let service = ClassificationService::with_page_extractor(Arc::new(StubPageExtractor));
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        write_metadata_pdf(&input)?;
        service.classify_pdf(
            Some(labels()),
            &input,
            "source.pdf",
            &AiCommentEngineSettings::new(true, engine_url, Duration::from_secs(5), None),
            &output,
        )?;

        let request = engine.join().map_err(|_| "engine stub panicked")??;
        assert!(!request.headers.contains("x-user-id"));
        assert_eq!(
            request.body["pages"],
            serde_json::json!([
                {"pageNumber": 1, "text": "page 1"},
                {"pageNumber": 2, "text": "page 2"},
                {"pageNumber": 4, "text": "page 4"},
                {"pageNumber": 5, "text": "page 5"}
            ])
        );
        assert_eq!(
            request.body["labels"],
            serde_json::json!([
                {"id": "invoice", "name": "Invoice"},
                {"id": "contract", "name": "Contract"}
            ])
        );
        assert!(request.body["labels"][0].get("icon").is_none());

        let output = Document::load(&output)?;
        let info = output.get_dictionary(output.trailer.get(b"Info")?.as_reference()?)?;
        assert_eq!(
            lopdf::decode_text_string(info.get(b"Title")?)?,
            "Keep title"
        );
        assert_eq!(lopdf::decode_text_string(info.get(b"KeepMe")?)?, "custom");
        assert_eq!(
            lopdf::decode_text_string(info.get(b"RustlingPDFClassification")?)?,
            r#"{"labels":["invoice"]}"#
        );
        Ok(())
    }

    struct StubRequest {
        headers: String,
        body: Value,
    }

    fn stub_engine_request(
        listener: &TcpListener,
    ) -> Result<StubRequest, Box<dyn std::error::Error + Send + Sync>> {
        let (mut stream, _) = listener.accept()?;
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Err("request ended before headers".into());
            }
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8(bytes[..header_end].to_vec())?;
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .and_then(|value| value.parse::<usize>().ok())
            })
            .ok_or("missing content length")?;
        while bytes.len().saturating_sub(header_end) < content_length {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Err("request ended before body".into());
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        let body = serde_json::from_slice(&bytes[header_end..header_end + content_length])?;
        let response_body = br#"{"outcome":"classification","labels":["invoice"]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        )?;
        stream.write_all(response_body)?;
        Ok(StubRequest {
            headers: headers.to_ascii_lowercase(),
            body,
        })
    }

    fn write_metadata_pdf(
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut document = Document::with_version("1.7");
        let page_tree_id = document.new_object_id();
        let page_object_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => page_tree_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
        });
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_object_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => page_tree_id,
        });
        let info_id = document.add_object(dictionary! {
            "Title" => Object::string_literal("Keep title"),
            "KeepMe" => Object::string_literal("custom"),
        });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("Info", info_id);
        document.save(path)?;
        Ok(())
    }
}
