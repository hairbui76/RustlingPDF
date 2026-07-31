//! Multipart boundary for stateless AI document understanding.
//!
//! PDF bytes remain in this process. The separately configured AI engine sees
//! only bounded, locally extracted page text and typed operation settings.

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Extension, Multipart},
    http::{StatusCode, header},
    routing::post,
};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::task;

use crate::{
    AI_TOOL_MAX_INPUT_BYTES, ApiError,
    ai_proxy::{engine_endpoint, proxy_client, transport_error},
    drain_field,
    pdf_ai_comments::AiCommentEngineSettings,
    pdfium_backend::{
        PdfiumWorkflowPageText, PdfiumWorkflowTextAttempt, try_extract_workflow_page_text,
    },
    read_form_value_bounded,
    runtime_config::RuntimeConfig,
    safe_filename, write_field_to_file_bounded,
};

pub(crate) const DOCUMENT_SUMMARY_PATH: &str = "/api/v1/ai/tools/document-summary";
pub(crate) const DOCUMENT_EXTRACTION_PATH: &str = "/api/v1/ai/tools/document-extraction";
pub(crate) const DOCUMENT_TRANSLATION_PATH: &str = "/api/v1/ai/tools/document-translation";

const ENGINE_SUMMARY_PATH: &str = "/api/v1/ai/document/summary";
const ENGINE_EXTRACTION_PATH: &str = "/api/v1/ai/document/extraction";
const ENGINE_TRANSLATION_PATH: &str = "/api/v1/ai/document/translation";
const ENGINE_AUTH_HEADER: &str = "X-Engine-Auth";
const MAX_ENGINE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TEXT_FIELD_BYTES: usize = 16 * 1024;
const MAX_FIELDS_BYTES: usize = 64 * 1024;
const MAX_EXTRACTION_FIELDS: usize = 50;
const MAX_FIELD_KEY_UNITS: usize = 64;
const MAX_FIELD_DESCRIPTION_UNITS: usize = 500;
const MAX_LANGUAGE_UNITS: usize = 100;
const PROVIDER_DISCLOSURE: &str =
    "Extracted document text was sent to the AI provider configured by this server.";

pub(crate) fn routes() -> Router {
    Router::new()
        .route(DOCUMENT_SUMMARY_PATH, post(document_summary))
        .route(DOCUMENT_EXTRACTION_PATH, post(document_extraction))
        .route(DOCUMENT_TRANSLATION_PATH, post(document_translation))
}

#[derive(Clone, Copy)]
enum Operation {
    Summary,
    Extraction,
    Translation,
}

impl Operation {
    const fn public_path(self) -> &'static str {
        match self {
            Self::Summary => DOCUMENT_SUMMARY_PATH,
            Self::Extraction => DOCUMENT_EXTRACTION_PATH,
            Self::Translation => DOCUMENT_TRANSLATION_PATH,
        }
    }

    const fn engine_path(self) -> &'static str {
        match self {
            Self::Summary => ENGINE_SUMMARY_PATH,
            Self::Extraction => ENGINE_EXTRACTION_PATH,
            Self::Translation => ENGINE_TRANSLATION_PATH,
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Extraction => "extraction",
            Self::Translation => "translation",
        }
    }
}

#[derive(Debug)]
struct UploadedDocument {
    filename: String,
    path: PathBuf,
}

#[derive(Debug)]
struct UnderstandingUpload {
    document: UploadedDocument,
    settings: OperationSettings,
    _temp_dir: TempDir,
}

#[derive(Debug)]
enum OperationSettings {
    Summary {
        detail: SummaryDetail,
        instructions: Option<String>,
    },
    Extraction {
        fields: Vec<ExtractionField>,
    },
    Translation {
        target_language: String,
        source_language: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum SummaryDetail {
    Brief,
    #[default]
    Standard,
    Detailed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ExtractionValueType {
    String,
    Number,
    Integer,
    Boolean,
    Date,
    List,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExtractionField {
    key: String,
    description: String,
    value_type: ExtractionValueType,
    #[serde(default)]
    required: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnginePageText {
    page_number: usize,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryEngineRequest<'request> {
    file_name: &'request str,
    pages: &'request [EnginePageText],
    detail: SummaryDetail,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'request str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractionEngineRequest<'request> {
    file_name: &'request str,
    pages: &'request [EnginePageText],
    fields: &'request [ExtractionField],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationEngineRequest<'request> {
    file_name: &'request str,
    pages: &'request [EnginePageText],
    target_language: &'request str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_language: Option<&'request str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceReport<'source> {
    file_name: &'source str,
    pages_processed: usize,
    characters_processed: usize,
    max_pages: usize,
    max_characters: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicResponse<'response> {
    schema_version: u8,
    operation: &'static str,
    provider_disclosure: &'static str,
    source: SourceReport<'response>,
    result: Value,
}

async fn document_summary(
    Extension(settings): Extension<Arc<AiCommentEngineSettings>>,
    Extension(runtime_config): Extension<Arc<RuntimeConfig>>,
    multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    handle_request(Operation::Summary, settings, runtime_config, multipart).await
}

async fn document_extraction(
    Extension(settings): Extension<Arc<AiCommentEngineSettings>>,
    Extension(runtime_config): Extension<Arc<RuntimeConfig>>,
    multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    handle_request(Operation::Extraction, settings, runtime_config, multipart).await
}

async fn document_translation(
    Extension(settings): Extension<Arc<AiCommentEngineSettings>>,
    Extension(runtime_config): Extension<Arc<RuntimeConfig>>,
    multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    handle_request(Operation::Translation, settings, runtime_config, multipart).await
}

async fn handle_request(
    operation: Operation,
    settings: Arc<AiCommentEngineSettings>,
    runtime_config: Arc<RuntimeConfig>,
    multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let upload = read_upload(operation, multipart).await?;
    if !settings.enabled() {
        return Err(ApiError::service_unavailable_at(
            operation.public_path(),
            "AI engine is not enabled",
        ));
    }
    let (max_pages, max_characters) = configured_limits(&runtime_config, operation.public_path())?;
    let pages = extract_pages(
        upload.document.path.clone(),
        upload.document.filename.clone(),
        max_pages,
        max_characters,
        operation.public_path(),
    )
    .await?;
    let engine_body = engine_request_body(&upload, &pages)?;
    let result = invoke_engine(&settings, operation, &engine_body).await?;
    let characters_processed = pages
        .iter()
        .map(|page| page.text.encode_utf16().count())
        .sum();
    let response = PublicResponse {
        schema_version: 1,
        operation: operation.wire_name(),
        provider_disclosure: PROVIDER_DISCLOSURE,
        source: SourceReport {
            file_name: &upload.document.filename,
            pages_processed: pages.len(),
            characters_processed,
            max_pages,
            max_characters,
        },
        result,
    };
    serde_json::to_value(response)
        .map(Json)
        .map_err(|error| ApiError::internal_at(operation.public_path(), error.to_string()))
}

fn configured_limits(
    runtime_config: &RuntimeConfig,
    public_path: &'static str,
) -> Result<(usize, usize), ApiError> {
    let limits = runtime_config.ai_engine_push_settings().limits;
    let max_pages = usize::try_from(limits.max_pages)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            ApiError::service_unavailable_at(
                public_path,
                "AI maxPages configuration must be positive",
            )
        })?;
    let max_characters = usize::try_from(limits.max_characters)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            ApiError::service_unavailable_at(
                public_path,
                "AI maxCharacters configuration must be positive",
            )
        })?;
    Ok((max_pages, max_characters))
}

async fn extract_pages(
    path: PathBuf,
    filename: String,
    max_pages: usize,
    max_characters: usize,
    public_path: &'static str,
) -> Result<Vec<EnginePageText>, ApiError> {
    let attempt = task::spawn_blocking(move || {
        try_extract_workflow_page_text(&path, &filename, &[], max_pages, max_characters, false)
    })
    .await
    .map_err(|error| {
        ApiError::internal_at(
            public_path,
            format!("document text extraction task failed: {error}"),
        )
    })?
    .map_err(|error| ApiError::bad_request_at(public_path, error.to_string()))?;
    let pages = match attempt {
        PdfiumWorkflowTextAttempt::Extracted(pages) => pages,
        PdfiumWorkflowTextAttempt::Unavailable {
            explicitly_configured,
            details,
        } => {
            let details = if explicitly_configured {
                format!("configured runtime could not be loaded: {details}")
            } else {
                details
            };
            return Err(ApiError::service_unavailable_at(
                public_path,
                format!("PDFium is unavailable for AI document text extraction: {details}"),
            ));
        }
    };
    if pages.is_empty() {
        return Err(ApiError::unprocessable_at(
            public_path,
            "PDF has no extractable text; run OCR first for an image-only scan",
        ));
    }
    Ok(pages.into_iter().map(engine_page).collect())
}

fn engine_page(page: PdfiumWorkflowPageText) -> EnginePageText {
    EnginePageText {
        page_number: page.page_number,
        text: page.text,
    }
}

fn engine_request_body(
    upload: &UnderstandingUpload,
    pages: &[EnginePageText],
) -> Result<Value, ApiError> {
    let public_path = match upload.settings {
        OperationSettings::Summary { .. } => DOCUMENT_SUMMARY_PATH,
        OperationSettings::Extraction { .. } => DOCUMENT_EXTRACTION_PATH,
        OperationSettings::Translation { .. } => DOCUMENT_TRANSLATION_PATH,
    };
    let body = match &upload.settings {
        OperationSettings::Summary {
            detail,
            instructions,
        } => json!(SummaryEngineRequest {
            file_name: &upload.document.filename,
            pages,
            detail: *detail,
            instructions: instructions.as_deref(),
        }),
        OperationSettings::Extraction { fields } => json!(ExtractionEngineRequest {
            file_name: &upload.document.filename,
            pages,
            fields,
        }),
        OperationSettings::Translation {
            target_language,
            source_language,
        } => json!(TranslationEngineRequest {
            file_name: &upload.document.filename,
            pages,
            target_language,
            source_language: source_language.as_deref(),
        }),
    };
    serde_json::to_value(body)
        .map_err(|error| ApiError::internal_at(public_path, error.to_string()))
}

async fn invoke_engine(
    settings: &AiCommentEngineSettings,
    operation: Operation,
    body: &Value,
) -> Result<Value, ApiError> {
    let public_path = operation.public_path();
    let endpoint = engine_endpoint(settings, operation.engine_path(), public_path)
        .map_err(|error| ApiError::bad_request_at(public_path, error.detail()))?;
    let client = proxy_client(settings.timeout(), public_path)
        .map_err(|error| ApiError::service_unavailable_at(public_path, error.detail()))?;
    let mut request = client
        .post(endpoint)
        .header(header::ACCEPT, "application/json")
        .json(body);
    if let Some(secret) = settings.shared_secret() {
        request = request.header(ENGINE_AUTH_HEADER, secret);
    }
    let response = request.send().await.map_err(|error| {
        let error = transport_error(&error, public_path);
        if error.detail() == "AI engine timed out" {
            ApiError::gateway_timeout_at(public_path, error.detail())
        } else {
            ApiError::service_unavailable_at(public_path, error.detail())
        }
    })?;
    let status = response.status();
    let body = read_bounded_engine_body(response, public_path).await?;
    if status.is_client_error() {
        let detail = engine_error_detail(&body);
        return Err(if status == StatusCode::UNPROCESSABLE_ENTITY {
            ApiError::unprocessable_at(public_path, detail)
        } else {
            ApiError::bad_request_at(public_path, detail)
        });
    }
    if !status.is_success() {
        return Err(ApiError::bad_gateway_at(
            public_path,
            format!("AI engine returned error: {}", status.as_u16()),
        ));
    }
    serde_json::from_slice(&body).map_err(|error| {
        ApiError::bad_gateway_at(
            public_path,
            format!("AI engine returned invalid JSON: {error}"),
        )
    })
}

async fn read_bounded_engine_body(
    response: reqwest::Response,
    public_path: &'static str,
) -> Result<Vec<u8>, ApiError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ENGINE_RESPONSE_BYTES as u64)
    {
        return Err(ApiError::bad_gateway_at(
            public_path,
            "AI engine response exceeds the 2 MiB limit",
        ));
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            ApiError::service_unavailable_at(
                public_path,
                format!("AI engine response could not be read: {error}"),
            )
        })?;
        if body.len().saturating_add(chunk.len()) > MAX_ENGINE_RESPONSE_BYTES {
            return Err(ApiError::bad_gateway_at(
                public_path,
                "AI engine response exceeds the 2 MiB limit",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn engine_error_detail(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|body| {
            body.get("detail")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|detail| !detail.trim().is_empty())
        .unwrap_or_else(|| {
            let text = String::from_utf8_lossy(body).trim().to_owned();
            if text.is_empty() {
                "AI engine rejected the request".to_owned()
            } else {
                text
            }
        })
}

#[derive(Default)]
struct MultipartDraft {
    document: Option<UploadedDocument>,
    detail: Option<String>,
    instructions: Option<String>,
    fields: Option<String>,
    target_language: Option<String>,
    source_language: Option<String>,
}

async fn read_upload(
    operation: Operation,
    mut multipart: Multipart,
) -> Result<UnderstandingUpload, ApiError> {
    let public_path = operation.public_path();
    let temp_dir = TempDir::new().map_err(|error| {
        ApiError::internal_at(
            public_path,
            format!("could not create AI document workspace: {error}"),
        )
    })?;
    let mut draft = MultipartDraft::default();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request_at(public_path, error.body_text()))?
    {
        match field.name().unwrap_or_default() {
            "fileInput" => {
                if draft.document.is_some() {
                    return Err(ApiError::bad_request_at(
                        public_path,
                        "fileInput must be supplied only once",
                    ));
                }
                draft.document =
                    Some(store_document_field(&mut field, &temp_dir, public_path).await?);
            }
            "detail" => {
                set_once(
                    &mut draft.detail,
                    read_form_value_bounded(&mut field, public_path, 32).await?,
                    "detail",
                    public_path,
                )?;
            }
            "instructions" => {
                set_once(
                    &mut draft.instructions,
                    read_form_value_bounded(&mut field, public_path, MAX_TEXT_FIELD_BYTES).await?,
                    "instructions",
                    public_path,
                )?;
            }
            "fields" => {
                set_once(
                    &mut draft.fields,
                    read_form_value_bounded(&mut field, public_path, MAX_FIELDS_BYTES).await?,
                    "fields",
                    public_path,
                )?;
            }
            "targetLanguage" => {
                set_once(
                    &mut draft.target_language,
                    read_form_value_bounded(&mut field, public_path, 512).await?,
                    "targetLanguage",
                    public_path,
                )?;
            }
            "sourceLanguage" => {
                set_once(
                    &mut draft.source_language,
                    read_form_value_bounded(&mut field, public_path, 512).await?,
                    "sourceLanguage",
                    public_path,
                )?;
            }
            _ => drain_field(&mut field, public_path).await?,
        }
    }
    let document = draft
        .document
        .take()
        .ok_or_else(|| ApiError::bad_request_at(public_path, "fileInput is required"))?;
    let settings = parse_settings(operation, draft)?;
    Ok(UnderstandingUpload {
        document,
        settings,
        _temp_dir: temp_dir,
    })
}

async fn store_document_field(
    field: &mut axum::extract::multipart::Field<'_>,
    temp_dir: &TempDir,
    public_path: &'static str,
) -> Result<UploadedDocument, ApiError> {
    if field
        .content_type()
        .is_some_and(|content_type| !content_type.eq_ignore_ascii_case("application/pdf"))
    {
        return Err(ApiError::bad_request_at(
            public_path,
            "Only application/pdf uploads are supported",
        ));
    }
    let filename = safe_filename(field.file_name());
    let path = temp_dir.path().join("input.pdf");
    write_field_to_file_bounded(field, &path, public_path, AI_TOOL_MAX_INPUT_BYTES).await?;
    let length = tokio::fs::metadata(&path)
        .await
        .map_err(|error| ApiError::internal_at(public_path, error.to_string()))?
        .len();
    if length == 0 {
        return Err(ApiError::bad_request_at(public_path, "File is empty"));
    }
    Ok(UploadedDocument { filename, path })
}

fn set_once(
    slot: &mut Option<String>,
    value: String,
    name: &str,
    public_path: &'static str,
) -> Result<(), ApiError> {
    if slot.replace(value).is_some() {
        return Err(ApiError::bad_request_at(
            public_path,
            format!("{name} must be supplied only once"),
        ));
    }
    Ok(())
}

fn parse_settings(
    operation: Operation,
    draft: MultipartDraft,
) -> Result<OperationSettings, ApiError> {
    let public_path = operation.public_path();
    match operation {
        Operation::Summary => {
            let detail = draft.detail.map_or(Ok(SummaryDetail::Standard), |detail| {
                serde_json::from_value::<SummaryDetail>(Value::String(
                    detail.trim().to_ascii_lowercase(),
                ))
                .map_err(|_| {
                    ApiError::bad_request_at(
                        public_path,
                        "detail must be brief, standard, or detailed",
                    )
                })
            })?;
            let instructions = draft
                .instructions
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            validate_utf16(instructions.as_deref(), "instructions", 4_000, public_path)?;
            Ok(OperationSettings::Summary {
                detail,
                instructions,
            })
        }
        Operation::Extraction => {
            let fields_json = draft
                .fields
                .ok_or_else(|| ApiError::bad_request_at(public_path, "fields is required"))?;
            let fields =
                serde_json::from_str::<Vec<ExtractionField>>(&fields_json).map_err(|error| {
                    ApiError::bad_request_at(
                        public_path,
                        format!("fields is invalid JSON: {error}"),
                    )
                })?;
            validate_fields(&fields, public_path)?;
            Ok(OperationSettings::Extraction { fields })
        }
        Operation::Translation => {
            let target_language = draft
                .target_language
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ApiError::bad_request_at(public_path, "targetLanguage is required")
                })?;
            let source_language = draft
                .source_language
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            validate_utf16(
                Some(&target_language),
                "targetLanguage",
                MAX_LANGUAGE_UNITS,
                public_path,
            )?;
            validate_utf16(
                source_language.as_deref(),
                "sourceLanguage",
                MAX_LANGUAGE_UNITS,
                public_path,
            )?;
            Ok(OperationSettings::Translation {
                target_language,
                source_language,
            })
        }
    }
}

fn validate_fields(fields: &[ExtractionField], public_path: &'static str) -> Result<(), ApiError> {
    if fields.is_empty() || fields.len() > MAX_EXTRACTION_FIELDS {
        return Err(ApiError::bad_request_at(
            public_path,
            format!("fields must contain 1 to {MAX_EXTRACTION_FIELDS} entries"),
        ));
    }
    let mut keys = HashSet::with_capacity(fields.len());
    for field in fields {
        if field.key.is_empty()
            || field.key.encode_utf16().count() > MAX_FIELD_KEY_UNITS
            || !field
                .key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ApiError::bad_request_at(
                public_path,
                format!("field key {:?} is invalid", field.key),
            ));
        }
        if !keys.insert(field.key.as_str()) {
            return Err(ApiError::bad_request_at(
                public_path,
                format!("field key {:?} is duplicated", field.key),
            ));
        }
        if field.description.trim().is_empty()
            || field.description.encode_utf16().count() > MAX_FIELD_DESCRIPTION_UNITS
        {
            return Err(ApiError::bad_request_at(
                public_path,
                format!("description for field {:?} is invalid", field.key),
            ));
        }
    }
    Ok(())
}

fn validate_utf16(
    value: Option<&str>,
    name: &str,
    max_units: usize,
    public_path: &'static str,
) -> Result<(), ApiError> {
    if value.is_some_and(|value| value.encode_utf16().count() > max_units) {
        return Err(ApiError::bad_request_at(
            public_path,
            format!("{name} exceeds maximum length of {max_units} characters"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        DOCUMENT_EXTRACTION_PATH, EnginePageText, ExtractionField, ExtractionValueType,
        MultipartDraft, Operation, OperationSettings, UnderstandingUpload, UploadedDocument,
        engine_request_body, parse_settings,
    };

    #[test]
    fn extraction_settings_reject_duplicate_keys() -> Result<(), String> {
        let fields = json!([
            {"key":"invoice","description":"First","valueType":"string"},
            {"key":"invoice","description":"Second","valueType":"string"}
        ])
        .to_string();
        let result = parse_settings(
            Operation::Extraction,
            MultipartDraft {
                fields: Some(fields),
                ..MultipartDraft::default()
            },
        );
        let Err(error) = result else {
            return Err("duplicate key unexpectedly succeeded".to_owned());
        };
        assert_eq!(error.path, DOCUMENT_EXTRACTION_PATH);
        assert!(error.message.contains("duplicated"));
        Ok(())
    }

    #[test]
    fn engine_payload_contains_text_not_a_pdf_path() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let upload = UnderstandingUpload {
            document: UploadedDocument {
                filename: "source.pdf".to_owned(),
                path: temp_dir.path().join("source.pdf"),
            },
            settings: OperationSettings::Extraction {
                fields: vec![ExtractionField {
                    key: "invoice".to_owned(),
                    description: "Invoice number".to_owned(),
                    value_type: ExtractionValueType::String,
                    required: true,
                }],
            },
            _temp_dir: temp_dir,
        };
        let body_result = engine_request_body(
            &upload,
            &[EnginePageText {
                page_number: 2,
                text: "Invoice 42".to_owned(),
            }],
        );
        let Ok(body) = body_result else {
            return Err("engine request body failed".into());
        };
        assert_eq!(body["fileName"], "source.pdf");
        assert_eq!(body["pages"][0]["pageNumber"], 2);
        assert_eq!(body["pages"][0]["text"], "Invoice 42");
        assert!(body.get("path").is_none());
        assert!(
            !body
                .to_string()
                .contains(upload.document.path.to_string_lossy().as_ref())
        );
        Ok(())
    }
}
