//! Public configuration for the Rust HTTP service.
//!
//! `configs/settings.yml` is loaded before `configs/custom_settings.yml` below
//! `RUSTLING_BASE_PATH`; the latter overrides the former. This module owns the
//! public runtime configuration surface.
//!
//! The service collects and transmits nothing about its users. Settings keys
//! from earlier releases that configured the removed opt-in analytics
//! (`system.enableAnalytics`, `system.enablePosthog`, `system.enableScarf`) and
//! their `SYSTEM_ENABLE*` environment overrides are simply never read: this
//! reader resolves each key by explicit path, so an unrecognised key in an
//! existing `settings.yml` is ignored, never refused. Nothing prunes them from
//! the file either — `update_settings_file_values` only upserts the
//! `AutomaticallyGenerated` identity section.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use crate::job_queue::JobQueueConfig;
use crate::runtime_dependencies::discover_dependencies;

// Functional groups and runtime dependency groups. Values are whitespace-separated
// endpoint keys to keep the table readable.
//
// Keys must match what `endpoint_key_for_uri` derives from the registered route, not the tool's
// display name. The overlay route derives `overlay-pdfs`, while the tool registry
// uses `overlay-pdf`, so both keys are retained: one gates the route and the other
// lets the UI discover that the tool is disabled.
//
// The table serves a second consumer, which is why a display-name key is kept *beside* the derived
// key rather than replaced by it. `endpoint_availability` answers
// `GET /api/v1/config/endpoints-availability` with no query from these keys, and the SPA's tool
// registry looks its own tool keys up in that map, treating a key it cannot find as enabled
// (`core/hooks/useEndpointConfig.ts`). So the tool-registry spellings — `compare`, `view-pdf`,
// `multi-tool`, `text-editor-pdf`, `overlay-pdf`, `form-fill`, the `dev-*-docs` entries — must
// stay: dropping one would leave the tool advertised as available in a UI whose group an
// administrator has disabled. Adding the derived key next to it is what actually stops the route
// answering. `every_spa_tool_registry_key_is_present_in_the_group_table` reads the registry and
// holds the two spellings together.
//
// An endpoint that appears in no group at all cannot be disabled by any group setting and the
// administrator gets no error, so a route belongs here unless one of two things is true:
//
//   1. It is infrastructure the UI needs in order to work at all — `/api/v1/info/*`,
//      `/api/v1/config/*` (including the availability map itself), `/api/v1/ui-data/*`,
//      `/api/v1/settings/*`, job and file plumbing, and the mobile-scanner session routes.
//      Gating those would let an administrator brick the UI rather than disable a tool.
//   2. It already has its own dedicated administrator switch that is strictly stronger than group
//      gating: `send-email` exists only when `mail.enabled` yields an SMTP service
//      (`processing_routes_with_mail` does not register the route otherwise), and every
//      `/api/v1/ai/*` proxy route answers `503` unless `AIENGINE_ENABLED` is set
//      (`ai_proxy::proxy_request`). Adding them to a functional group would surprise an
//      administrator who disabled that group to switch off PDF tools.
//
// `every_processing_route_is_reachable_from_some_functional_group` pins that split.
//
// Two sharp edges worth knowing before editing:
//
//   - Keys are not one-to-one with routes. `/api/v1/ai/health` and `/api/v1/info/health` both
//     derive `health`, and every `/api/v1/ai/tools/*` route derives `tools`, so gating one of a
//     colliding pair would silently gate the other.
//   - `is_group_enabled` reports a functional group enabled only when *every* member key is
//     enabled, so adding a key here also means a single `endpoints.toRemove` entry can flip
//     `group-enabled?group=<name>` to false for the whole group.
const ENDPOINT_GROUPS: &[(&str, &str)] = &[
    (
        "PageOps",
        concat!(
            "remove-pages merge-pdfs split-pages rearrange-pages rotate-pdf multi-page-layout ",
            "booklet-imposition scale-pages crop pdf-to-single-page auto-split-pdf ",
            "split-by-size-or-count overlay-pdfs overlay-pdf split-pdf-by-sections ",
            "split-pdf-by-chapters add-page-numbers extract-pages split-for-poster-print",
        ),
    ),
    (
        "Convert",
        // Ghostscript-gated conversions (pdf-to-pdfa, pdf-to-vector, vector-to-pdf)
        // are removed with the Ghostscript engine — the routes no longer exist.
        concat!(
            "pdf-to-img img-to-pdf file-to-pdf pdf-to-word pdf-to-presentation ",
            "pdf-to-text pdf-to-html pdf-to-xml html-to-pdf url-to-pdf markdown-to-pdf ",
            "pdf-to-csv pdf-to-xlsx pdf-to-markdown eml-to-pdf pdf-to-epub ebook-to-pdf ",
            "svg-to-pdf pdf-to-video cbz-to-pdf cbr-to-pdf ",
            "pdf-to-cbz pdf-to-cbr pdf-to-json json-to-pdf pdf-to-rtf",
        ),
    ),
    (
        "Security",
        concat!(
            "add-password remove-password change-permissions add-watermark cert-sign ",
            "remove-cert-sign sanitize-pdf timestamp-pdf auto-redact validate-signature ",
            "add-stamp unlock-pdf-forms redact redact-execute verify-pdf sign",
        ),
    ),
    (
        "Other",
        concat!(
            "ocr-pdf extract-images update-metadata flatten remove-blanks remove-annotations ",
            "get-info-on-pdf add-attachments replace-invert-pdf edit-table-of-contents ",
            "text-editor-pdf add-image compare view-pdf multi-tool fields modify-fields ",
            "batch-fill create-fields delete-fields fill check remediate ",
            // Text editor: `text-editor-pdf` above is the SPA tool-registry spelling; these two
            // are the keys the registered routes actually derive. Deliberately here rather than
            // in `Convert`: the routes live under `/api/v1/convert/` but they are the text
            // editor's own machinery, not a conversion a user picks, and upstream groups the
            // tool under `Other` too.
            "pdf-to-text-editor text-editor-to-pdf edit-text remove-image-pdf ",
            // Attachment family, alongside its `add-attachments` sibling.
            "extract-attachments list-attachments delete-attachment rename-attachment ",
            // Form family, alongside its `fields`/`batch-fill`/`create-fields`/
            // `modify-fields`/`delete-fields`/`fill` siblings. `form-fill` is the
            // SPA tool-registry spelling of `fill`.
            "extract-csv extract-xlsx fields-with-coordinates form-fill ",
            // Bookmark extraction beside `edit-table-of-contents`, and comment insertion beside
            // `remove-annotations`.
            "extract-bookmarks add-comments ",
            // `/api/v1/filter/*`: upstream leaves all six out of its group map but names them in
            // testing/allEndpointsRemovedSettings.yml's `toRemove` set, so it treats them as
            // ordinary disableable endpoints; they also parse an uploaded PDF, the same argument
            // that places the analysis routes below.
            "filter-contains-image filter-contains-text filter-file-size filter-page-count ",
            "filter-page-rotation filter-page-size ",
            // `/api/v1/analysis/*` introspection: the same parse-a-user-PDF-and-report surface as
            // `get-info-on-pdf` above, which this group already covers.
            "annotation-info basic-info document-properties font-info form-fields page-count ",
            "page-dimensions security-info",
        ),
    ),
    (
        "Advance",
        "compress-pdf decompress-pdf extract-image-scans repair auto-rename scanner-effect overlay-pdfs overlay-pdf adjust-contrast",
    ),
    ("Automation", "handleData automate pipeline"),
    ("DeveloperTools", "show-javascript"),
    (
        "DeveloperDocs",
        "dev-api-docs dev-folder-scanning-docs dev-sso-guide-docs dev-airgapped-docs",
    ),
    (
        // `file-to-pdf` deliberately does *not* appear here. Office → PDF now
        // has a built-in pure-Rust engine that covers DOCX/XLSX/PPTX with no
        // external tool, so reporting the endpoint unavailable when LibreOffice
        // is missing would be a lie — that is exactly the "Convert to PDF: this
        // tool is not available from your server" report this change answers.
        // The key still exists under `Convert`, so it stays a known, listable,
        // `endpoints.toRemove`-able endpoint; only the dependency gate is gone.
        // The PDF → office direction has no built-in engine and stays gated.
        "LibreOffice",
        "pdf-to-word pdf-to-presentation pdf-to-rtf pdf-to-xml",
    ),
    ("tesseract", "ocr-pdf"),
    ("OCRmyPDF", "ocr-pdf"),
    ("rar", "pdf-to-cbr"),
    (
        "Weasyprint",
        "html-to-pdf url-to-pdf markdown-to-pdf eml-to-pdf",
    ),
    ("Calibre", "pdf-to-epub ebook-to-pdf"),
    ("FFmpeg", "pdf-to-video"),
    ("unrar", "cbr-to-pdf"),
];

const FUNCTIONAL_GROUPS: &[&str] = &[
    "PageOps",
    "Convert",
    "Security",
    "Other",
    "Advance",
    "Automation",
    "DeveloperTools",
    "DeveloperDocs",
];

const ENDPOINT_ALTERNATIVES: &[(&str, &[&str])] = &[("ocr-pdf", &["tesseract", "OCRmyPDF"])];

const MAX_LOGIN_DISCLAIMER_BYTES: usize = 256 * 1024;
const MAX_LOGIN_DISCLAIMER_BYTES_U64: u64 = 256 * 1024;

#[derive(Debug, Serialize)]
pub struct EndpointAvailability {
    enabled: bool,
    reason: Option<&'static str>,
}

impl EndpointAvailability {
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn reason(&self) -> Option<&'static str> {
        self.reason
    }
}

#[derive(Debug, Serialize)]
pub struct LoginDisclaimer {
    enabled: bool,
    #[serde(rename = "showInAnonymousMode")]
    show_in_anonymous_mode: bool,
    content: String,
    format: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OcrProcessSettings {
    pub(crate) ocrmypdf_session_limit: usize,
    pub(crate) ocrmypdf_timeout: Duration,
    pub(crate) tesseract_session_limit: usize,
    pub(crate) tesseract_timeout: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RepairProcessSettings {
    pub(crate) qpdf_session_limit: usize,
    pub(crate) qpdf_timeout: Duration,
}

#[derive(Clone)]
pub struct RuntimeConfig {
    settings: Value,
    settings_path: PathBuf,
    load_error: Option<String>,
    custom_files_dir: PathBuf,
    dependency_disabled_groups: BTreeSet<String>,
    dependency_commands: BTreeMap<String, PathBuf>,
    dependencies_checked: bool,
}

/// SMTP relay settings for the optional email-with-attachment route.
///
/// Secrets are zeroized with the resolved configuration. Certificate and
/// hostname verification are always retained by the Rust transport.
#[derive(Clone)]
pub(crate) struct SmtpMailConfig {
    pub(crate) enabled: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<Zeroizing<String>>,
    pub(crate) from: String,
    pub(crate) transport_security: SmtpTransportSecurity,
    pub(crate) ssl_trust: Option<String>,
    pub(crate) hostname_verification: SmtpHostnameVerification,
}

#[derive(Clone, Copy)]
pub(crate) enum SmtpTransportSecurity {
    Plaintext,
    OpportunisticStartTls,
    RequiredStartTls,
    ImplicitTls,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SmtpHostnameVerification {
    Required,
    Disabled,
}

impl RuntimeConfig {
    #[must_use]
    pub fn from_environment() -> Self {
        let base_path = crate::environment::var_os("RUSTLING_BASE_PATH")
            .filter(|value| !value.is_empty())
            .map_or_else(|| PathBuf::from("."), PathBuf::from);
        let mut config = Self::from_files(
            base_path.join("configs/settings.yml"),
            base_path.join("configs/custom_settings.yml"),
        );
        config.dependencies_checked = false;
        config
    }

    /// Probes optional command-line tools and applies dependency-disabled groups.
    ///
    /// The executable calls this once during startup. File-backed constructors stay
    /// probe-free so embedded routers and tests cannot unexpectedly start processes.
    #[must_use]
    pub fn with_dependency_discovery(mut self) -> Self {
        let discovery = discover_dependencies();
        self.dependency_disabled_groups = discovery.disabled_groups;
        self.dependency_commands = discovery.commands;
        self.dependencies_checked = true;
        self
    }

    #[must_use]
    pub fn from_files(
        settings_path: impl Into<PathBuf>,
        custom_settings_path: impl Into<PathBuf>,
    ) -> Self {
        let settings_path = settings_path.into();
        let custom_settings_path = custom_settings_path.into();
        Self::from_paths(settings_path, &custom_settings_path)
    }

    #[must_use]
    pub fn app_config(&self, host: Option<&str>, forwarded_proto: Option<&str>) -> Value {
        let mut config = Map::new();
        self.insert_connection_config(&mut config, host, forwarded_proto);
        self.insert_ui_config(&mut config);
        self.insert_system_config(&mut config);
        self.insert_feature_config(&mut config);
        self.insert_timestamp_and_legal_config(&mut config);
        if let Some(error) = &self.load_error {
            insert(&mut config, "error", error.clone());
        }
        Value::Object(config)
    }

    /// Returns the strict UI language allowlist, or an empty list when every
    /// bundled language is permitted.
    #[must_use]
    pub fn ui_languages(&self) -> Vec<String> {
        self.strings(&["ui", "languages"], "UI_LANGUAGES")
    }

    #[must_use]
    pub fn timestamp_settings(&self) -> (String, Vec<String>) {
        (
            self.string(
                &["security", "timestamp", "defaultTsaUrl"],
                "SECURITY_TIMESTAMP_DEFAULTTSAURL",
                "http://timestamp.digicert.com",
            ),
            self.strings(
                &["security", "timestamp", "customTsaUrls"],
                "SECURITY_TIMESTAMP_CUSTOMTSAURLS",
            ),
        )
    }

    /// Resolves the `mail.*` SMTP relay settings without opening a
    /// network connection. The route is mounted only when `mail.enabled` is
    /// true.
    #[must_use]
    pub(crate) fn smtp_mail_config(&self) -> SmtpMailConfig {
        let optional_string = |path: &[&str], environment: &str| {
            let value = self.string(path, environment, "");
            (!value.trim().is_empty()).then_some(value)
        };
        let configured_port = self.u64(&["mail", "port"], "MAIL_PORT", 587);
        let ssl_enable = self.boolean(&["mail", "sslEnable"], "MAIL_SSLENABLE", false);
        let start_tls_enable =
            self.boolean(&["mail", "startTlsEnable"], "MAIL_STARTTLSENABLE", true);
        let start_tls_required = self.boolean(
            &["mail", "startTlsRequired"],
            "MAIL_STARTTLSREQUIRED",
            false,
        );
        let transport_security = if ssl_enable {
            SmtpTransportSecurity::ImplicitTls
        } else if start_tls_enable && start_tls_required {
            SmtpTransportSecurity::RequiredStartTls
        } else if start_tls_enable {
            SmtpTransportSecurity::OpportunisticStartTls
        } else {
            SmtpTransportSecurity::Plaintext
        };
        let hostname_verification = if self.optional_boolean(
            &["mail", "sslCheckServerIdentity"],
            "MAIL_SSLCHECKSERVERIDENTITY",
        ) == Some(false)
        {
            SmtpHostnameVerification::Disabled
        } else {
            SmtpHostnameVerification::Required
        };
        SmtpMailConfig {
            enabled: self.boolean(&["mail", "enabled"], "MAIL_ENABLED", false),
            host: self.string(&["mail", "host"], "MAIL_HOST", ""),
            port: u16::try_from(configured_port).unwrap_or(587),
            username: optional_string(&["mail", "username"], "MAIL_USERNAME"),
            password: optional_string(&["mail", "password"], "MAIL_PASSWORD").map(Zeroizing::new),
            from: self.string(&["mail", "from"], "MAIL_FROM", ""),
            transport_security,
            ssl_trust: optional_string(&["mail", "sslTrust"], "MAIL_SSLTRUST"),
            hostname_verification,
        }
    }

    /// Resolves the backend-to-engine connection settings shared by AI tools.
    ///
    /// Environment variables take precedence over the corresponding
    /// `aiEngine.*` YAML values.
    #[must_use]
    pub fn ai_engine_settings(&self) -> (bool, String, u64) {
        let enabled = env_bool("AIENGINE_ENABLED")
            .or_else(|| env_bool("RUSTLING_AI_ENGINE_ENABLED"))
            .or_else(|| value_at(&self.settings, &["aiEngine", "enabled"]).and_then(yaml_bool))
            .unwrap_or(false);
        let url = crate::environment::var("AIENGINE_URL")
            .ok()
            .or_else(|| crate::environment::var("RUSTLING_AI_ENGINE_URL").ok())
            .or_else(|| {
                value_at(&self.settings, &["aiEngine", "url"])
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "http://localhost:5001".to_owned());
        let timeout_seconds = crate::environment::var("AIENGINE_TIMEOUTSECONDS")
            .ok()
            .or_else(|| crate::environment::var("AIENGINE_TIMEOUT_SECONDS").ok())
            .and_then(|value| value.parse().ok())
            .or_else(|| {
                value_at(&self.settings, &["aiEngine", "timeoutSeconds"]).and_then(Value::as_u64)
            })
            .unwrap_or(120)
            .max(1);
        (enabled, url, timeout_seconds)
    }

    #[must_use]
    pub(crate) fn ai_engine_long_running_timeout(&self) -> Duration {
        let seconds = crate::environment::var("AIENGINE_LONGRUNNINGTIMEOUTSECONDS")
            .ok()
            .or_else(|| crate::environment::var("AIENGINE_LONG_RUNNING_TIMEOUT_SECONDS").ok())
            .and_then(|value| value.parse().ok())
            .or_else(|| {
                value_at(&self.settings, &["aiEngine", "longRunningTimeoutSeconds"])
                    .and_then(Value::as_u64)
            })
            .unwrap_or(600)
            .max(1);
        Duration::from_secs(seconds)
    }

    /// Resolves the engine-relevant `aiEngine.*` configuration pushed to the
    /// engine's `POST /api/v1/config` on startup.
    #[must_use]
    pub fn ai_engine_push_settings(&self) -> AiEnginePushSettings {
        let (enabled, _, _) = self.ai_engine_settings();
        AiEnginePushSettings {
            enabled,
            push_config_to_engine: self.boolean(
                &["aiEngine", "pushConfigToEngine"],
                "AIENGINE_PUSHCONFIGTOENGINE",
                true,
            ),
            models: AiEngineModelsPush {
                provider: self.string(
                    &["aiEngine", "models", "provider"],
                    "AIENGINE_MODELS_PROVIDER",
                    DEFAULT_AI_MODEL_PROVIDER,
                ),
                smart_model: self.string(
                    &["aiEngine", "models", "smartModel"],
                    "AIENGINE_MODELS_SMARTMODEL",
                    DEFAULT_AI_SMART_MODEL,
                ),
                fast_model: self.string(
                    &["aiEngine", "models", "fastModel"],
                    "AIENGINE_MODELS_FASTMODEL",
                    DEFAULT_AI_FAST_MODEL,
                ),
                smart_max_tokens: self.signed_integer(
                    &["aiEngine", "models", "smartMaxTokens"],
                    "AIENGINE_MODELS_SMARTMAXTOKENS",
                    8_192,
                ),
                fast_max_tokens: self.signed_integer(
                    &["aiEngine", "models", "fastMaxTokens"],
                    "AIENGINE_MODELS_FASTMAXTOKENS",
                    2_048,
                ),
                api_key: self.string(
                    &["aiEngine", "models", "apiKey"],
                    "AIENGINE_MODELS_APIKEY",
                    "",
                ),
                base_url: self.string(
                    &["aiEngine", "models", "baseUrl"],
                    "AIENGINE_MODELS_BASEURL",
                    "",
                ),
            },
            limits: AiEngineLimitsPush {
                max_pages: self.signed_integer(
                    &["aiEngine", "limits", "maxPages"],
                    "AIENGINE_LIMITS_MAXPAGES",
                    200,
                ),
                max_characters: self.signed_integer(
                    &["aiEngine", "limits", "maxCharacters"],
                    "AIENGINE_LIMITS_MAXCHARACTERS",
                    200_000,
                ),
                model_max_concurrency: self.signed_integer(
                    &["aiEngine", "limits", "modelMaxConcurrency"],
                    "AIENGINE_LIMITS_MODELMAXCONCURRENCY",
                    32,
                ),
            },
        }
    }

    #[must_use]
    pub(crate) fn ai_workflow_stream_timeout(&self) -> Duration {
        let milliseconds = crate::environment::var("RUSTLING_AI_STREAMTIMEOUTMS")
            .ok()
            .or_else(|| crate::environment::var("RUSTLING_AI_STREAM_TIMEOUT_MS").ok())
            .and_then(|value| value.parse().ok())
            .or_else(|| {
                product_value_at(&self.settings, &["ai", "streamTimeoutMs"]).and_then(Value::as_u64)
            })
            .unwrap_or(1_800_000)
            .max(1);
        Duration::from_millis(milliseconds)
    }

    /// Resolves bounded asynchronous job admission, including an explicit
    /// weighted execution budget for the scheduler.
    pub(crate) fn job_queue_config(&self) -> JobQueueConfig {
        let queue_capacity = self
            .product_u64(
                &["job", "queue", "baseCapacity"],
                "RUSTLING_JOB_QUEUE_BASE_CAPACITY",
                10,
            )
            .clamp(1, 10_000) as usize;
        let resource_budget = self
            .product_u64(
                &["job", "queue", "resourceBudget"],
                "RUSTLING_JOB_QUEUE_RESOURCE_BUDGET",
                10,
            )
            .clamp(1, 1_000) as u32;
        let max_wait_millis = self
            .product_u64(
                &["job", "queue", "maxWaitTimeMs"],
                "RUSTLING_JOB_QUEUE_MAX_WAIT_TIME_MS",
                600_000,
            )
            .clamp(1_000, 86_400_000);
        JobQueueConfig {
            queue_capacity,
            resource_budget,
            max_wait: Duration::from_millis(max_wait_millis),
        }
    }

    pub(crate) fn job_result_ttl(&self) -> Duration {
        let minutes = self
            .product_u64(
                &["jobResultExpiryMinutes"],
                "RUSTLING_JOB_RESULT_EXPIRY_MINUTES",
                30,
            )
            .clamp(1, 7 * 24 * 60);
        Duration::from_secs(minutes * 60)
    }

    #[must_use]
    pub fn login_disclaimer(&self, requested_locale: Option<&str>) -> LoginDisclaimer {
        let show_in_anonymous_mode = self.login_agreement_show_in_anonymous_mode();
        if !self.login_agreement_is_enabled() {
            return LoginDisclaimer {
                enabled: false,
                show_in_anonymous_mode,
                content: String::new(),
                format: "markdown",
            };
        }

        let content = self.resolve_login_disclaimer(requested_locale);
        let enabled = !content.trim().is_empty();
        LoginDisclaimer {
            enabled,
            show_in_anonymous_mode,
            content: if enabled { content } else { String::new() },
            format: "markdown",
        }
    }

    #[must_use]
    pub fn metrics_enabled(&self) -> bool {
        self.boolean(&["metrics", "enabled"], "METRICS_ENABLED", true)
    }

    #[must_use]
    pub fn mobile_scanner_enabled(&self) -> bool {
        self.boolean(
            &["system", "enableMobileScanner"],
            "SYSTEM_ENABLEMOBILESCANNER",
            true,
        )
    }

    /// Returns whether the instance permits search-engine indexing.
    #[must_use]
    pub fn google_visibility(&self) -> bool {
        self.boolean(
            &["system", "googlevisibility"],
            "SYSTEM_GOOGLEVISIBILITY",
            false,
        )
    }

    /// Resolves the directory containing the preloaded pipeline templates for
    /// the unchanged client. This is separate from watched-folder pipelines.
    #[must_use]
    pub fn pipeline_web_ui_configs_dir(&self) -> PathBuf {
        let base_path = installation_path(&self.settings_path);
        let pipeline_dir = resolve_configured_path(
            &base_path.join("pipeline"),
            &self.string(
                &["system", "customPaths", "pipeline", "pipelineDir"],
                "SYSTEM_CUSTOMPATHS_PIPELINE_PIPELINEDIR",
                "",
            ),
        );
        resolve_configured_path(
            &pipeline_dir.join("defaultWebUIConfigs"),
            &self.string(
                &["system", "customPaths", "pipeline", "webUIConfigsDir"],
                "SYSTEM_CUSTOMPATHS_PIPELINE_WEBUICONFIGSDIR",
                "",
            ),
        )
    }

    /// Returns the Tesseract language-data directory using this precedence:
    /// explicit settings, `TESSDATA_PREFIX`, then the packaged Linux default.
    #[must_use]
    pub fn tessdata_dir(&self) -> PathBuf {
        let configured = self.string(&["system", "tessdataDir"], "SYSTEM_TESSDATADIR", "");
        if !configured.trim().is_empty() {
            return PathBuf::from(configured);
        }
        crate::environment::var_os("TESSDATA_PREFIX")
            .filter(|value| !value.is_empty())
            .map_or_else(
                || PathBuf::from("/usr/share/tesseract-ocr/5/tessdata"),
                PathBuf::from,
            )
    }

    /// Resolves the explicitly selected Paddle OCR artifact set.
    ///
    /// `auto` (the default) preserves the OCRmyPDF/Tesseract path. Selecting
    /// `paddle` requires every path; incomplete configuration is retained as an
    /// error so a typo cannot silently fall back to a different engine.
    pub(crate) fn paddle_ocr_config(
        &self,
    ) -> Result<Option<crate::paddle_ocr::PaddleOcrConfig>, String> {
        let engine = self.string(&["ocr", "engine"], "RUSTLING_PROCESSING_OCR_ENGINE", "auto");
        match engine.trim() {
            "" | "auto" => return Ok(None),
            "paddle" => {}
            value => {
                return Err(format!(
                    "ocr.engine must be 'auto' or 'paddle', got '{value}'"
                ));
            }
        }

        let path = |field: &str, environment: &str| {
            let value = self.string(&["ocr", "paddle", field], environment, "");
            (!value.trim().is_empty())
                .then(|| PathBuf::from(value))
                .ok_or_else(|| format!("ocr.paddle.{field} is required when ocr.engine is paddle"))
        };
        Ok(Some(crate::paddle_ocr::PaddleOcrConfig {
            onnx_runtime_path: path(
                "onnxRuntimePath",
                "RUSTLING_PROCESSING_PADDLE_OCR_ONNX_RUNTIME_PATH",
            )?,
            detector_model_path: path(
                "detectorModelPath",
                "RUSTLING_PROCESSING_PADDLE_OCR_DETECTOR_MODEL_PATH",
            )?,
            recognizer_model_path: path(
                "recognizerModelPath",
                "RUSTLING_PROCESSING_PADDLE_OCR_RECOGNIZER_MODEL_PATH",
            )?,
            dictionary_path: path(
                "dictionaryPath",
                "RUSTLING_PROCESSING_PADDLE_OCR_DICTIONARY_PATH",
            )?,
            text_layer_font_path: path(
                "textLayerFontPath",
                "RUSTLING_PROCESSING_PADDLE_OCR_TEXT_LAYER_FONT_PATH",
            )?,
        }))
    }

    /// Reports whether the operator selected an OCR engine other than the
    /// default.
    ///
    /// Any non-`auto` value counts, including an invalid one. An invalid
    /// selection must reach `paddle_ocr_config` and fail as a configuration
    /// error; letting it fall back to the Tesseract-availability answer would
    /// report `501 Not Implemented` on a host without `OCRmyPDF` and hide the
    /// typo.
    fn ocr_engine_is_explicitly_selected(&self) -> bool {
        !matches!(
            self.string(&["ocr", "engine"], "RUSTLING_PROCESSING_OCR_ENGINE", "auto")
                .trim(),
            "" | "auto"
        )
    }

    /// Returns the maximum page-rendering DPI used by the OCR fallback.
    #[must_use]
    pub fn max_render_dpi(&self) -> i32 {
        let configured = self.signed_integer(&["system", "maxDPI"], "SYSTEM_MAXDPI", 500);
        i32::try_from(configured.clamp(1, i64::from(i32::MAX))).unwrap_or(500)
    }

    /// Returns the process limits used by the OCR controller.
    #[must_use]
    pub(crate) fn ocr_process_settings(&self) -> OcrProcessSettings {
        let positive = |path: &[&str], environment: &str, default: u64| {
            let signed_default = i64::try_from(default).unwrap_or(i64::MAX);
            u64::try_from(self.signed_integer(path, environment, signed_default))
                .ok()
                .filter(|value| *value > 0)
                .unwrap_or(default)
        };
        let ocrmypdf_session_limit = positive(
            &["processExecutor", "sessionLimit", "ocrMyPdfSessionLimit"],
            "PROCESS_EXECUTOR_SESSION_LIMIT_OCR_MY_PDF_SESSION_LIMIT",
            2,
        );
        let tesseract_session_limit = positive(
            &["processExecutor", "sessionLimit", "tesseractSessionLimit"],
            "PROCESS_EXECUTOR_SESSION_LIMIT_TESSERACT_SESSION_LIMIT",
            1,
        );
        let ocrmypdf_timeout_minutes = positive(
            &[
                "processExecutor",
                "timeoutMinutes",
                "ocrMyPdfTimeoutMinutes",
            ],
            "PROCESS_EXECUTOR_TIMEOUT_MINUTES_OCR_MY_PDF_TIMEOUT_MINUTES",
            30,
        );
        let tesseract_timeout_minutes = positive(
            &[
                "processExecutor",
                "timeoutMinutes",
                "tesseractTimeoutMinutes",
            ],
            "PROCESS_EXECUTOR_TIMEOUT_MINUTES_TESSERACT_TIMEOUT_MINUTES",
            30,
        );
        OcrProcessSettings {
            ocrmypdf_session_limit: usize::try_from(ocrmypdf_session_limit).unwrap_or(2),
            ocrmypdf_timeout: Duration::from_secs(ocrmypdf_timeout_minutes.saturating_mul(60)),
            tesseract_session_limit: usize::try_from(tesseract_session_limit).unwrap_or(1),
            tesseract_timeout: Duration::from_secs(tesseract_timeout_minutes.saturating_mul(60)),
        }
    }

    /// Returns the process limits used by the repair controller.
    #[must_use]
    pub(crate) fn repair_process_settings(&self) -> RepairProcessSettings {
        let positive = |path: &[&str], environment: &str, default: u64| {
            let signed_default = i64::try_from(default).unwrap_or(i64::MAX);
            u64::try_from(self.signed_integer(path, environment, signed_default))
                .ok()
                .filter(|value| *value > 0)
                .unwrap_or(default)
        };
        let qpdf_session_limit = positive(
            &["processExecutor", "sessionLimit", "qpdfSessionLimit"],
            "PROCESS_EXECUTOR_SESSION_LIMIT_QPDF_SESSION_LIMIT",
            2,
        );
        let qpdf_timeout_minutes = positive(
            &["processExecutor", "timeoutMinutes", "qpdfTimeoutMinutes"],
            "PROCESS_EXECUTOR_TIMEOUT_MINUTES_QPDF_TIMEOUT_MINUTES",
            30,
        );
        RepairProcessSettings {
            qpdf_session_limit: usize::try_from(qpdf_session_limit).unwrap_or(2),
            qpdf_timeout: Duration::from_secs(qpdf_timeout_minutes.saturating_mul(60)),
        }
    }

    /// Returns the exact executable accepted by startup dependency discovery.
    #[must_use]
    pub(crate) fn dependency_command(&self, group: &str) -> Option<PathBuf> {
        self.is_group_enabled(group).then_some(())?;
        self.dependency_commands.get(group).cloned()
    }

    /// Returns the shared signature-image directory (operator-provisioned).
    #[must_use]
    pub fn shared_signatures_dir(&self) -> PathBuf {
        installation_path(&self.settings_path)
            .join("customFiles")
            .join("signatures")
            .join("ALL_USERS")
    }

    /// Returns the administrator-provided static-font directory.
    #[must_use]
    pub fn custom_static_fonts_dir(&self) -> PathBuf {
        installation_path(&self.settings_path)
            .join("customFiles")
            .join("static")
            .join("fonts")
    }

    /// Returns the administrator-provided static override directory
    /// (`customFiles/static/`).
    #[must_use]
    pub(crate) fn custom_static_dir(&self) -> PathBuf {
        self.custom_files_dir.join("static")
    }

    /// Returns the built SPA `dist/` directory to serve from the binary, when
    /// single-binary UI serving is enabled via `RUSTLING_FRONTEND_DIST` (env)
    /// or `system.frontendDist` (settings). Unset means SPA serving stays fully
    /// disabled for the Vite development-proxy workflow.
    #[must_use]
    pub fn frontend_dist_dir(&self) -> Option<PathBuf> {
        let configured = self.string(&["system", "frontendDist"], "RUSTLING_FRONTEND_DIST", "");
        let trimmed = configured.trim();
        (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
    }

    #[must_use]
    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    /// Ensures the persisted install identity. On first boot, a random UUID
    /// and machine key are generated and written
    /// into `AutomaticallyGenerated.*` in the settings file, and the current
    /// application version is persisted (its previous absence — empty or the
    /// `0.0.0` placeholder — marks a new server). Identity supplied through the
    /// environment (`AUTOMATICALLYGENERATED_*`) is honored without being
    /// written back. An unchanged boot writes nothing, keeping the settings
    /// file byte-stable and preserving template-merge idempotence.
    ///
    /// # Errors
    ///
    /// Returns a description when the settings file cannot be read, parsed, or
    /// written; the returned identity is then only valid for this process.
    pub fn initialize_generated_identity(&self) -> Result<GeneratedIdentity, String> {
        let existing_uuid = self.generated_setting("UUID", "AUTOMATICALLYGENERATED_UUID");
        let existing_key = self.generated_setting("key", "AUTOMATICALLYGENERATED_KEY");
        let existing_version =
            self.generated_setting("appVersion", "AUTOMATICALLYGENERATED_APPVERSION");
        let is_new_server =
            existing_version.trim().is_empty() || existing_version.trim() == "0.0.0";
        // Persist the repository application version, not the crate version.
        let app_version = crate::runtime_metrics::application_version().to_owned();

        let mut writes: Vec<(&str, serde_yaml::Value)> = Vec::new();
        let uuid = if is_valid_settings_uuid(&existing_uuid) {
            existing_uuid
        } else {
            let generated = random_uuid_v4();
            writes.push(("UUID", serde_yaml::Value::String(generated.clone())));
            generated
        };
        let key = if is_valid_settings_uuid(&existing_key) {
            existing_key
        } else {
            let generated = random_uuid_v4();
            writes.push(("key", serde_yaml::Value::String(generated.clone())));
            generated
        };
        if existing_version.trim() != app_version {
            writes.push(("appVersion", serde_yaml::Value::String(app_version.clone())));
        }
        if !writes.is_empty() {
            update_settings_file_values(&self.settings_path, "AutomaticallyGenerated", &writes)?;
        }
        Ok(GeneratedIdentity {
            uuid,
            key,
            app_version,
            is_new_server,
        })
    }

    /// Resolves the install identity without ever touching the settings file.
    ///
    /// The open, stateless server has no server-side identity to preserve: the
    /// UUID and key are taken from configuration/environment when supplied
    /// (`AUTOMATICALLYGENERATED_*` stays honored as config) and otherwise
    /// generated fresh for this process. Only the desktop sidecar (Tauri mode)
    /// persists the identity back into its own local `settings.yml`.
    #[must_use]
    pub fn ephemeral_generated_identity(&self) -> GeneratedIdentity {
        let existing_uuid = self.generated_setting("UUID", "AUTOMATICALLYGENERATED_UUID");
        let existing_key = self.generated_setting("key", "AUTOMATICALLYGENERATED_KEY");
        let existing_version =
            self.generated_setting("appVersion", "AUTOMATICALLYGENERATED_APPVERSION");
        let is_new_server =
            existing_version.trim().is_empty() || existing_version.trim() == "0.0.0";
        GeneratedIdentity {
            uuid: if is_valid_settings_uuid(&existing_uuid) {
                existing_uuid
            } else {
                random_uuid_v4()
            },
            key: if is_valid_settings_uuid(&existing_key) {
                existing_key
            } else {
                random_uuid_v4()
            },
            app_version: crate::runtime_metrics::application_version().to_owned(),
            is_new_server,
        }
    }

    fn generated_setting(&self, field: &str, environment: &str) -> String {
        crate::environment::var(environment)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                value_at(&self.settings, &["AutomaticallyGenerated", field])
                    .or_else(|| value_at(&self.settings, &["automaticallyGenerated", field]))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default()
    }

    fn insert_connection_config(
        &self,
        config: &mut Map<String, Value>,
        host: Option<&str>,
        forwarded_proto: Option<&str>,
    ) {
        insert(config, "dependenciesReady", self.dependencies_checked);
        insert(
            config,
            "baseUrl",
            self.string(&["system", "backendUrl"], "SYSTEM_BACKENDURL", ""),
        );
        insert(config, "contextPath", "");
        insert(
            config,
            "serverPort",
            Self::usize("RUSTLING_PROCESSING_PORT", 8081),
        );
        insert(
            config,
            "frontendUrl",
            self.frontend_url(host, forwarded_proto),
        );
    }

    fn insert_ui_config(&self, config: &mut Map<String, Value>) {
        insert(
            config,
            "appNameNavbar",
            self.string(&["ui", "appNameNavbar"], "UI_APPNAMENAVBAR", ""),
        );
        insert(config, "languages", self.ui_languages());
        insert(
            config,
            "logoStyle",
            self.string(&["ui", "logoStyle"], "UI_LOGOSTYLE", "classic"),
        );
    }

    fn insert_system_config(&self, config: &mut Map<String, Value>) {
        // The build's own version, so the UI can name the running build without
        // a second round trip. Same source as the persisted
        // `AutomaticallyGenerated.appVersion`: the workspace VERSION file.
        insert(
            config,
            "appVersion",
            crate::runtime_metrics::application_version().to_owned(),
        );
        insert(
            config,
            "defaultLocale",
            self.string(&["system", "defaultLocale"], "SYSTEM_DEFAULTLOCALE", ""),
        );
        insert(
            config,
            "enableAlphaFunctionality",
            self.boolean(
                &["system", "enableAlphaFunctionality"],
                "SYSTEM_ENABLEALPHAFUNCTIONALITY",
                false,
            ),
        );
        insert(
            config,
            "enableDesktopInstallSlide",
            self.boolean(
                &["system", "enableDesktopInstallSlide"],
                "SYSTEM_ENABLEDESKTOPINSTALLSLIDE",
                true,
            ),
        );
        self.insert_mobile_scanner_config(config);
    }

    fn insert_mobile_scanner_config(&self, config: &mut Map<String, Value>) {
        insert(
            config,
            "enableMobileScanner",
            self.boolean(
                &["system", "enableMobileScanner"],
                "SYSTEM_ENABLEMOBILESCANNER",
                true,
            ),
        );
        insert(
            config,
            "mobileScannerConvertToPdf",
            self.boolean(
                &["system", "mobileScannerSettings", "convertToPdf"],
                "SYSTEM_MOBILESCANNERSETTINGS_CONVERTTOPDF",
                true,
            ),
        );
        insert(
            config,
            "mobileScannerImageResolution",
            self.string(
                &["system", "mobileScannerSettings", "imageResolution"],
                "SYSTEM_MOBILESCANNERSETTINGS_IMAGERESOLUTION",
                "full",
            ),
        );
        insert(
            config,
            "mobileScannerPageFormat",
            self.string(
                &["system", "mobileScannerSettings", "pageFormat"],
                "SYSTEM_MOBILESCANNERSETTINGS_PAGEFORMAT",
                "A4",
            ),
        );
        insert(
            config,
            "mobileScannerStretchToFit",
            self.boolean(
                &["system", "mobileScannerSettings", "stretchToFit"],
                "SYSTEM_MOBILESCANNERSETTINGS_STRETCHTOFIT",
                false,
            ),
        );
    }

    fn insert_feature_config(&self, config: &mut Map<String, Value>) {
        insert(
            config,
            "defaultHideUnavailableTools",
            self.boolean(
                &["ui", "defaultHideUnavailableTools"],
                "UI_DEFAULTHIDEUNAVAILABLETOOLS",
                false,
            ),
        );
        insert(
            config,
            "defaultHideUnavailableConversions",
            self.boolean(
                &["ui", "defaultHideUnavailableConversions"],
                "UI_DEFAULTHIDEUNAVAILABLECONVERSIONS",
                false,
            ),
        );
        insert(
            config,
            "hideDisabledToolsGoogleDrive",
            self.boolean(
                &["ui", "hideDisabledTools", "googleDrive"],
                "UI_HIDEDISABLEDTOOLS_GOOGLEDRIVE",
                false,
            ),
        );
        insert(
            config,
            "hideDisabledToolsMobileQRScanner",
            self.boolean(
                &["ui", "hideDisabledTools", "mobileQRScanner"],
                "UI_HIDEDISABLEDTOOLS_MOBILEQRSCANNER",
                false,
            ),
        );
        insert(
            config,
            "aiEngineEnabled",
            self.boolean(&["aiEngine", "enabled"], "AIENGINE_ENABLED", false),
        );
        insert(config, "serverCertificateEnabled", false);
        insert(config, "hardwareSigningAvailable", false);
    }

    fn insert_timestamp_and_legal_config(&self, config: &mut Map<String, Value>) {
        insert(
            config,
            "timestampDefaultTsaUrl",
            self.string(
                &["security", "timestamp", "defaultTsaUrl"],
                "SECURITY_TIMESTAMP_DEFAULTTSAURL",
                "http://timestamp.digicert.com",
            ),
        );
        insert(
            config,
            "timestampCustomTsaUrls",
            self.strings(
                &["security", "timestamp", "customTsaUrls"],
                "SECURITY_TIMESTAMP_CUSTOMTSAURLS",
            ),
        );
        insert(config, "timestampTsaPresets", tsa_presets());
        insert(
            config,
            "termsAndConditions",
            self.string(
                &["legal", "termsAndConditions"],
                "LEGAL_TERMSANDCONDITIONS",
                "",
            ),
        );
        insert(
            config,
            "privacyPolicy",
            self.string(&["legal", "privacyPolicy"], "LEGAL_PRIVACYPOLICY", ""),
        );
        insert(
            config,
            "cookiePolicy",
            self.string(&["legal", "cookiePolicy"], "LEGAL_COOKIEPOLICY", ""),
        );
        insert(
            config,
            "impressum",
            self.string(&["legal", "impressum"], "LEGAL_IMPRESSUM", ""),
        );
        insert(
            config,
            "accessibilityStatement",
            self.string(
                &["legal", "accessibilityStatement"],
                "LEGAL_ACCESSIBILITYSTATEMENT",
                "",
            ),
        );
    }

    #[must_use]
    pub fn is_endpoint_enabled(&self, endpoint: &str) -> bool {
        let endpoint = normalize_endpoint(endpoint);
        self.is_endpoint_enabled_with_groups(&endpoint, &self.disabled_groups())
    }

    #[must_use]
    pub fn is_endpoint_enabled_for_uri(&self, uri: &str) -> bool {
        let endpoint = endpoint_key_for_uri(uri).unwrap_or_else(|| uri.to_owned());
        self.is_endpoint_enabled(&endpoint)
    }

    #[must_use]
    pub fn is_group_enabled(&self, group: &str) -> bool {
        let disabled_groups = self.disabled_groups();
        let group = group.trim();
        if group.is_empty() || is_group_disabled(group, &disabled_groups) {
            return false;
        }
        if is_tool_group(group) {
            return true;
        }
        if !FUNCTIONAL_GROUPS.contains(&group) {
            return false;
        }
        let Some((_, endpoints)) = ENDPOINT_GROUPS
            .iter()
            .find(|(configured_group, _)| *configured_group == group)
        else {
            return false;
        };
        endpoints
            .split_whitespace()
            .all(|endpoint| self.is_endpoint_enabled_directly(endpoint, &disabled_groups))
    }

    #[must_use]
    pub fn disabled_endpoint_statuses(&self) -> BTreeMap<String, bool> {
        let disabled_groups = self.disabled_groups();
        let mut statuses = self
            .disabled_endpoint_keys()
            .into_iter()
            .map(|endpoint| (endpoint, false))
            .collect::<BTreeMap<_, _>>();
        for (group, endpoints) in ENDPOINT_GROUPS {
            if !is_tool_group(group) && is_group_disabled(group, &disabled_groups) {
                statuses.extend(
                    endpoints
                        .split_whitespace()
                        .map(|endpoint| (endpoint.to_owned(), false)),
                );
            }
        }
        if !self.url_to_pdf_is_enabled("url-to-pdf") {
            statuses.insert("url-to-pdf".to_owned(), false);
        }
        statuses
    }

    #[must_use]
    pub fn endpoint_availability(
        &self,
        requested_endpoints: &[String],
    ) -> BTreeMap<String, EndpointAvailability> {
        let configured_disabled_groups = self.configured_disabled_groups();
        let disabled_groups = self.disabled_groups();
        let endpoints: BTreeSet<String> = if requested_endpoints.is_empty() {
            Self::known_endpoint_keys()
                .chain(self.disabled_endpoint_keys())
                .collect()
        } else {
            requested_endpoints
                .iter()
                .map(|endpoint| normalize_endpoint(endpoint))
                .collect()
        };
        endpoints
            .into_iter()
            .filter(|endpoint| !endpoint.is_empty())
            .map(|endpoint| {
                let enabled = self.is_endpoint_enabled_with_groups(&endpoint, &disabled_groups);
                let reason = if enabled {
                    None
                } else if self
                    .is_endpoint_enabled_with_groups(&endpoint, &configured_disabled_groups)
                {
                    Some("DEPENDENCY")
                } else {
                    Some("CONFIG")
                };
                (endpoint, EndpointAvailability { enabled, reason })
            })
            .collect()
    }

    /// Reports availability for one canonical API URI.
    ///
    /// This preserves the same endpoint-key normalization used by request
    /// middleware while allowing non-HTTP frontends such as the local CLI to
    /// distinguish configuration policy from a missing optional dependency.
    #[must_use]
    pub fn endpoint_availability_for_uri(&self, uri: &str) -> EndpointAvailability {
        let endpoint = endpoint_key_for_uri(uri).unwrap_or_else(|| uri.to_owned());
        let configured_disabled_groups = self.configured_disabled_groups();
        let disabled_groups = self.disabled_groups();
        let enabled = self.is_endpoint_enabled_with_groups(&endpoint, &disabled_groups);
        let reason = if enabled {
            None
        } else if self.is_endpoint_enabled_with_groups(&endpoint, &configured_disabled_groups) {
            Some("DEPENDENCY")
        } else {
            Some("CONFIG")
        };
        EndpointAvailability { enabled, reason }
    }

    fn disabled_groups(&self) -> Vec<String> {
        let mut groups = self.configured_disabled_groups();
        groups.extend(self.dependency_disabled_groups.iter().cloned());
        groups
    }

    fn configured_disabled_groups(&self) -> Vec<String> {
        self.strings(&["endpoints", "groupsToRemove"], "ENDPOINTS_GROUPSTOREMOVE")
            .into_iter()
            .map(|group| group.trim().to_owned())
            .filter(|group| !group.is_empty())
            .collect()
    }

    fn disabled_endpoint_keys(&self) -> BTreeSet<String> {
        self.strings(&["endpoints", "toRemove"], "ENDPOINTS_TOREMOVE")
            .into_iter()
            .map(|endpoint| normalize_endpoint(&endpoint))
            .filter(|endpoint| !endpoint.is_empty())
            .collect()
    }

    fn known_endpoint_keys() -> impl Iterator<Item = String> {
        ENDPOINT_GROUPS
            .iter()
            .flat_map(|(_, endpoints)| endpoints.split_whitespace())
            .map(ToOwned::to_owned)
    }

    fn is_endpoint_enabled_with_groups(&self, endpoint: &str, disabled_groups: &[String]) -> bool {
        if self.disabled_endpoint_keys().contains(endpoint) || !self.url_to_pdf_is_enabled(endpoint)
        {
            return false;
        }
        if ENDPOINT_GROUPS.iter().any(|(group, endpoints)| {
            !is_tool_group(group)
                && is_group_disabled(group, disabled_groups)
                && group_contains_endpoint(endpoints, endpoint)
        }) {
            return false;
        }
        if let Some((_, alternatives)) = ENDPOINT_ALTERNATIVES
            .iter()
            .find(|(configured_endpoint, _)| *configured_endpoint == endpoint)
        {
            return (endpoint == "ocr-pdf" && self.ocr_engine_is_explicitly_selected())
                || alternatives
                    .iter()
                    .any(|group| !is_group_disabled(group, disabled_groups));
        }
        !ENDPOINT_GROUPS.iter().any(|(group, endpoints)| {
            is_tool_group(group)
                && is_group_disabled(group, disabled_groups)
                && group_contains_endpoint(endpoints, endpoint)
        })
    }

    fn is_endpoint_enabled_directly(&self, endpoint: &str, disabled_groups: &[String]) -> bool {
        !self.disabled_endpoint_keys().contains(endpoint)
            && !ENDPOINT_GROUPS.iter().any(|(group, endpoints)| {
                !is_tool_group(group)
                    && is_group_disabled(group, disabled_groups)
                    && group_contains_endpoint(endpoints, endpoint)
            })
    }

    fn url_to_pdf_is_enabled(&self, endpoint: &str) -> bool {
        endpoint != "url-to-pdf"
            || env_bool("RUSTLING_PROCESSING_ENABLE_URL_TO_PDF")
                .or_else(|| env_bool("SYSTEM_ENABLE_URL_TO_PDF"))
                .unwrap_or_else(|| {
                    self.boolean(
                        &["system", "enableUrlToPDF"],
                        "SYSTEM_ENABLEURLTOPDF",
                        false,
                    )
                })
    }

    fn from_paths(settings_path: PathBuf, custom_settings_path: &Path) -> Self {
        let custom_files_dir = custom_files_dir(&settings_path);
        let mut settings = Value::Object(Map::new());
        let mut errors = Vec::new();
        for path in [settings_path.as_path(), custom_settings_path] {
            match read_yaml_file(path) {
                Ok(Some(value)) => merge_json(&mut settings, value),
                Ok(None) => {}
                Err(error) => errors.push(error),
            }
        }
        Self {
            settings,
            settings_path,
            load_error: (!errors.is_empty()).then(|| errors.join("; ")),
            custom_files_dir,
            dependency_disabled_groups: BTreeSet::new(),
            dependency_commands: BTreeMap::new(),
            dependencies_checked: true,
        }
    }

    fn login_agreement_is_enabled(&self) -> bool {
        env_bool("LEGAL_LOGINAGREEMENT_ENABLED")
            .or_else(|| env_bool("LEGAL_LOGIN_AGREEMENT_ENABLED"))
            .or_else(|| {
                value_at(&self.settings, &["legal", "loginAgreement", "enabled"])
                    .and_then(yaml_bool)
            })
            .unwrap_or(false)
    }

    fn login_agreement_show_in_anonymous_mode(&self) -> bool {
        env_bool("LEGAL_LOGINAGREEMENT_SHOWINANONYMOUSMODE")
            .or_else(|| env_bool("LEGAL_LOGIN_AGREEMENT_SHOW_IN_ANONYMOUS_MODE"))
            .or_else(|| {
                value_at(
                    &self.settings,
                    &["legal", "loginAgreement", "showInAnonymousMode"],
                )
                .and_then(yaml_bool)
            })
            .unwrap_or(true)
    }

    fn resolve_login_disclaimer(&self, requested_locale: Option<&str>) -> String {
        let mut candidates = Vec::new();
        add_locale_candidates(&mut candidates, requested_locale);
        let default_locale = self.string(
            &["system", "defaultLocale"],
            "SYSTEM_DEFAULTLOCALE",
            "en-US",
        );
        add_locale_candidates(&mut candidates, Some(&default_locale));

        for locale in candidates {
            if let Some(content) = self.read_login_disclaimer(&locale)
                && !content.trim().is_empty()
            {
                return content;
            }
        }

        crate::environment::var("LEGAL_LOGINAGREEMENT_FALLBACKTEXT")
            .or_else(|_| crate::environment::var("LEGAL_LOGIN_AGREEMENT_FALLBACK_TEXT"))
            .ok()
            .or_else(|| {
                value_at(&self.settings, &["legal", "loginAgreement", "fallbackText"])
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default()
    }

    fn read_login_disclaimer(&self, locale: &str) -> Option<String> {
        let path = login_disclaimer_path(&self.custom_files_dir, locale)?;
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_LOGIN_DISCLAIMER_BYTES_U64
        {
            return None;
        }

        let mut file = fs::File::open(path).ok()?;
        let mut bytes = Vec::with_capacity(metadata.len().try_into().ok()?);
        file.by_ref()
            .take(MAX_LOGIN_DISCLAIMER_BYTES_U64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= MAX_LOGIN_DISCLAIMER_BYTES)
            .then(|| String::from_utf8(bytes).ok())
            .flatten()
    }

    fn boolean(&self, path: &[&str], environment: &str, default: bool) -> bool {
        env_bool(environment)
            .or_else(|| value_at(&self.settings, path).and_then(yaml_bool))
            .unwrap_or(default)
    }

    fn optional_boolean(&self, path: &[&str], environment: &str) -> Option<bool> {
        env_bool(environment).or_else(|| value_at(&self.settings, path).and_then(yaml_bool))
    }

    fn string(&self, path: &[&str], environment: &str, default: &str) -> String {
        crate::environment::var(environment)
            .ok()
            .or_else(|| {
                value_at(&self.settings, path)
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| default.to_owned())
    }

    fn strings(&self, path: &[&str], environment: &str) -> Vec<String> {
        if let Ok(value) = crate::environment::var(environment) {
            return split_strings(&value);
        }
        value_at(&self.settings, path)
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// [`Self::u64`] against the `rustling.*` product settings root.
    fn product_u64(&self, path_below_root: &[&str], environment: &str, default: u64) -> u64 {
        crate::environment::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .or_else(|| product_value_at(&self.settings, path_below_root).and_then(Value::as_u64))
            .unwrap_or(default)
    }

    fn u64(&self, path: &[&str], environment: &str, default: u64) -> u64 {
        crate::environment::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .or_else(|| value_at(&self.settings, path).and_then(Value::as_u64))
            .unwrap_or(default)
    }

    fn signed_integer(&self, path: &[&str], environment: &str, default: i64) -> i64 {
        crate::environment::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .or_else(|| value_at(&self.settings, path).and_then(Value::as_i64))
            .unwrap_or(default)
    }

    fn usize(environment: &str, default: usize) -> usize {
        crate::environment::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    fn frontend_url(&self, host: Option<&str>, forwarded_proto: Option<&str>) -> String {
        let configured = self.string(&["system", "frontendUrl"], "SYSTEM_FRONTENDURL", "");
        if !configured.trim().is_empty() {
            return configured;
        }
        let Some(host) = host.map(str::trim).filter(|host| !is_loopback_host(host)) else {
            return String::new();
        };
        let scheme = forwarded_proto
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| matches!(*value, "http" | "https"))
            .unwrap_or("http");
        format!("{scheme}://{host}")
    }
}

// Model identity defaults shared with config-push detection.
pub(crate) const DEFAULT_AI_MODEL_PROVIDER: &str = "anthropic";
pub(crate) const DEFAULT_AI_SMART_MODEL: &str = "claude-haiku-4-5";
pub(crate) const DEFAULT_AI_FAST_MODEL: &str = "claude-haiku-4-5";

/// Model and provider selection pushed to the engine.
#[derive(Clone, Debug)]
pub struct AiEngineModelsPush {
    pub provider: String,
    pub smart_model: String,
    pub fast_model: String,
    pub smart_max_tokens: i64,
    pub fast_max_tokens: i64,
    pub api_key: String,
    pub base_url: String,
}

/// Request-size and cost guardrails pushed to the engine.
#[derive(Clone, Debug)]
pub struct AiEngineLimitsPush {
    pub max_pages: i64,
    pub max_characters: i64,
    pub model_max_concurrency: i64,
}

/// The engine-relevant `aiEngine.*` configuration slice.
#[derive(Clone, Debug)]
pub struct AiEnginePushSettings {
    pub enabled: bool,
    /// Whether the processor pushes settings-derived AI config to the engine on
    /// startup/save. Pinned false for env-driven deployments so the engine
    /// stays environment-controlled.
    pub push_config_to_engine: bool,
    pub models: AiEngineModelsPush,
    pub limits: AiEngineLimitsPush,
}

impl Default for AiEnginePushSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            push_config_to_engine: true,
            models: AiEngineModelsPush {
                provider: DEFAULT_AI_MODEL_PROVIDER.to_owned(),
                smart_model: DEFAULT_AI_SMART_MODEL.to_owned(),
                fast_model: DEFAULT_AI_FAST_MODEL.to_owned(),
                smart_max_tokens: 8_192,
                fast_max_tokens: 2_048,
                api_key: String::new(),
                base_url: String::new(),
            },
            limits: AiEngineLimitsPush {
                max_pages: 200,
                max_characters: 200_000,
                model_max_concurrency: 32,
            },
        }
    }
}

/// Persisted installation identity.
#[derive(Clone, Debug)]
pub struct GeneratedIdentity {
    /// Stable installation UUID (`AutomaticallyGenerated.UUID`).
    pub uuid: String,
    /// Machine secret key (`AutomaticallyGenerated.key`).
    pub key: String,
    /// The version persisted for this boot (`AutomaticallyGenerated.appVersion`).
    pub app_version: String,
    /// Whether the settings file carried no prior version: empty or the
    /// `0.0.0` placeholder.
    pub is_new_server: bool,
}

/// Read-modify-write of `section.key` values in `settings.yml` that preserves
/// every other byte of the file — comments, blank lines, key order and
/// formatting — by rewriting only the targeted value lines through the shared
/// comment-preserving editor ([`crate::settings_yaml`]). A first boot rewrites
/// just the value portion of the template's `AutomaticallyGenerated` lines and
/// keeps the banner and every comment intact. An existing section or key whose
/// spelling differs only by ASCII case is reused rather than duplicated; keys
/// the file lacks are inserted into the section, and a missing section (or
/// file) is created. Hostile
/// hand-edited shapes the editor cannot extend (flow-collection roots or
/// sections, block-scalar values, block-sequence sections) are refused with
/// the file untouched, and the edited text must reparse as a YAML mapping
/// before any byte reaches disk.
fn update_settings_file_values(
    settings_path: &Path,
    section: &str,
    entries: &[(&str, serde_yaml::Value)],
) -> Result<(), String> {
    let contents = match fs::read_to_string(settings_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "could not read {}: {error}",
                settings_path.display()
            ));
        }
    };
    if !contents.trim().is_empty() {
        let parsed = serde_yaml::from_str::<serde_yaml::Value>(&contents).map_err(|error| {
            format!(
                "could not parse {} for a settings update: {error}",
                settings_path.display()
            )
        })?;
        if !parsed.is_mapping() {
            return Err(format!(
                "could not update {} because its root is not a mapping",
                settings_path.display()
            ));
        }
    }
    let updated = crate::settings_yaml::upsert_section_values(&contents, section, entries)
        .map_err(|error| format!("could not update {}: {error}", settings_path.display()))?;
    if updated == contents {
        return Ok(());
    }
    // Prove the edited text still parses as a YAML mapping before any byte
    // reaches disk: an editor defect on a hostile hand-edited shape must fail
    // the update cleanly (the identity caller stays fail-open), never write a
    // corrupted settings file.
    if !serde_yaml::from_str::<serde_yaml::Value>(&updated).is_ok_and(|parsed| parsed.is_mapping())
    {
        return Err(format!(
            "could not update {}: the edited settings would no longer parse as a YAML mapping",
            settings_path.display()
        ));
    }
    if let Some(parent) = settings_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(settings_path, updated)
        .map_err(|error| format!("could not write {}: {error}", settings_path.display()))
}

/// Whether a persisted identity is a canonical hyphenated UUID.
fn is_valid_settings_uuid(value: &str) -> bool {
    let value = value.trim();
    if value.len() != 36 {
        return false;
    }
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn random_uuid_v4() -> String {
    use rand::RngExt as _;
    let mut bytes = [0_u8; 16];
    rand::rng().fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

fn read_yaml_file(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    // An empty document — zero bytes, whitespace, or comments only — parses to
    // YAML `null`. Treat it as an absent file rather than a value: desktop
    // startup always creates a zero-byte `custom_settings.yml`, and merging a
    // `Null` overlay would replace the ENTIRE settings snapshot, blanking every
    // configured value (and re-rolling the install identity) on every boot.
    // This keeps `settings.yml` in full effect.
    serde_yaml::from_str(&contents)
        .map(|value| match value {
            Value::Null => None,
            value => Some(value),
        })
        .map_err(|error| format!("could not parse {}: {error}", path.display()))
}

fn custom_files_dir(settings_path: &Path) -> PathBuf {
    installation_path(settings_path).join("customFiles")
}

fn installation_path(settings_path: &Path) -> PathBuf {
    let settings_dir = settings_path.parent().unwrap_or_else(|| Path::new("."));
    (settings_dir.file_name() == Some(std::ffi::OsStr::new("configs")))
        .then(|| settings_dir.parent())
        .flatten()
        .unwrap_or(settings_dir)
        .to_path_buf()
}

fn resolve_configured_path(default: &Path, configured: &str) -> PathBuf {
    let configured = configured.trim();
    if configured.is_empty() {
        return default.to_path_buf();
    }
    PathBuf::from(configured)
}

fn add_locale_candidates(candidates: &mut Vec<String>, locale: Option<&str>) {
    let Some(locale) = locale.filter(|locale| is_valid_locale(locale)) else {
        return;
    };
    if !candidates.iter().any(|candidate| candidate == locale) {
        candidates.push(locale.to_owned());
    }
    let base = locale.split(['_', '-']).next().unwrap_or(locale);
    if base != locale && !candidates.iter().any(|candidate| candidate == base) {
        candidates.push(base.to_owned());
    }
}

fn login_disclaimer_path(custom_files_dir: &Path, locale: &str) -> Option<PathBuf> {
    if !is_valid_locale(locale) {
        return None;
    }
    let directory = custom_files_dir.join("disclaimer");
    let path = directory.join(format!("{locale}.md"));
    path.starts_with(&directory).then_some(path)
}

pub(crate) fn is_valid_locale(locale: &str) -> bool {
    if !(2..=35).contains(&locale.len()) {
        return false;
    }
    let mut parts = locale.split(['_', '-']);
    let Some(language) = parts.next() else {
        return false;
    };
    if !(2..=3).contains(&language.len())
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    parts.all(|part| {
        (2..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

fn merge_json(target: &mut Value, overlay: Value) {
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = target.get_mut(&key) {
                    merge_json(existing, value);
                } else {
                    target.insert(key, value);
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}

/// Product-rooted settings lookup below `rustling.*`.
fn product_value_at<'a>(value: &'a Value, path_below_root: &[&str]) -> Option<&'a Value> {
    let mut path = Vec::with_capacity(path_below_root.len() + 1);
    path.push("rustling");
    path.extend_from_slice(path_below_root);
    value_at(value, &path)
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn env_bool(name: &str) -> Option<bool> {
    crate::environment::var(name)
        .ok()
        .and_then(|value| parse_boolean(&value))
}

/// Parses a configuration boolean using common environment-variable forms.
fn parse_boolean(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Reads a YAML setting as a boolean, accepting string and numeric forms used
/// by environment-backed configuration.
fn yaml_bool(value: &Value) -> Option<bool> {
    value
        .as_bool()
        .or_else(|| value.as_str().and_then(parse_boolean))
        .or_else(|| match value.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        })
}

fn split_strings(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim().trim_start_matches('/').to_owned()
}

fn is_tool_group(group: &str) -> bool {
    crate::runtime_dependencies::dependency_group_names()
        .any(|dependency_group| dependency_group == group)
}

fn is_group_disabled(group: &str, disabled_groups: &[String]) -> bool {
    disabled_groups.iter().any(|disabled| disabled == group)
}

fn group_contains_endpoint(endpoints: &str, endpoint: &str) -> bool {
    endpoints
        .split_whitespace()
        .any(|member| member == endpoint)
}

fn endpoint_key_for_uri(uri: &str) -> Option<String> {
    let api_start = uri.find("/api/v1")?;
    let parts = uri[api_start..].split('/').collect::<Vec<_>>();
    if parts.len() <= 4 {
        return None;
    }
    if parts[3] == "convert" && parts.len() > 5 {
        return Some(format!("{}-to-{}", parts[4], parts[5]));
    }
    Some(parts[4].to_owned())
}

fn insert<T: Serialize>(config: &mut Map<String, Value>, key: &str, value: T) {
    config.insert(
        key.to_owned(),
        serde_json::to_value(value).unwrap_or(Value::Null),
    );
}

fn tsa_presets() -> Value {
    json!([
        { "label": "DigiCert", "url": "http://timestamp.digicert.com" },
        { "label": "Sectigo", "url": "http://timestamp.sectigo.com" },
        { "label": "SSL.com", "url": "http://ts.ssl.com" },
        { "label": "FreeTSA", "url": "https://freetsa.org/tsr" },
        { "label": "MeSign", "url": "http://tsa.mesign.com" }
    ])
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    if matches!(host, "localhost" | "127.0.0.1" | "::1" | "0:0:0:0:0:0:0:1") {
        return true;
    }
    let host = if host.starts_with('[') {
        host.trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "0:0:0:0:0:0:0:1")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
    };

    use regex::Regex;
    use serde_json::json;
    use tempfile::tempdir;

    use std::path::Path;

    use super::{
        ENDPOINT_ALTERNATIVES, ENDPOINT_GROUPS, FUNCTIONAL_GROUPS, RuntimeConfig,
        endpoint_key_for_uri, is_tool_group, merge_json, parse_boolean, split_strings, yaml_bool,
    };

    #[test]
    fn maximum_render_dpi_matches_java_default_and_yaml() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        assert_eq!(config.max_render_dpi(), 500);

        fs::write(&settings, "system:\n  maxDPI: 360\n")?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        assert_eq!(config.max_render_dpi(), 360);
        Ok(())
    }

    #[test]
    fn ocr_process_limits_and_timeouts_match_java_defaults_and_yaml()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let defaults = config.ocr_process_settings();
        assert_eq!(defaults.ocrmypdf_session_limit, 2);
        assert_eq!(defaults.ocrmypdf_timeout.as_secs(), 30 * 60);
        assert_eq!(defaults.tesseract_session_limit, 1);
        assert_eq!(defaults.tesseract_timeout.as_secs(), 30 * 60);

        fs::write(
            &settings,
            "processExecutor:\n  sessionLimit:\n    ocrMyPdfSessionLimit: 4\n    tesseractSessionLimit: 3\n  timeoutMinutes:\n    ocrMyPdfTimeoutMinutes: 12\n    tesseractTimeoutMinutes: 9\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let configured = config.ocr_process_settings();
        assert_eq!(configured.ocrmypdf_session_limit, 4);
        assert_eq!(configured.ocrmypdf_timeout.as_secs(), 12 * 60);
        assert_eq!(configured.tesseract_session_limit, 3);
        assert_eq!(configured.tesseract_timeout.as_secs(), 9 * 60);
        Ok(())
    }

    #[test]
    fn paddle_ocr_is_disabled_by_default_and_requires_every_explicit_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        assert_eq!(config.paddle_ocr_config()?, None);

        fs::write(&settings, "ocr:\n  engine: paddle\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        assert_eq!(
            config.paddle_ocr_config(),
            Err("ocr.paddle.onnxRuntimePath is required when ocr.engine is paddle".to_owned())
        );
        Ok(())
    }

    #[test]
    fn paddle_ocr_yaml_keeps_all_paths_explicit_and_enables_ocr_availability()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "ocr:\n  engine: paddle\n  paddle:\n    onnxRuntimePath: /opt/onnx/libonnxruntime.so\n    detectorModelPath: /models/detector.onnx\n    recognizerModelPath: /models/recognizer.onnx\n    dictionaryPath: /models/ppocrv6_dict.txt\n    textLayerFontPath: /fonts/NotoSansCJK-Regular.otf\n",
        )?;
        let mut config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let paddle = config
            .paddle_ocr_config()?
            .ok_or("expected Paddle OCR configuration")?;
        assert_eq!(
            paddle.onnx_runtime_path,
            Path::new("/opt/onnx/libonnxruntime.so")
        );
        assert_eq!(
            paddle.detector_model_path,
            Path::new("/models/detector.onnx")
        );
        assert_eq!(
            paddle.recognizer_model_path,
            Path::new("/models/recognizer.onnx")
        );
        assert_eq!(
            paddle.dictionary_path,
            Path::new("/models/ppocrv6_dict.txt")
        );
        assert_eq!(
            paddle.text_layer_font_path,
            Path::new("/fonts/NotoSansCJK-Regular.otf")
        );

        config
            .dependency_disabled_groups
            .extend(["OCRmyPDF".to_owned(), "tesseract".to_owned()]);
        assert!(config.is_endpoint_enabled("ocr-pdf"));
        Ok(())
    }

    #[test]
    fn invalid_ocr_engine_is_reported_instead_of_falling_back()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "ocr:\n  engine: Paddle\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        assert_eq!(
            config.paddle_ocr_config(),
            Err("ocr.engine must be 'auto' or 'paddle', got 'Paddle'".to_owned())
        );

        // The endpoint stays advertised so the request fails with the
        // configuration error above instead of `501 Not Implemented`, which
        // would blame the absent Tesseract for the operator's typo.
        config
            .dependency_disabled_groups
            .extend(["OCRmyPDF".to_owned(), "tesseract".to_owned()]);
        assert!(config.is_endpoint_enabled("ocr-pdf"));
        Ok(())
    }

    #[test]
    fn repair_process_limits_and_timeouts_use_defaults_and_yaml()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let defaults = config.repair_process_settings();
        assert_eq!(defaults.qpdf_session_limit, 2);
        assert_eq!(defaults.qpdf_timeout.as_secs(), 30 * 60);

        fs::write(
            &settings,
            "processExecutor:\n  sessionLimit:\n    qpdfSessionLimit: 5\n  timeoutMinutes:\n    qpdfTimeoutMinutes: 11\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let configured = config.repair_process_settings();
        assert_eq!(configured.qpdf_session_limit, 5);
        assert_eq!(configured.qpdf_timeout.as_secs(), 11 * 60);
        Ok(())
    }

    #[test]
    fn dependency_commands_are_available_only_for_enabled_discovered_groups()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let command = directory.path().join("qpdf");
        config
            .dependency_commands
            .insert("qpdf".to_owned(), command.clone());
        assert_eq!(config.dependency_command("qpdf"), Some(command));

        config.dependency_disabled_groups.insert("qpdf".to_owned());
        assert_eq!(config.dependency_command("qpdf"), None);
        Ok(())
    }

    #[test]
    fn every_dependency_group_in_the_availability_model_has_a_discovery_spec() {
        for (group, _) in ENDPOINT_GROUPS {
            assert!(
                FUNCTIONAL_GROUPS.contains(group) || is_tool_group(group),
                "availability group {group} has no dependency spec"
            );
        }
        for (_, alternatives) in ENDPOINT_ALTERNATIVES {
            for group in *alternatives {
                assert!(
                    is_tool_group(group),
                    "alternative group {group} has no dependency spec"
                );
            }
        }
    }

    #[test]
    fn missing_new_dependencies_disable_only_their_unconditional_endpoints()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config.dependency_disabled_groups.extend([
            "FFmpeg".to_owned(),
            "unrar".to_owned(),
            "veraPDF".to_owned(),
        ]);

        for group in ["FFmpeg", "unrar", "veraPDF"] {
            assert!(!config.is_group_enabled(group));
        }
        let availability = config.endpoint_availability(&[
            "pdf-to-video".to_owned(),
            "cbr-to-pdf".to_owned(),
            "verify-pdf".to_owned(),
        ]);
        assert!(!availability["pdf-to-video"].enabled);
        assert_eq!(availability["pdf-to-video"].reason, Some("DEPENDENCY"));
        assert!(!availability["cbr-to-pdf"].enabled);
        assert_eq!(availability["cbr-to-pdf"].reason, Some("DEPENDENCY"));
        // `verify-pdf` stays enabled without veraPDF on purpose: a PDF that
        // declares no validation profile completes through the native
        // `not-pdfa` path. Only a declared profile needs veraPDF, and that is a
        // request-time 501. See contracts/verify-pdf.md — route-level
        // availability has no "conditional capability" state, and blanking the
        // whole route would deny the case that works.
        assert!(availability["verify-pdf"].enabled);
        assert_eq!(availability["verify-pdf"].reason, None);
        Ok(())
    }

    #[test]
    fn weasyprint_dependency_disables_html_markdown_and_eml_to_pdf()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        // enableUrlToPDF must be turned on so url-to-pdf's availability reflects only the
        // WeasyPrint dependency being tested here, not its own separate CONFIG-disabled default.
        fs::write(&settings, "system:\n  enableUrlToPDF: true\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .insert("Weasyprint".to_owned());

        let availability = config.endpoint_availability(&[
            "html-to-pdf".to_owned(),
            "url-to-pdf".to_owned(),
            "markdown-to-pdf".to_owned(),
            "eml-to-pdf".to_owned(),
        ]);
        for endpoint in ["html-to-pdf", "url-to-pdf", "markdown-to-pdf", "eml-to-pdf"] {
            assert!(
                !availability[endpoint].enabled,
                "{endpoint} should be disabled when WeasyPrint is unavailable"
            );
            assert_eq!(availability[endpoint].reason, Some("DEPENDENCY"));
        }
        Ok(())
    }

    #[test]
    fn pdftohtml_gates_neither_pdf_to_html_nor_pdf_to_markdown()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .insert("Pdftohtml".to_owned());
        // `pdf-to-html` falls back to the native PDFium renderer, and
        // `pdf-to-markdown` never invoked pdftohtml at all.
        assert!(config.is_endpoint_enabled("pdf-to-html"));
        assert!(config.is_endpoint_enabled("pdf-to-markdown"));
        Ok(())
    }

    #[test]
    fn custom_settings_override_base_settings() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        let custom = directory.path().join("custom_settings.yml");
        fs::write(
            &settings,
            "system:\n  defaultLocale: en-US\nui:\n  logoStyle: classic\n",
        )?;
        fs::write(
            &custom,
            "system:\n  defaultLocale: vi-VN\nsecurity:\n  timestamp:\n    defaultTsaUrl: https://tsa.example.test\n    customTsaUrls: [https://custom-tsa.example.test]\n",
        )?;
        let config = RuntimeConfig::from_files(settings, custom);
        let app_config = config.app_config(None, None);
        assert_eq!(app_config["defaultLocale"], "vi-VN");
        assert_eq!(app_config["logoStyle"], "classic");
        assert_eq!(
            config.timestamp_settings(),
            (
                "https://tsa.example.test".to_owned(),
                vec!["https://custom-tsa.example.test".to_owned()]
            )
        );
        Ok(())
    }

    /// Settings keys for features this build no longer has: the opt-in
    /// analytics (`enableAnalytics` / `enablePosthog` / `enableScarf`) and the
    /// update check (`showUpdate`). An existing install's `settings.yml` still
    /// carries them, so this pins the two halves of that promise: the keys are
    /// IGNORED (never surfaced on the public app config, which is what the SPA
    /// reads) and never REFUSED (the file still loads cleanly and every
    /// neighbouring key still resolves).
    #[test]
    fn removed_feature_settings_keys_are_ignored_not_refused()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "system:\n  enableAnalytics: true\n  enablePosthog: true\n  enableScarf: true\n  showUpdate: true\n  defaultLocale: en-GB\n",
        )?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        assert_eq!(config.load_error, None);

        let app_config = config.app_config(None, None);
        for key in [
            "enableAnalytics",
            "enablePosthog",
            "enableScarf",
            "shouldShowUpdate",
            "showUpdate",
        ] {
            assert!(
                app_config.get(key).is_none(),
                "{key} must not reappear on the public app config",
            );
        }
        // A neighbouring key in the same `system` block still resolves, proving
        // the unrecognised keys did not poison the load.
        assert_eq!(app_config["defaultLocale"], "en-GB");
        Ok(())
    }

    #[test]
    fn endpoint_statuses_use_the_configured_disabled_endpoint_list()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "endpoints:\n  toRemove: [compress-pdf, /rotate-pdf]\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        assert!(!config.is_endpoint_enabled("/compress-pdf"));
        assert!(!config.is_endpoint_enabled("rotate-pdf"));
        assert!(config.is_endpoint_enabled("merge-pdfs"));
        assert_eq!(
            config.disabled_endpoint_statuses().get("rotate-pdf"),
            Some(&false)
        );
        Ok(())
    }

    #[test]
    fn merge_replaces_scalars_and_recurses_into_objects() {
        let mut base = json!({ "system": { "defaultLocale": "en-US", "logoStyle": "classic" } });
        merge_json(&mut base, json!({ "system": { "defaultLocale": "vi-VN" } }));
        assert_eq!(
            base,
            json!({ "system": { "defaultLocale": "vi-VN", "logoStyle": "classic" } })
        );
        assert_eq!(split_strings("one, two,,three"), ["one", "two", "three"]);
    }

    #[test]
    fn parse_boolean_matches_springs_relaxed_vocabulary() {
        for truthy in ["true", "on", "yes", "1", " TRUE ", "On", "YeS"] {
            assert_eq!(parse_boolean(truthy), Some(true), "{truthy:?}");
        }
        for falsy in ["false", "off", "no", "0", " FALSE ", "oFf", "No"] {
            assert_eq!(parse_boolean(falsy), Some(false), "{falsy:?}");
        }
        for malformed in ["", "2", "enable", "true!", "y", "n", "t", "f", "10"] {
            assert_eq!(parse_boolean(malformed), None, "{malformed:?}");
        }
    }

    #[test]
    fn yaml_bool_reads_yaml_1_1_spellings_the_way_snakeyaml_gives_them_to_java()
    -> Result<(), Box<dyn std::error::Error>> {
        // serde_yaml (YAML 1.2) delivers unquoted yes/on/no/off as strings.
        for (yaml, expected) in [
            ("value: true", Some(true)),
            ("value: false", Some(false)),
            ("value: yes", Some(true)),
            ("value: on", Some(true)),
            ("value: no", Some(false)),
            ("value: off", Some(false)),
            ("value: \"1\"", Some(true)),
            ("value: \"0\"", Some(false)),
            // Unquoted numeric scalars are also accepted.
            ("value: 1", Some(true)),
            ("value: 0", Some(false)),
            ("value: banana", None),
            ("value: 42", None),
        ] {
            let parsed: serde_json::Value = serde_yaml::from_str(yaml)?;
            assert_eq!(yaml_bool(&parsed["value"]), expected, "yaml {yaml:?}");
        }
        Ok(())
    }

    #[test]
    fn boolean_settings_read_every_java_yaml_spelling() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        for (spelling, expected) in [("yes", true), ("on", true), ("\"1\"", true), ("no", false)] {
            fs::write(
                &settings,
                format!("system:\n  googlevisibility: {spelling}\n"),
            )?;
            let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
            assert_eq!(
                config.google_visibility(),
                expected,
                "googlevisibility: {spelling}"
            );
        }
        // Malformed values fall back to the default (false here).
        fs::write(&settings, "system:\n  googlevisibility: banana\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        assert!(!config.google_visibility());
        Ok(())
    }

    #[test]
    fn availability_includes_known_and_explicitly_disabled_endpoints()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "endpoints:\n  toRemove: [compress-pdf, unknown-tool]\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let availability = config.endpoint_availability(&[]);
        assert!(availability["merge-pdfs"].enabled);
        assert!(!availability["compress-pdf"].enabled);
        assert_eq!(availability["compress-pdf"].reason, Some("CONFIG"));
        assert!(!availability["unknown-tool"].enabled);
        Ok(())
    }

    #[test]
    fn dependency_groups_report_a_distinct_availability_reason()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .insert("LibreOffice".to_owned());
        let availability = config.endpoint_availability(&["pdf-to-word".to_owned()]);
        assert!(!availability["pdf-to-word"].enabled);
        assert_eq!(availability["pdf-to-word"].reason, Some("DEPENDENCY"));
        let uri_availability = config.endpoint_availability_for_uri("/api/v1/convert/pdf/word");
        assert!(!uri_availability.is_enabled());
        assert_eq!(uri_availability.reason(), Some("DEPENDENCY"));
        assert_eq!(config.app_config(None, None)["dependenciesReady"], true);
        Ok(())
    }

    /// The regression this whole change exists for: without `LibreOffice`
    /// installed, "Convert to PDF" used to report itself unavailable. It now has
    /// a built-in engine, so a missing `LibreOffice` must leave it enabled while
    /// the PDF → office direction, which has no built-in engine, stays gated.
    #[test]
    fn office_to_pdf_survives_a_missing_libreoffice() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .insert("LibreOffice".to_owned());

        let availability =
            config.endpoint_availability(&["file-to-pdf".to_owned(), "pdf-to-word".to_owned()]);
        assert!(availability["file-to-pdf"].enabled);
        assert_eq!(availability["file-to-pdf"].reason, None);
        assert!(!availability["pdf-to-word"].enabled);

        let uri_availability = config.endpoint_availability_for_uri("/api/v1/convert/file/pdf");
        assert!(uri_availability.is_enabled());
        Ok(())
    }

    #[test]
    fn ocr_availability_requires_at_least_one_discovered_tool()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .insert("OCRmyPDF".to_owned());
        assert!(config.is_endpoint_enabled("ocr-pdf"));

        config
            .dependency_disabled_groups
            .insert("tesseract".to_owned());
        assert!(!config.is_endpoint_enabled("ocr-pdf"));
        assert_eq!(
            config.endpoint_availability(&["ocr-pdf".to_owned()])["ocr-pdf"].reason,
            Some("DEPENDENCY")
        );
        Ok(())
    }

    #[test]
    fn pdf_to_html_remains_available_without_external_converters()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .extend(["LibreOffice".to_owned(), "Pdftohtml".to_owned()]);

        assert!(config.is_endpoint_enabled("pdf-to-html"));
        assert!(config.is_endpoint_enabled("pdf-to-markdown"));
        Ok(())
    }

    #[test]
    fn group_configuration_disables_functional_and_fallback_tool_endpoints()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            // `Ghostscript` is a removed dependency group. It must still be accepted in
            // `groupsToRemove` and simply ignored, never refused.
            "endpoints:\n  groupsToRemove: [PageOps, LibreOffice, Ghostscript]\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        assert!(!config.is_group_enabled("PageOps"));
        assert!(!config.is_group_enabled("LibreOffice"));
        assert!(config.is_group_enabled("Convert"));
        assert!(!config.is_endpoint_enabled("merge-pdfs"));
        assert!(config.is_endpoint_enabled("repair"));
        assert!(!config.is_endpoint_enabled("pdf-to-word"));
        // `file-to-pdf` no longer belongs to the `LibreOffice` group, so
        // removing that group leaves the built-in engine's endpoint alone.
        assert!(config.is_endpoint_enabled("file-to-pdf"));
        assert!(config.is_endpoint_enabled("pdf-to-img"));
        let statuses = config.disabled_endpoint_statuses();
        assert_eq!(statuses.get("merge-pdfs"), Some(&false));
        assert!(!statuses.contains_key("repair"));
        Ok(())
    }

    #[test]
    fn disabling_pageops_or_advance_disables_the_registered_overlay_pdfs_route()
    -> Result<(), Box<dyn std::error::Error>> {
        // Regression test for the overlay-pdf/overlay-pdfs key mismatch: the route is
        // `/api/v1/general/overlay-pdfs` (OVERLAY_PDFS_PATH in lib.rs), so
        // `endpoint_key_for_uri` derives "overlay-pdfs". Before the fix, ENDPOINT_GROUPS listed
        // "overlay-pdf" (singular) under both PageOps and Advance, so an administrator who
        // disabled either group believed overlay was off while the endpoint kept answering.
        // Assert directly on the URI, the same input the axum middleware sees.
        const OVERLAY_URI: &str = "/api/v1/general/overlay-pdfs";
        for group in ["PageOps", "Advance"] {
            let directory = tempdir()?;
            let settings = directory.path().join("settings.yml");
            fs::write(
                &settings,
                format!("endpoints:\n  groupsToRemove: [{group}]\n"),
            )?;
            let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
            assert!(
                !config.is_endpoint_enabled_for_uri(OVERLAY_URI),
                "disabling {group} must disable {OVERLAY_URI}"
            );
        }
        Ok(())
    }

    #[test]
    fn disabling_pageops_or_advance_reports_the_spa_overlay_key_disabled()
    -> Result<(), Box<dyn std::error::Error>> {
        // Spelling only the derived `overlay-pdfs` fixes the 403 but not the UI. The SPA tool
        // registry declares the overlay tool as `overlay-pdf`, and `useEndpointConfig.ts` reads
        // the *no-query* endpoints-availability map and treats a key missing from it as enabled —
        // so with only the derived key in the table the endpoint correctly refuses requests while
        // the tool stays advertised in the disabled group's UI. Assert on the map the UI reads,
        // not merely on the route.
        for group in ["PageOps", "Advance"] {
            let directory = tempdir()?;
            let settings = directory.path().join("settings.yml");
            fs::write(
                &settings,
                format!("endpoints:\n  groupsToRemove: [{group}]\n"),
            )?;
            let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
            let availability = config.endpoint_availability(&[]);
            let advertised = availability.get("overlay-pdf").unwrap_or_else(|| {
                panic!(
                    "the no-query availability map must carry the SPA registry key `overlay-pdf`; \
                     without it, disabling {group} leaves the overlay tool advertised as enabled"
                )
            });
            assert!(
                !advertised.enabled,
                "disabling {group} must report `overlay-pdf` disabled to the UI"
            );
            assert_eq!(advertised.reason, Some("CONFIG"));
            // The registered route stays gated by its own derived key.
            assert!(!config.is_endpoint_enabled_for_uri("/api/v1/general/overlay-pdfs"));
        }
        Ok(())
    }

    #[test]
    fn every_spa_tool_registry_key_is_present_in_the_group_table()
    -> Result<(), Box<dyn std::error::Error>> {
        // A registry key absent from ENDPOINT_GROUPS never appears in the availability map, and
        // `useEndpointConfig.ts` reads a missing key as enabled — so the tool stays advertised
        // however the administrator has configured groups. Two instances of that were found by
        // hand (`overlay-pdf`, `form-fill`); this closes the class.
        let registry_keys = spa_tool_registry_keys()?;
        assert!(
            registry_keys.len() > 40,
            "found only {} registry keys; the tool-registry scan likely stopped matching",
            registry_keys.len()
        );
        assert!(registry_keys.contains("overlay-pdf"));
        assert!(registry_keys.contains("form-fill"));

        let group_keys = group_keys();
        let missing: Vec<&str> = registry_keys
            .iter()
            .map(String::as_str)
            .filter(|key| !group_keys.contains(key))
            .collect();
        assert!(
            missing.is_empty(),
            "SPA tool-registry key(s) are absent from ENDPOINT_GROUPS, so the availability map \
             never mentions them and the UI advertises those tools as enabled whatever the \
             administrator disabled: {missing:?}"
        );
        Ok(())
    }

    #[test]
    fn endpoint_keys_follow_the_java_uri_mapping() {
        assert_eq!(
            endpoint_key_for_uri("/api/v1/general/remove-pages"),
            Some("remove-pages".to_owned())
        );
        assert_eq!(
            endpoint_key_for_uri("/api/v1/convert/pdf/img"),
            Some("pdf-to-img".to_owned())
        );
        assert_eq!(endpoint_key_for_uri("/api/v1/general"), None);
    }

    /// Every endpoint key `endpoint_key_for_uri` derives from a route this crate registers,
    /// mapped to the registered paths that yield it.
    ///
    /// Walks the crate's own `src/` tree at test time rather than a hand-maintained duplicate
    /// list, so a route added in any module is covered, not only the ones in lib.rs — `pipeline`,
    /// `smtp_mail`, `classification`, and the AI proxies all register their own. Each file is cut
    /// at its first `#[cfg(test)]`, which is always its trailing test module, so fixture routers
    /// such as lib.rs's `/api/v1/misc/echo` are not mistaken for shipped routes. Only a path that
    /// reaches a `.route(...)` call counts: `classification.rs` also declares
    /// `/api/v1/documents/classify`, but that is the *engine* path it proxies to, not a route
    /// this service serves.
    fn registered_routes() -> Result<BTreeMap<String, BTreeSet<String>>, Box<dyn std::error::Error>>
    {
        let string_const = Regex::new(r#"const\s+([A-Za-z0-9_]+)\s*:\s*&str\s*=\s*"([^"]*)"\s*;"#)?;
        let route_by_name = Regex::new(r"\.route\(\s*([A-Za-z0-9_]+)\s*,")?;
        let route_by_literal = Regex::new(r#"\.route\(\s*"([^"]*)"\s*,"#)?;

        let mut constants: BTreeMap<String, String> = BTreeMap::new();
        let mut registered_names: BTreeSet<String> = BTreeSet::new();
        let mut registered_paths: BTreeSet<String> = BTreeSet::new();
        let mut scanned = 0usize;
        let mut pending = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let source = fs::read_to_string(&path)?;
                let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();
                scanned += 1;
                for capture in string_const.captures_iter(shipped) {
                    constants.insert(capture[1].to_owned(), capture[2].to_owned());
                }
                for capture in route_by_name.captures_iter(shipped) {
                    registered_names.insert(capture[1].to_owned());
                }
                for capture in route_by_literal.captures_iter(shipped) {
                    registered_paths.insert(capture[1].to_owned());
                }
            }
        }
        assert!(
            scanned > 50,
            "scanned only {scanned} source files; the src walk is not reaching the crate"
        );

        for name in &registered_names {
            let path = constants.get(name).ok_or_else(|| {
                format!("`.route({name}, ...)` names a constant this scan could not resolve")
            })?;
            registered_paths.insert(path.clone());
        }
        let mut routes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for path in registered_paths {
            if let Some(key) = endpoint_key_for_uri(&path) {
                routes.entry(key).or_default().insert(path);
            }
        }
        assert!(
            routes.len() > 100,
            "found only {} route keys; the scanning regexes likely stopped matching the crate's \
             current formatting",
            routes.len()
        );
        Ok(routes)
    }

    /// Every endpoint key the SPA's tool registry asks the availability map about.
    ///
    /// Read from the frontend sources at test time, because the two trees have to agree and
    /// nothing else makes them: `useEndpointConfig.ts` fetches the no-query
    /// `endpoints-availability` map and treats a key it cannot find there as enabled, so a
    /// registry spelling absent from `ENDPOINT_GROUPS` leaves its tool advertised no matter what
    /// the administrator disabled. A checkout without `frontend/` fails this loudly rather than
    /// skipping: a silent skip would be the same blind spot with extra steps.
    fn spa_tool_registry_keys() -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
        let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../frontend/editor/src");
        let registry_path = frontend.join("core/data/useTranslatedToolRegistry.tsx");
        let registry = fs::read_to_string(&registry_path)
            .map_err(|error| format!("cannot read {}: {error}", registry_path.display()))?;

        let quoted = Regex::new(r#""([^"]*)""#)?;
        let inline_list = Regex::new(r"endpoints:\s*\[([^\]]*)\]")?;
        let mut keys = BTreeSet::new();
        let mut inline_entries = 0usize;
        for capture in inline_list.captures_iter(&registry) {
            inline_entries += 1;
            for key in quoted.captures_iter(&capture[1]) {
                keys.insert(key[1].to_owned());
            }
        }

        // One registry entry builds its list in code rather than spelling it inline
        // (`endpoints: Array.from(new Set(Object.values(SPLIT_ENDPOINT_NAMES)))`), so resolve
        // that constant too. Pinning the count means a second computed entry cannot slip past
        // this scan unnoticed.
        let declared_entries = registry.matches("endpoints:").count();
        assert_eq!(
            declared_entries,
            inline_entries + 1,
            "the tool registry declares {declared_entries} endpoint lists but only \
             {inline_entries} are inline arrays; a computed list other than SPLIT_ENDPOINT_NAMES \
             would be invisible to this scan"
        );
        let split_constants_path = frontend.join("core/constants/splitConstants.ts");
        let split_constants = fs::read_to_string(&split_constants_path)
            .map_err(|error| format!("cannot read {}: {error}", split_constants_path.display()))?;
        let split_block = split_constants
            .split_once("export const ENDPOINTS = {")
            .and_then(|(_, rest)| rest.split_once('}'))
            .ok_or("splitConstants.ts no longer declares `export const ENDPOINTS = { .. }`")?
            .0;
        for key in quoted.captures_iter(split_block) {
            keys.insert(key[1].to_owned());
        }
        assert!(
            keys.contains("split-for-poster-print"),
            "SPLIT_ENDPOINT_NAMES did not resolve; its declared members are missing from the scan"
        );
        Ok(keys)
    }

    fn group_keys() -> BTreeSet<&'static str> {
        ENDPOINT_GROUPS
            .iter()
            .flat_map(|(_, endpoints)| endpoints.split_whitespace())
            .collect()
    }

    /// The two shapes of the "table written against the tool's display name" mistake, and
    /// deliberately only those two — an edit-distance rule would flag the inert upstream-only
    /// entries (`pdf-to-json` against `pdf-to-json`-less routes, `compare`, `view-pdf`, …) and
    /// train the next reader to ignore the failure.
    fn is_display_name_lookalike(group_key: &str, route_key: &str) -> bool {
        // A trailing `s`, in either direction: `overlay-pdf` against the registered
        // `overlay-pdfs`.
        if route_key.strip_suffix('s') == Some(group_key)
            || group_key.strip_suffix('s') == Some(route_key)
        {
            return true;
        }
        // A dropped `-to-` segment: `text-editor-pdf` against the registered
        // `text-editor-to-pdf`, where the tool is named after its two file formats but the route
        // is `/api/v1/convert/<a>/<b>`.
        route_key
            .split_once("-to-")
            .is_some_and(|(before, after)| format!("{before}-{after}") == group_key)
    }

    #[test]
    fn no_ungated_route_hides_behind_a_display_name_lookalike_group_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let routes = registered_routes()?;
        let group_keys = group_keys();

        // Pin the two known regressions directly, so they stay caught even if the general rule
        // below is ever loosened: each route's derived key must be what `endpoint_key_for_uri`
        // computes for its registered path, and must be present in the table.
        assert_eq!(
            endpoint_key_for_uri("/api/v1/general/overlay-pdfs"),
            Some("overlay-pdfs".to_owned())
        );
        assert!(routes.contains_key("overlay-pdfs"));
        assert!(group_keys.contains("overlay-pdfs"));
        assert_eq!(
            endpoint_key_for_uri("/api/v1/convert/text-editor/pdf"),
            Some("text-editor-to-pdf".to_owned())
        );
        assert_eq!(
            endpoint_key_for_uri("/api/v1/convert/pdf/text-editor/metadata"),
            Some("pdf-to-text-editor".to_owned())
        );
        assert!(routes.contains_key("text-editor-to-pdf"));
        assert!(routes.contains_key("pdf-to-text-editor"));
        assert!(group_keys.contains("text-editor-to-pdf"));
        assert!(group_keys.contains("pdf-to-text-editor"));

        // A group key that matches no route is normally inert: it can be an SPA
        // tool-registry spelling kept so the UI can read the tool's availability.
        //
        // The defect is narrower than "a key matches no route". It is a *registered route that no
        // group can disable* sitting next to a group key a reader would take for that route — the
        // overlay-pdf/overlay-pdfs class of bug, where the table was written against the tool's
        // display name rather than the key `endpoint_key_for_uri` derives, so the group silently
        // never disables its own endpoint. Once the derived key is in the table the group does
        // disable the route, and the display-name key beside it is a harmless alias, so only
        // routes that are in no group at all are reported.
        let mut lookalikes = Vec::new();
        for route_key in routes.keys() {
            if group_keys.contains(route_key.as_str()) {
                continue;
            }
            for &group_key in &group_keys {
                if routes.contains_key(group_key) {
                    continue;
                }
                if is_display_name_lookalike(group_key, route_key) {
                    lookalikes.push((group_key.to_owned(), route_key.clone()));
                }
            }
        }
        assert!(
            lookalikes.is_empty(),
            "group key(s) name a registered route that is in no group, differing from its derived \
             key only by a trailing 's' or a dropped '-to-', so disabling the group cannot \
             actually disable the endpoint, (group_key, route_key): {lookalikes:?}"
        );
        Ok(())
    }

    #[test]
    fn disabling_security_disables_the_registered_redact_execute_route()
    -> Result<(), Box<dyn std::error::Error>> {
        // `redact` and `auto-redact` were both in the Security group while `redact-execute` — the
        // route that performs the redaction the manual tool has only previewed — was in no group,
        // so an administrator who disabled Security still had a live redaction endpoint. Assert
        // on the URI, the same input the axum middleware feeds `is_endpoint_enabled_for_uri`.
        const REDACT_EXECUTE_URI: &str = "/api/v1/security/redact-execute";
        assert_eq!(
            endpoint_key_for_uri(REDACT_EXECUTE_URI),
            Some("redact-execute".to_owned())
        );
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "endpoints:\n  groupsToRemove: [Security]\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        for uri in [
            REDACT_EXECUTE_URI,
            "/api/v1/security/redact",
            "/api/v1/security/auto-redact",
        ] {
            assert!(
                !config.is_endpoint_enabled_for_uri(uri),
                "disabling Security must disable {uri}"
            );
        }
        assert_eq!(
            config.disabled_endpoint_statuses().get("redact-execute"),
            Some(&false),
            "a disabled redact-execute must also be reported by get-endpoints-status"
        );

        // Disabling a different functional group must leave it alone.
        fs::write(&settings, "endpoints:\n  groupsToRemove: [PageOps]\n")?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        assert!(config.is_endpoint_enabled_for_uri(REDACT_EXECUTE_URI));
        Ok(())
    }

    #[test]
    fn a_default_configuration_leaves_every_registered_route_enabled()
    -> Result<(), Box<dyn std::error::Error>> {
        // Adding a key to ENDPOINT_GROUPS only makes an endpoint *disableable*; nothing is
        // disabled until an administrator names a group or an endpoint. Walk every registered
        // route through the exact call the middleware makes and prove that.
        let routes = registered_routes()?;
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        for (key, paths) in &routes {
            for path in paths {
                // `url-to-pdf` is the single route that ships disabled, gated by
                // `system.enableUrlToPDF` rather than by any group; asserted separately below.
                if key == "url-to-pdf" {
                    continue;
                }
                assert!(
                    config.is_endpoint_enabled_for_uri(path),
                    "{path} ({key}) must stay enabled on a default configuration"
                );
                assert!(
                    config.endpoint_availability(std::slice::from_ref(key))[key].enabled,
                    "{key} must report enabled on a default configuration"
                );
            }
        }
        assert!(!config.is_endpoint_enabled_for_uri("/api/v1/convert/url/pdf"));

        // With that one opt-in turned on, no registered route is disabled at all.
        fs::write(&settings, "system:\n  enableUrlToPDF: true\n")?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        for (key, paths) in &routes {
            for path in paths {
                assert!(
                    config.is_endpoint_enabled_for_uri(path),
                    "{path} ({key}) must stay enabled on a default configuration"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn every_processing_route_is_reachable_from_some_functional_group()
    -> Result<(), Box<dyn std::error::Error>> {
        // Infrastructure the SPA needs in order to work at all. An administrator who could gate
        // these would brick the UI rather than disable a tool, so they are deliberately in no
        // group: config/settings lookups (including the availability map itself), the info
        // metrics, the UI-data payloads, job and file plumbing, and the mobile-scanner session
        // handshake. `health` covers `/api/v1/info/health` and `/api/v1/ai/health` alike, since
        // both derive the same key.
        //
        // `stats`, `queue` and `cleanup` are the job-observation routes
        // (`/api/v1/jobs/stats`, `/api/v1/jobs/queue/stats`, `/api/v1/jobs/cleanup`). They
        // report on and expire the job plumbing that `job` itself exposes, so gating them
        // apart from `job` would leave the SPA able to submit work it cannot observe. Per
        // `contracts/job-stats.md` they are part of the open route set; `cleanup` removes only
        // jobs already past the 30-minute retention window that the background sweeper enforces
        // anyway, so an anonymous caller cannot destroy live work with it.
        const INFRASTRUCTURE_ROUTES: &[&str] = &[
            "app-config",
            "cleanup",
            "create-session",
            "download",
            "endpoint-enabled",
            "endpoints-availability",
            "endpoints-enabled",
            "files",
            "footer-info",
            "get-endpoints-status",
            "group-enabled",
            "health",
            "home",
            "job",
            "licenses",
            "load",
            "login-disclaimer",
            "queue",
            "requests",
            "session",
            "stats",
            "status",
            "upload",
            "uptime",
            "validate-session",
            "wau",
        ];
        // Routes whose own administrator switch is strictly stronger than group gating, so a
        // group entry would add nothing and would surprise whoever disabled that group to switch
        // off PDF tools. `send-email` is not registered at all unless `mail.enabled` yields an
        // SMTP service (`processing_routes_with_mail`); the AI proxies — `orchestrate`, `pdf`,
        // and every `/api/v1/ai/tools/*` route, which all derive `tools` — answer `503` unless
        // AIENGINE_ENABLED is set (`ai_proxy::proxy_request`).
        const ROUTES_WITH_THEIR_OWN_SWITCH: &[&str] =
            &["orchestrate", "pdf", "send-email", "tools"];

        let routes = registered_routes()?;
        let group_keys = group_keys();
        let ungated: BTreeSet<&str> = routes
            .keys()
            .map(String::as_str)
            .filter(|key| !group_keys.contains(key))
            .collect();
        let expected: BTreeSet<&str> = INFRASTRUCTURE_ROUTES
            .iter()
            .chain(ROUTES_WITH_THEIR_OWN_SWITCH)
            .copied()
            .collect();
        assert_eq!(
            expected.len(),
            INFRASTRUCTURE_ROUTES.len() + ROUTES_WITH_THEIR_OWN_SWITCH.len(),
            "the two lists must stay disjoint and duplicate-free"
        );
        for key in &expected {
            assert!(
                routes.contains_key(*key),
                "{key} is listed as an ungated route but lib.rs registers no route yielding it"
            );
        }
        assert_eq!(
            ungated, expected,
            "the set of routes that no functional group can disable changed. A route missing from \
             the right-hand side is in no group: pick the group matching what it does and add its \
             derived key to ENDPOINT_GROUPS, or list it above with a reason."
        );
        Ok(())
    }

    #[test]
    fn ai_engine_push_settings_carry_java_defaults_and_yaml_overrides()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let push = config.ai_engine_push_settings();
        assert!(!push.enabled);
        assert!(push.push_config_to_engine);
        assert_eq!(push.models.provider, "anthropic");
        assert_eq!(push.models.smart_model, "claude-haiku-4-5");
        assert_eq!(push.models.fast_model, "claude-haiku-4-5");
        assert_eq!(push.models.smart_max_tokens, 8_192);
        assert_eq!(push.models.fast_max_tokens, 2_048);
        assert_eq!(push.models.api_key, "");
        assert_eq!(push.models.base_url, "");
        assert_eq!(push.limits.max_pages, 200);
        assert_eq!(push.limits.max_characters, 200_000);
        assert_eq!(push.limits.model_max_concurrency, 32);

        fs::write(
            &settings,
            "aiEngine:\n  enabled: true\n  pushConfigToEngine: false\n  models:\n    provider: ollama\n    smartModel: qwen3\n    apiKey: sk-yaml\n  limits:\n    maxPages: 42\n",
        )?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let push = config.ai_engine_push_settings();
        assert!(push.enabled);
        assert!(!push.push_config_to_engine);
        assert_eq!(push.models.provider, "ollama");
        assert_eq!(push.models.smart_model, "qwen3");
        // Unset keys inside a configured section keep their defaults.
        assert_eq!(push.models.fast_model, "claude-haiku-4-5");
        assert_eq!(push.models.api_key, "sk-yaml");
        assert_eq!(push.limits.max_pages, 42);
        Ok(())
    }

    #[test]
    fn generated_identity_persists_once_and_is_stable_across_boots()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("configs").join("settings.yml");
        fs::create_dir_all(settings.parent().ok_or("parent")?)?;
        // Template-shaped file: placeholder identity plus an unrelated section
        // that must survive the rewrite.
        fs::write(
            &settings,
            "system:\n  defaultLocale: vi-VN\nAutomaticallyGenerated:\n  key: example\n  UUID: example\n  appVersion: 0.35.0\n",
        )?;

        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let identity = config.initialize_generated_identity()?;
        assert!(super::is_valid_settings_uuid(&identity.uuid));
        assert!(super::is_valid_settings_uuid(&identity.key));
        assert_ne!(identity.uuid, identity.key);
        assert_eq!(
            identity.app_version,
            crate::runtime_metrics::application_version()
        );
        // Any pre-existing non-placeholder version means this is not a
        // brand-new server.
        assert!(!identity.is_new_server);

        let persisted: serde_json::Value = serde_yaml::from_str(&fs::read_to_string(&settings)?)?;
        assert_eq!(
            persisted["AutomaticallyGenerated"]["UUID"],
            json!(identity.uuid)
        );
        assert_eq!(
            persisted["AutomaticallyGenerated"]["key"],
            json!(identity.key)
        );
        assert_eq!(
            persisted["AutomaticallyGenerated"]["appVersion"],
            json!(identity.app_version)
        );
        assert_eq!(persisted["system"]["defaultLocale"], json!("vi-VN"));

        // Second boot: same identity, byte-identical file (idempotent — the
        // template merge and repeated boots never churn the settings file).
        let before = fs::read_to_string(&settings)?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let second = config.initialize_generated_identity()?;
        assert_eq!(second.uuid, identity.uuid);
        assert_eq!(second.key, identity.key);
        assert!(!second.is_new_server);
        assert_eq!(fs::read_to_string(&settings)?, before);
        Ok(())
    }

    /// Regression for the every-boot identity reroll: desktop startup always
    /// creates a zero-byte `configs/custom_settings.yml` next to
    /// `settings.yml`. An empty document parses to YAML `null`, and merging it
    /// used to replace the ENTIRE settings snapshot with `Null` — blanking
    /// every configured value and regenerating key/UUID
    /// (`is_new_server=true`) on every single boot. An empty (or
    /// comments-only) custom settings document must merge as a no-op.
    #[test]
    fn generated_identity_survives_an_empty_custom_settings_sibling()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let config_directory = directory.path().join("configs");
        fs::create_dir_all(&config_directory)?;
        let settings = config_directory.join("settings.yml");
        let persisted = format!(
            "system:\n  defaultLocale: vi-VN\nAutomaticallyGenerated:\n  key: 123e4567-e89b-12d3-a456-426614174000\n  UUID: 223e4567-e89b-12d3-a456-426614174000\n  appVersion: {}\n",
            crate::runtime_metrics::application_version()
        );
        fs::write(&settings, &persisted)?;
        let custom_settings = config_directory.join("custom_settings.yml");

        for custom_contents in ["", "\n  \n", "# comments only\n"] {
            fs::write(&custom_settings, custom_contents)?;
            let config = RuntimeConfig::from_files(&settings, &custom_settings);
            assert_eq!(config.load_error, None);
            // The empty overlay must not blank the snapshot: every
            // settings.yml-backed value stays readable.
            assert_eq!(
                super::value_at(&config.settings, &["system", "defaultLocale"]),
                Some(&json!("vi-VN"))
            );
            let identity = config.initialize_generated_identity()?;
            assert_eq!(identity.uuid, "223e4567-e89b-12d3-a456-426614174000");
            assert_eq!(identity.key, "123e4567-e89b-12d3-a456-426614174000");
            assert!(!identity.is_new_server);
            // Reboot idempotence: nothing to rewrite, the file stays
            // byte-stable.
            assert_eq!(fs::read_to_string(&settings)?, persisted);
        }
        Ok(())
    }

    /// Regression for the comment-destruction defect: the identity write must
    /// preserve every non-identity byte of the settings file — banner comments,
    /// inline comments, blank lines, unrelated sections — changing ONLY the
    /// three `AutomaticallyGenerated` value lines.
    #[test]
    fn generated_identity_write_preserves_comments_and_every_other_byte()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        let original = "# banner line one\n# banner line two\nui:\n  logoStyle: classic # keep me\n\n# Automatically Generated Settings (Do Not Edit Directly)\nAutomaticallyGenerated:\n  key: example # inline key comment\n  UUID: example\n  appVersion: 0.35.0\n\nsystem:\n  defaultLocale: en-GB # locale comment\n";
        fs::write(&settings, original)?;

        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let identity = config.initialize_generated_identity()?;

        let expected = original
            .replace(
                "  key: example # inline key comment",
                &format!("  key: {} # inline key comment", identity.key),
            )
            .replace("  UUID: example", &format!("  UUID: {}", identity.uuid))
            .replace(
                "  appVersion: 0.35.0",
                &format!("  appVersion: {}", identity.app_version),
            );
        assert_eq!(fs::read_to_string(&settings)?, expected);
        Ok(())
    }

    /// The SPA names the running build from the public app config alone (the
    /// sidebar version line), so `appVersion` has to be present there even for
    /// an install whose `settings.yml` carries no generated identity yet, and
    /// it has to be the application version rather than the crate version.
    #[test]
    fn app_config_reports_the_application_version() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "system:\n  defaultLocale: en-GB\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));

        let expected_version = crate::runtime_metrics::application_version();
        assert_ne!(expected_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            config.app_config(None, None)["appVersion"],
            expected_version
        );
        Ok(())
    }

    /// Regression for the wrong-version-source defect: the persisted
    /// `appVersion` is the canonical application version, never the crate
    /// version.
    #[test]
    fn generated_identity_persists_the_application_version_not_the_crate_version()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        // Valid persisted identity, so only the version line changes.
        fs::write(
            &settings,
            "AutomaticallyGenerated:\n  key: 123e4567-e89b-12d3-a456-426614174000\n  UUID: 223e4567-e89b-12d3-a456-426614174000\n  appVersion: 0.35.0\n",
        )?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let identity = config.initialize_generated_identity()?;

        let expected_version = crate::runtime_metrics::application_version();
        // Guard that this assertion is meaningful: the crate version differs
        // from the application version, so writing the wrong source would fail.
        assert_ne!(expected_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(identity.app_version, expected_version);
        let contents = fs::read_to_string(&settings)?;
        assert!(
            contents.contains(&format!("  appVersion: {expected_version}")),
            "{contents}"
        );
        assert!(!contents.contains(env!("CARGO_PKG_VERSION")), "{contents}");
        Ok(())
    }

    #[test]
    fn generated_identity_marks_new_servers_and_creates_missing_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("configs").join("settings.yml");
        // No settings file at all: identity is generated and the file created.
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let identity = config.initialize_generated_identity()?;
        assert!(identity.is_new_server);
        let persisted: serde_json::Value = serde_yaml::from_str(&fs::read_to_string(&settings)?)?;
        assert_eq!(
            persisted["AutomaticallyGenerated"]["UUID"],
            json!(identity.uuid)
        );

        // A '0.0.0' placeholder version also marks a new server.
        let placeholder = directory.path().join("placeholder.yml");
        fs::write(
            &placeholder,
            "AutomaticallyGenerated:\n  appVersion: 0.0.0\n",
        )?;
        let config = RuntimeConfig::from_files(&placeholder, directory.path().join("missing.yml"));
        assert!(config.initialize_generated_identity()?.is_new_server);
        Ok(())
    }

    #[test]
    fn generated_identity_reuses_a_lowercase_section_spelling()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "automaticallyGenerated:\n  UUID: 123e4567-e89b-12d3-a456-426614174000\n",
        )?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let identity = config.initialize_generated_identity()?;
        // The valid existing UUID is kept even under the relaxed spelling.
        assert_eq!(identity.uuid, "123e4567-e89b-12d3-a456-426614174000");
        let contents = fs::read_to_string(&settings)?;
        // The writer reuses the existing section instead of duplicating it.
        assert!(!contents.contains("AutomaticallyGenerated:"));
        let persisted: serde_json::Value = serde_yaml::from_str(&contents)?;
        assert_eq!(
            persisted["automaticallyGenerated"]["UUID"],
            json!("123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(
            persisted["automaticallyGenerated"]["key"],
            json!(identity.key)
        );
        Ok(())
    }

    // TESTER: hostile settings shapes must never panic or corrupt sibling
    // sections — a malformed `AutomaticallyGenerated` node is repaired, and a
    // non-mapping document fails cleanly so the caller stays fail-open.
    #[test]
    fn generated_identity_survives_hostile_settings_shapes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;

        // Section present but a scalar: repaired into a mapping, siblings kept.
        let scalar_section = directory.path().join("scalar.yml");
        fs::write(
            &scalar_section,
            "system:\n  defaultLocale: en-GB\nAutomaticallyGenerated: 42\n",
        )?;
        let config =
            RuntimeConfig::from_files(&scalar_section, directory.path().join("missing.yml"));
        let identity = config.initialize_generated_identity()?;
        assert!(super::is_valid_settings_uuid(&identity.uuid));
        let persisted: serde_json::Value =
            serde_yaml::from_str(&fs::read_to_string(&scalar_section)?)?;
        assert_eq!(
            persisted["AutomaticallyGenerated"]["UUID"],
            json!(identity.uuid)
        );
        assert_eq!(persisted["system"]["defaultLocale"], json!("en-GB"));

        // Whole document is not a mapping: a clean error, no partial write.
        let sequence_root = directory.path().join("sequence.yml");
        fs::write(&sequence_root, "- not\n- a\n- mapping\n")?;
        let before = fs::read_to_string(&sequence_root)?;
        let config =
            RuntimeConfig::from_files(&sequence_root, directory.path().join("missing.yml"));
        assert!(config.initialize_generated_identity().is_err());
        assert_eq!(fs::read_to_string(&sequence_root)?, before);

        // Root is a FLOW mapping: it passes the is-a-mapping pre-check, but
        // the comment-preserving editor has no block structure to extend —
        // appending a section would corrupt the file into two YAML documents.
        // The write must refuse cleanly and leave every byte untouched.
        let flow_root = directory.path().join("flow.yml");
        fs::write(&flow_root, "{system: {defaultLocale: en-GB}}\n")?;
        let before = fs::read_to_string(&flow_root)?;
        let config = RuntimeConfig::from_files(&flow_root, directory.path().join("missing.yml"));
        assert!(config.initialize_generated_identity().is_err());
        assert_eq!(fs::read_to_string(&flow_root)?, before);

        // The generated section holds a block SEQUENCE: it also passes the
        // is-a-mapping pre-check, but mapping keys cannot join `- item`
        // children — the writer used to append `UUID:`/`key:` lines after the
        // items, writing UNPARSEABLE YAML while reporting Ok. It must refuse
        // cleanly with every byte untouched.
        let sequence_section = directory.path().join("sequence-section.yml");
        fs::write(&sequence_section, "automaticallyGenerated:\n  - 1\n  - 2\n")?;
        let before = fs::read_to_string(&sequence_section)?;
        let config =
            RuntimeConfig::from_files(&sequence_section, directory.path().join("missing.yml"));
        assert!(config.initialize_generated_identity().is_err());
        assert_eq!(fs::read_to_string(&sequence_section)?, before);

        // A UUID leaf holding a block scalar: rewriting only the `|`
        // indicator would fold the continuation lines into the new value
        // (valid YAML, wrong data), so the write refuses untouched.
        let block_scalar_leaf = directory.path().join("block-scalar.yml");
        fs::write(
            &block_scalar_leaf,
            "AutomaticallyGenerated:\n  UUID: |\n    junk\n",
        )?;
        let before = fs::read_to_string(&block_scalar_leaf)?;
        let config =
            RuntimeConfig::from_files(&block_scalar_leaf, directory.path().join("missing.yml"));
        assert!(config.initialize_generated_identity().is_err());
        assert_eq!(fs::read_to_string(&block_scalar_leaf)?, before);
        Ok(())
    }

    #[test]
    fn settings_uuid_validation_requires_canonical_shapes() {
        assert!(super::is_valid_settings_uuid(
            "123e4567-e89b-12d3-a456-426614174000"
        ));
        for invalid in [
            "",
            "example",
            "1-1-1-1-1",
            "123e4567e89b12d3a456426614174000",
            "123e4567-e89b-12d3-a456",
            "123e4567-e89b-12d3-a456-42661417400g",
            "123e4567-e89b-12d3-a456-426614174000-extra",
        ] {
            assert!(!super::is_valid_settings_uuid(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn product_rooted_keys_use_the_rustling_root() {
        let settings = json!({"rustling": {"jobResultExpiryMinutes": 5}});
        assert_eq!(
            super::product_value_at(&settings, &["jobResultExpiryMinutes"])
                .and_then(serde_json::Value::as_u64),
            Some(5),
            "rustling.* must be the canonical product settings root"
        );
        assert!(super::product_value_at(&json!({}), &["jobResultExpiryMinutes"]).is_none());
    }
}
