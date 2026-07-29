//! Public compatibility configuration for the Rust HTTP service.
//!
//! The Java application loads `configs/settings.yml` and then
//! `configs/custom_settings.yml` below `RUSTLING_BASE_PATH`; the latter overrides
//! the former. This module mirrors the public runtime configuration surface and the
//! anonymous analytics-onboarding mutation. Authentication and administrator mutation remain separate
//! migration tracks and are intentionally not claimed here.

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
use crate::license::LicenseConfig;
use crate::runtime_dependencies::discover_dependencies;

// Retained functional groups and real Rust dependency groups. Values are whitespace-separated
// endpoint keys to keep the compatibility table readable.
const ENDPOINT_GROUPS: &[(&str, &str)] = &[
    (
        "PageOps",
        "remove-pages merge-pdfs split-pages rearrange-pages rotate-pdf multi-page-layout booklet-imposition scale-pages crop pdf-to-single-page auto-split-pdf split-by-size-or-count overlay-pdf split-pdf-by-sections split-pdf-by-chapters add-page-numbers extract-pages",
    ),
    (
        "Convert",
        "pdf-to-img img-to-pdf pdf-to-pdfa file-to-pdf pdf-to-word pdf-to-presentation pdf-to-text pdf-to-html pdf-to-xml html-to-pdf url-to-pdf markdown-to-pdf pdf-to-csv pdf-to-markdown eml-to-pdf pdf-to-epub ebook-to-pdf pdf-to-vector vector-to-pdf pdf-to-video cbz-to-pdf cbr-to-pdf pdf-to-cbz pdf-to-cbr pdf-to-json json-to-pdf pdf-to-rtf",
    ),
    (
        "Security",
        "add-password remove-password change-permissions add-watermark cert-sign remove-cert-sign sanitize-pdf timestamp-pdf auto-redact validate-signature add-stamp unlock-pdf-forms redact verify-pdf sign",
    ),
    (
        "Other",
        "ocr-pdf extract-images update-metadata flatten remove-blanks remove-annotations get-info-on-pdf add-attachments replace-invert-pdf edit-table-of-contents text-editor-pdf add-image compare view-pdf multi-tool fields modify-fields delete-fields fill",
    ),
    (
        "Advance",
        "compress-pdf extract-image-scans repair auto-rename scanner-effect overlay-pdf adjust-contrast",
    ),
    ("Automation", "handleData automate pipeline"),
    ("DeveloperTools", "show-javascript"),
    (
        "DeveloperDocs",
        "dev-api-docs dev-folder-scanning-docs dev-sso-guide-docs dev-airgapped-docs",
    ),
    (
        "LibreOffice",
        "file-to-pdf pdf-to-word pdf-to-presentation pdf-to-rtf pdf-to-xml",
    ),
    ("Ghostscript", "pdf-to-pdfa pdf-to-vector vector-to-pdf"),
    ("tesseract", "ocr-pdf"),
    ("OCRmyPDF", "ocr-pdf"),
    ("rar", "pdf-to-cbr"),
    ("Weasyprint", "html-to-pdf url-to-pdf markdown-to-pdf eml-to-pdf"),
    ("Pdftohtml", "pdf-to-html"),
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
    pub(crate) ghostscript_session_limit: usize,
    pub(crate) ghostscript_timeout: Duration,
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
        let base_path = crate::env_compat::var_os("RUSTLING_BASE_PATH")
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

    /// Resolves the legacy `mail.*` SMTP relay settings without opening a
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
    /// The environment names mirror Spring's relaxed binding for
    /// `aiEngine.*`; YAML values keep the same compatibility when no override
    /// is set.
    #[must_use]
    pub fn ai_engine_settings(&self) -> (bool, String, u64) {
        let enabled = env_bool("AIENGINE_ENABLED")
            .or_else(|| env_bool("RUSTLING_AI_ENGINE_ENABLED"))
            .or_else(|| value_at(&self.settings, &["aiEngine", "enabled"]).and_then(yaml_bool))
            .unwrap_or(false);
        let url = crate::env_compat::var("AIENGINE_URL")
            .ok()
            .or_else(|| crate::env_compat::var("RUSTLING_AI_ENGINE_URL").ok())
            .or_else(|| {
                value_at(&self.settings, &["aiEngine", "url"])
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "http://localhost:5001".to_owned());
        let timeout_seconds = crate::env_compat::var("AIENGINE_TIMEOUTSECONDS")
            .ok()
            .or_else(|| crate::env_compat::var("AIENGINE_TIMEOUT_SECONDS").ok())
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
        let seconds = crate::env_compat::var("AIENGINE_LONGRUNNINGTIMEOUTSECONDS")
            .ok()
            .or_else(|| crate::env_compat::var("AIENGINE_LONG_RUNNING_TIMEOUT_SECONDS").ok())
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
    /// engine's `POST /api/v1/config` on startup and after admin saves.
    ///
    /// Environment names mirror Spring's relaxed binding for `aiEngine.*`
    /// (e.g. `AIENGINE_MODELS_SMARTMODEL`); YAML values keep the same
    /// compatibility when no override is set, and defaults match Java's
    /// `ApplicationProperties.AiEngine`.
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
        let milliseconds = crate::env_compat::var("RUSTLING_AI_STREAMTIMEOUTMS")
            .ok()
            .or_else(|| crate::env_compat::var("RUSTLING_AI_STREAM_TIMEOUT_MS").ok())
            .and_then(|value| value.parse().ok())
            .or_else(|| {
                product_value_at(&self.settings, &["ai", "streamTimeoutMs"]).and_then(Value::as_u64)
            })
            .unwrap_or(1_800_000)
            .max(1);
        Duration::from_millis(milliseconds)
    }

    /// Resolves bounded asynchronous job admission. Values mirror the Java
    /// queue property names while adding an explicit weighted execution budget
    /// for the Rust scheduler.
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

    /// Resolves the Java-compatible premium license settings, including the
    /// temporary `enterpriseEdition` migration fallback.
    #[must_use]
    pub(crate) fn license_config(&self) -> LicenseConfig {
        self.license_config_with_environment(|name| crate::env_compat::var(name).ok())
    }

    fn license_config_with_environment(
        &self,
        environment: impl Fn(&str) -> Option<String>,
    ) -> LicenseConfig {
        const EMPTY_KEY: &str = "00000000-0000-0000-0000-000000000000";
        let configured_bool = |path: &[&str], name: &str| {
            environment(name)
                .as_deref()
                .and_then(parse_boolean)
                .or_else(|| value_at(&self.settings, path).and_then(yaml_bool))
                .unwrap_or(false)
        };
        let configured_string = |path: &[&str], name: &str, default: &str| {
            environment(name)
                .or_else(|| {
                    value_at(&self.settings, path)
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| default.to_owned())
        };
        let premium_enabled = configured_bool(&["premium", "enabled"], "PREMIUM_ENABLED");
        let legacy_enabled = configured_bool(
            &["enterpriseEdition", "enabled"],
            "ENTERPRISEEDITION_ENABLED",
        );
        let mut key = configured_string(&["premium", "key"], "PREMIUM_KEY", EMPTY_KEY);
        if key == EMPTY_KEY {
            let legacy_key = configured_string(
                &["enterpriseEdition", "key"],
                "ENTERPRISEEDITION_KEY",
                EMPTY_KEY,
            );
            if legacy_key != EMPTY_KEY {
                key = legacy_key;
            }
        }
        let initial_max_users = environment("PREMIUM_MAXUSERS")
            .and_then(|value| value.parse::<i64>().ok())
            .or_else(|| value_at(&self.settings, &["premium", "maxUsers"]).and_then(Value::as_i64))
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(0);
        LicenseConfig {
            enabled: premium_enabled || legacy_enabled,
            key: Zeroizing::new(key),
            initial_max_users,
        }
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

    /// Returns the Tesseract language-data directory using Java's precedence:
    /// explicit settings, `TESSDATA_PREFIX`, then the packaged Linux default.
    #[must_use]
    pub fn tessdata_dir(&self) -> PathBuf {
        let configured = self.string(&["system", "tessdataDir"], "SYSTEM_TESSDATADIR", "");
        if !configured.trim().is_empty() {
            return PathBuf::from(configured);
        }
        crate::env_compat::var_os("TESSDATA_PREFIX")
            .filter(|value| !value.is_empty())
            .map_or_else(
                || PathBuf::from("/usr/share/tesseract-ocr/5/tessdata"),
                PathBuf::from,
            )
    }

    /// Returns the maximum page-rendering DPI used by Java's OCR fallback.
    #[must_use]
    pub fn max_render_dpi(&self) -> i32 {
        let configured = self.signed_integer(&["system", "maxDPI"], "SYSTEM_MAXDPI", 500);
        i32::try_from(configured.clamp(1, i64::from(i32::MAX))).unwrap_or(500)
    }

    /// Returns the two Java `ProcessExecutor` pools used by the OCR controller.
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

    /// Returns the Java `ProcessExecutor` pools used by the repair controller.
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
        let ghostscript_session_limit = positive(
            &["processExecutor", "sessionLimit", "ghostscriptSessionLimit"],
            "PROCESS_EXECUTOR_SESSION_LIMIT_GHOSTSCRIPT_SESSION_LIMIT",
            8,
        );
        let qpdf_timeout_minutes = positive(
            &["processExecutor", "timeoutMinutes", "qpdfTimeoutMinutes"],
            "PROCESS_EXECUTOR_TIMEOUT_MINUTES_QPDF_TIMEOUT_MINUTES",
            30,
        );
        let ghostscript_timeout_minutes = positive(
            &[
                "processExecutor",
                "timeoutMinutes",
                "ghostscriptTimeoutMinutes",
            ],
            "PROCESS_EXECUTOR_TIMEOUT_MINUTES_GHOSTSCRIPT_TIMEOUT_MINUTES",
            30,
        );
        RepairProcessSettings {
            qpdf_session_limit: usize::try_from(qpdf_session_limit).unwrap_or(2),
            qpdf_timeout: Duration::from_secs(qpdf_timeout_minutes.saturating_mul(60)),
            ghostscript_session_limit: usize::try_from(ghostscript_session_limit).unwrap_or(8),
            ghostscript_timeout: Duration::from_secs(
                ghostscript_timeout_minutes.saturating_mul(60),
            ),
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
    /// (`customFiles/static/`), the Rust spelling of upstream
    /// `InstallationPathConfig.getStaticPath()`.
    #[must_use]
    pub(crate) fn custom_static_dir(&self) -> PathBuf {
        self.custom_files_dir.join("static")
    }

    /// Returns the built SPA `dist/` directory to serve from the binary, when
    /// single-binary UI serving is enabled via `RUSTLING_FRONTEND_DIST` (env)
    /// or `system.frontendDist` (settings). Upstream has no equivalent
    /// property: the Java build bakes the dist onto the servlet classpath, so
    /// this key is owned by the Rust runtime. Unset means SPA serving stays
    /// fully disabled (the Vite dev-proxy workflow).
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

    /// Ensures the persisted install identity, mirroring Java `InitialSetup`:
    /// on first boot a random UUID and machine key are generated and written
    /// into `AutomaticallyGenerated.*` in the settings file, and the current
    /// application version is persisted (its previous absence — empty or the
    /// `0.0.0` placeholder — marks a new server). Identity supplied through the
    /// environment (relaxed-binding `AUTOMATICALLYGENERATED_*`) is honored
    /// without being written back, exactly like Java's property binding.
    ///
    /// Unlike Java, an unchanged boot writes nothing (Java rewrites the same
    /// values every start); the at-rest result is identical and the settings
    /// file stays byte-stable, preserving template-merge idempotence.
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
        // Java persists the application version from `version.properties`; the
        // Rust equivalent is `application_version()` (backed by the repo VERSION
        // file), NOT the crate version.
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

    /// Logs a one-line warning for every notable legacy setting that the
    /// stateless, no-login runtime now ignores.
    ///
    /// The removal of authentication and server-side state left behind
    /// configuration keys in existing installs — the bundled desktop template
    /// even ships `security.enableLogin: true`. Those keys must be IGNORED,
    /// never refused: a hard failure would brick every existing install on
    /// upgrade. Only the keys worth an operator's attention warn; everything
    /// else is silently skipped.
    #[allow(clippy::too_many_lines)] // a flat checklist of legacy keys
    pub fn warn_on_ignored_legacy_settings(&self) {
        let yaml_present = |path: &[&str]| value_at(&self.settings, path).is_some();
        let yaml_true = |path: &[&str]| {
            value_at(&self.settings, path)
                .and_then(yaml_bool)
                .unwrap_or(false)
        };
        let env_present = |names: &[&str]| {
            names
                .iter()
                .any(|name| crate::env_compat::var_os(name).is_some())
        };
        let mut ignored: Vec<&str> = Vec::new();
        if yaml_true(&["security", "enableLogin"])
            || env_present(&["SECURITY_ENABLELOGIN", "SECURITY_ENABLE_LOGIN"])
        {
            ignored.push("security.enableLogin / SECURITY_ENABLELOGIN");
        }
        if env_present(&["DOCKER_ENABLE_SECURITY"]) {
            ignored.push("DOCKER_ENABLE_SECURITY");
        }
        if yaml_present(&["security", "initialLogin"])
            || env_present(&[
                "SECURITY_INITIALLOGIN_USERNAME",
                "SECURITY_INITIALLOGIN_PASSWORD",
            ])
        {
            ignored.push("security.initialLogin.*");
        }
        if yaml_present(&["security", "oauth2"]) || env_present(&["SECURITY_OAUTH2_ENABLED"]) {
            ignored.push("security.oauth2.*");
        }
        if yaml_present(&["security", "saml2"]) {
            ignored.push("security.saml2.*");
        }
        if yaml_present(&["security", "jwt"]) {
            ignored.push("security.jwt.*");
        }
        if yaml_present(&["security", "loginMethod"]) || env_present(&["SECURITY_LOGINMETHOD"]) {
            ignored.push("security.loginMethod");
        }
        if yaml_present(&["security", "databasePath"])
            || env_present(&[
                "RUSTLING_SECURITY_DATABASE_PATH",
                "STIRLING_SECURITY_DATABASE_PATH",
            ])
        {
            ignored.push("security.databasePath");
        }
        if yaml_present(&["security", "credentialEncryptionKey"])
            || yaml_present(&["security", "credentialEncryptionKeyPath"])
            || env_present(&[
                "RUSTLING_CREDENTIAL_ENCRYPTION_KEY",
                "RUSTLING_CREDENTIAL_ENCRYPTION_KEY_PATH",
                "STIRLING_CREDENTIAL_ENCRYPTION_KEY",
                "STIRLING_CREDENTIAL_ENCRYPTION_KEY_PATH",
            ])
        {
            ignored.push("security.credentialEncryptionKey[Path]");
        }
        if yaml_true(&["mcp", "enabled"]) || env_present(&["MCP_ENABLED", "MCP_AUTH_MODE"]) {
            ignored.push("mcp.*");
        }
        if yaml_true(&["storage", "enabled"]) || env_present(&["STORAGE_ENABLED"]) {
            ignored.push("storage.*");
        }
        if yaml_true(&["policies", "enabled"]) || env_present(&["POLICIES_ENABLED"]) {
            ignored.push("policies.*");
        }
        if yaml_present(&["premium", "enterpriseFeatures", "audit"]) {
            ignored.push("premium.enterpriseFeatures.audit.*");
        }
        if yaml_true(&["mail", "enableInvites"]) || env_present(&["MAIL_ENABLEINVITES"]) {
            ignored.push("mail.enableInvites");
        }
        // The document store / PDF question-answer feature was removed;
        // retrieval settings are no longer pushed to the AI engine.
        if yaml_present(&["aiEngine", "rag"])
            || env_present(&[
                "AIENGINE_RAG_EMBEDDINGPROVIDER",
                "AIENGINE_RAG_EMBEDDINGMODEL",
                "AIENGINE_RAG_EMBEDDINGAPIKEY",
                "AIENGINE_RAG_EMBEDDINGBASEURL",
                "AIENGINE_RAG_TOPK",
                "AIENGINE_RAG_MAXSEARCHES",
            ])
        {
            ignored.push("aiEngine.rag.*");
        }
        if yaml_present(&["app", "supabase"])
            || env_present(&[
                "SAAS_DB_PROJECT_REF",
                "RUSTLING_SUPABASE_ISSUER",
                "APP_SUPABASE_ISSUER",
            ])
        {
            ignored.push("app.supabase.*");
        }
        for key in ignored {
            tracing::warn!(
                key,
                "this setting belongs to a removed feature (login/auth, MCP, or server-side \
                 state) and is ignored; the server always runs in open, stateless mode"
            );
        }
    }

    fn generated_setting(&self, field: &str, environment: &str) -> String {
        crate::env_compat::var(environment)
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
        insert(
            config,
            "defaultLocale",
            self.string(&["system", "defaultLocale"], "SYSTEM_DEFAULTLOCALE", ""),
        );
        // The existing Rust binary has no authentication middleware yet. Reporting a configured
        // login flag here would make the unchanged UI render a login flow it cannot complete.
        insert(config, "enableLogin", false);
        insert(
            config,
            "showSettingsWhenNoLogin",
            self.boolean(
                &["system", "showSettingsWhenNoLogin"],
                "SYSTEM_SHOWSETTINGSWHENNOLOGIN",
                true,
            ),
        );
        insert(config, "enableEmailInvites", false);
        insert(config, "enableOAuth", false);
        insert(config, "enableSaml", false);
        insert(config, "isAdmin", false);
        insert(config, "isNewUser", false);
        insert(config, "isNewServer", false);
        insert(
            config,
            "shouldShowUpdate",
            self.boolean(&["system", "showUpdate"], "SYSTEM_SHOWUPDATE", true),
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
        insert(config, "enableAnalytics", self.analytics_enabled());
        insert(
            config,
            "enablePosthog",
            self.optional_boolean(&["system", "enablePosthog"], "SYSTEM_ENABLEPOSTHOG"),
        );
        insert(
            config,
            "enableScarf",
            self.optional_boolean(&["system", "enableScarf"], "SYSTEM_ENABLESCARF"),
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
        insert(config, "premiumEnabled", self.license_config().enabled);
        insert(config, "runningProOrHigher", false);
        insert(config, "runningEE", false);
        insert(config, "license", "NORMAL");
        insert(
            config,
            "aiEngineEnabled",
            self.boolean(&["aiEngine", "enabled"], "AIENGINE_ENABLED", false),
        );
        insert(config, "storageEnabled", false);
        insert(config, "storageSharingEnabled", false);
        insert(config, "storageShareLinksEnabled", false);
        insert(config, "storageShareEmailEnabled", false);
        insert(config, "storageGroupSigningEnabled", false);
        insert(config, "serverCertificateEnabled", false);
        insert(config, "hardwareSigningAvailable", false);
        insert(config, "activeSecurity", false);
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
                "https://www.stirling.com/legal/terms-of-service",
            ),
        );
        insert(
            config,
            "privacyPolicy",
            self.string(
                &["legal", "privacyPolicy"],
                "LEGAL_PRIVACYPOLICY",
                "https://www.stirling.com/legal/privacy-policy",
            ),
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
            return alternatives
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

        crate::env_compat::var("LEGAL_LOGINAGREEMENT_FALLBACKTEXT")
            .or_else(|_| crate::env_compat::var("LEGAL_LOGIN_AGREEMENT_FALLBACK_TEXT"))
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

    fn analytics_enabled(&self) -> Option<bool> {
        self.optional_boolean(&["system", "enableAnalytics"], "SYSTEM_ENABLEANALYTICS")
    }

    fn string(&self, path: &[&str], environment: &str, default: &str) -> String {
        crate::env_compat::var(environment)
            .ok()
            .or_else(|| {
                value_at(&self.settings, path)
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| default.to_owned())
    }

    fn strings(&self, path: &[&str], environment: &str) -> Vec<String> {
        if let Ok(value) = crate::env_compat::var(environment) {
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

    /// [`Self::u64`] against the product-rooted settings keys: `rustling.*`
    /// is the primary root and the pre-rename `stirling.*` root keeps working
    /// as a legacy alias (`rustling.*` wins when both are present).
    fn product_u64(&self, path_below_root: &[&str], environment: &str, default: u64) -> u64 {
        crate::env_compat::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .or_else(|| product_value_at(&self.settings, path_below_root).and_then(Value::as_u64))
            .unwrap_or(default)
    }

    fn u64(&self, path: &[&str], environment: &str, default: u64) -> u64 {
        crate::env_compat::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .or_else(|| value_at(&self.settings, path).and_then(Value::as_u64))
            .unwrap_or(default)
    }

    fn signed_integer(&self, path: &[&str], environment: &str, default: i64) -> i64 {
        crate::env_compat::var(environment)
            .ok()
            .and_then(|value| value.parse().ok())
            .or_else(|| value_at(&self.settings, path).and_then(Value::as_i64))
            .unwrap_or(default)
    }

    fn usize(environment: &str, default: usize) -> usize {
        crate::env_compat::var(environment)
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

// Java `ApplicationProperties.AiEngine` model identity defaults, shared with
// the config-push "was this section configured?" detection.
pub(crate) const DEFAULT_AI_MODEL_PROVIDER: &str = "anthropic";
pub(crate) const DEFAULT_AI_SMART_MODEL: &str = "claude-haiku-4-5";
pub(crate) const DEFAULT_AI_FAST_MODEL: &str = "claude-haiku-4-5";

/// Model + provider selection pushed to the engine (Java `AiEngine.Models`).
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

/// Request size / cost guardrails pushed to the engine (Java `AiEngine.Limits`).
#[derive(Clone, Debug)]
pub struct AiEngineLimitsPush {
    pub max_pages: i64,
    pub max_characters: i64,
    pub model_max_concurrency: i64,
}

/// The engine-relevant `aiEngine.*` configuration slice, with Java's built-in
/// defaults as the `Default` value.
#[derive(Clone, Debug)]
pub struct AiEnginePushSettings {
    pub enabled: bool,
    /// Whether the processor pushes settings-derived AI config to the engine on
    /// startup/save. Pinned false for env-driven deployments so the engine
    /// stays env-controlled (Java `aiEngine.pushConfigToEngine`).
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

/// Persisted install identity mirroring Java's
/// `ApplicationProperties.AutomaticallyGenerated` section.
#[derive(Clone, Debug)]
pub struct GeneratedIdentity {
    /// Stable installation UUID (`AutomaticallyGenerated.UUID`).
    pub uuid: String,
    /// Machine secret key (`AutomaticallyGenerated.key`).
    pub key: String,
    /// The version persisted for this boot (`AutomaticallyGenerated.appVersion`).
    pub app_version: String,
    /// Whether the settings file carried no prior version (Java
    /// `InitialSetup.isNewServer`): empty or the `0.0.0` placeholder.
    pub is_new_server: bool,
}

/// Read-modify-write of `section.key` values in `settings.yml` that preserves
/// every other byte of the file — comments, blank lines, key order and
/// formatting — by rewriting only the targeted value lines through the shared
/// comment-preserving editor ([`crate::settings_yaml`]). This matches Java's
/// writer (`GeneralUtils.saveKeyToSettings` via `YamlHelper`, whose snakeyaml
/// round-trips comments): a first boot rewrites just the value portion of the
/// template's `AutomaticallyGenerated` lines and keeps the banner and every
/// comment intact. An existing section or key whose spelling differs only by
/// ASCII case is reused rather than duplicated, matching Java's relaxed
/// binding reading either spelling back; keys the file lacks are inserted into
/// the section, and a missing section (or file) is created. Hostile
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

/// Whether a persisted identity value passes Java's `GeneralUtils.isValidUUID`
/// (`UUID.fromString`): five non-empty hyphen-separated hex groups within the
/// canonical length. Java additionally tolerates over-long groups up to a
/// signed-long overflow; those exotic spellings are regenerated once here into
/// canonical form and stay stable afterwards.
fn is_valid_settings_uuid(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 36 {
        return false;
    }
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && groups.iter().all(|group| {
            !group.is_empty()
                && group.len() <= 12
                && group.bytes().all(|byte| byte.is_ascii_hexdigit())
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
    // Java parity: Spring's YAML loader yields no properties for an empty
    // document, so `settings.yml` keeps full effect.
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

/// Product-rooted settings lookup: `rustling.*` is the primary key root and
/// the pre-rename `stirling.*` root is honoured as a legacy alias.
fn product_value_at<'a>(value: &'a Value, path_below_root: &[&str]) -> Option<&'a Value> {
    let lookup = |root: &'static str| {
        let mut path = Vec::with_capacity(path_below_root.len() + 1);
        path.push(root);
        path.extend_from_slice(path_below_root);
        value_at(value, &path)
    };
    lookup("rustling").or_else(|| lookup("stirling"))
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn env_bool(name: &str) -> Option<bool> {
    crate::env_compat::var(name)
        .ok()
        .and_then(|value| parse_boolean(&value))
}

/// Parses a configuration boolean with Spring's relaxed vocabulary.
///
/// Java binds environment and YAML values through spring-core's
/// `StringToBooleanConverter`, which accepts `true`/`on`/`yes`/`1` and
/// `false`/`off`/`no`/`0` (trimmed, case-insensitive). Anything narrower here
/// would make the Rust binary read the same deployment configuration
/// differently from Java — in the security guard's case, fail-open.
fn parse_boolean(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Reads a YAML setting as a boolean with Java's SnakeYAML/Spring semantics.
///
/// `serde_yaml` implements YAML 1.2, so unquoted `yes`/`on`/`no`/`off` arrive
/// here as *strings*, while `SnakeYAML` (YAML 1.1) hands Java a real `Boolean`.
/// Falling back to [`parse_boolean`] keeps `enableLogin: yes` and
/// `enabled: on` meaning the same thing in both runtimes; genuine YAML
/// booleans still take the direct path. An unquoted numeric `1`/`0` reaches
/// Java as an `Integer` that Spring's binder coerces truthily, so the numeric
/// arm keeps `enableLogin: 1` requesting secured mode instead of silently
/// reading as unset (which would fail open in the security guard).
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
    use std::{collections::BTreeMap, fs};

    use serde_json::json;
    use tempfile::tempdir;

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
    fn repair_process_limits_and_timeouts_match_java_defaults_and_yaml()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let defaults = config.repair_process_settings();
        assert_eq!(defaults.qpdf_session_limit, 2);
        assert_eq!(defaults.qpdf_timeout.as_secs(), 30 * 60);
        assert_eq!(defaults.ghostscript_session_limit, 8);
        assert_eq!(defaults.ghostscript_timeout.as_secs(), 30 * 60);

        fs::write(
            &settings,
            "processExecutor:\n  sessionLimit:\n    qpdfSessionLimit: 5\n    ghostscriptSessionLimit: 6\n  timeoutMinutes:\n    qpdfTimeoutMinutes: 11\n    ghostscriptTimeoutMinutes: 13\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let configured = config.repair_process_settings();
        assert_eq!(configured.qpdf_session_limit, 5);
        assert_eq!(configured.qpdf_timeout.as_secs(), 11 * 60);
        assert_eq!(configured.ghostscript_session_limit, 6);
        assert_eq!(configured.ghostscript_timeout.as_secs(), 13 * 60);
        Ok(())
    }

    #[test]
    fn dependency_commands_are_available_only_for_enabled_discovered_groups()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let command = directory.path().join("gs");
        config
            .dependency_commands
            .insert("Ghostscript".to_owned(), command.clone());
        assert_eq!(config.dependency_command("Ghostscript"), Some(command));

        config
            .dependency_disabled_groups
            .insert("Ghostscript".to_owned());
        assert_eq!(config.dependency_command("Ghostscript"), None);
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
    fn java_era_phantom_groups_are_inert_but_legacy_keys_remain_accepted()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "endpoints:\n  groupsToRemove: [Java, Python, OpenCV, ImageMagick, Javascript, CLI, Unoconvert]\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        for group in [
            "Java",
            "Python",
            "OpenCV",
            "ImageMagick",
            "Javascript",
            "CLI",
            "Unoconvert",
        ] {
            assert!(!config.is_group_enabled(group), "{group} must stay inert");
        }
        assert!(config.is_endpoint_enabled("merge-pdfs"));
        assert!(config.is_endpoint_enabled("pdf-to-markdown"));
        Ok(())
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
        assert!(availability["verify-pdf"].enabled);
        assert_eq!(availability["verify-pdf"].reason, None);
        Ok(())
    }

    #[test]
    fn weasyprint_dependency_disables_html_markdown_and_eml_to_pdf()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
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
    fn pdftohtml_is_not_a_dependency_of_pdf_to_markdown() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "{}\n")?;
        let mut config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        config
            .dependency_disabled_groups
            .insert("Pdftohtml".to_owned());
        assert!(!config.is_endpoint_enabled("pdf-to-html"));
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

    #[test]
    fn license_config_migrates_legacy_enterprise_settings_and_environment_overrides()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "premium:\n  enabled: false\n  key: 00000000-0000-0000-0000-000000000000\n  maxUsers: 6\nenterpriseEdition:\n  enabled: true\n  key: legacy-key\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let license = config.license_config_with_environment(|_| None);
        assert!(license.enabled);
        assert_eq!(license.key.as_str(), "legacy-key");
        assert_eq!(license.initial_max_users, 6);

        let environment = BTreeMap::from([
            ("PREMIUM_ENABLED", "true"),
            ("PREMIUM_KEY", "current-key"),
            ("PREMIUM_MAXUSERS", "13"),
        ]);
        let license = config.license_config_with_environment(|name| {
            environment.get(name).map(|value| (*value).to_owned())
        });
        assert!(license.enabled);
        assert_eq!(license.key.as_str(), "current-key");
        assert_eq!(license.initial_max_users, 13);
        Ok(())
    }

    #[test]
    fn app_config_defaults_to_unverified_normal_license_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(&settings, "premium:\n  enabled: true\n  key: opaque-key\n")?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        let app_config = config.app_config(None, None);
        assert_eq!(app_config["premiumEnabled"], true);
        assert_eq!(app_config["runningProOrHigher"], false);
        assert_eq!(app_config["runningEE"], false);
        assert_eq!(app_config["license"], "NORMAL");
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
        let mut base = json!({ "system": { "defaultLocale": "en-US", "showUpdate": true } });
        merge_json(&mut base, json!({ "system": { "defaultLocale": "vi-VN" } }));
        assert_eq!(
            base,
            json!({ "system": { "defaultLocale": "vi-VN", "showUpdate": true } })
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
            // Unquoted numerics reach Java as Integers that Spring coerces.
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
        // Malformed values fall back to the Java default (false here).
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
        let availability = config.endpoint_availability(&["file-to-pdf".to_owned()]);
        assert!(!availability["file-to-pdf"].enabled);
        assert_eq!(availability["file-to-pdf"].reason, Some("DEPENDENCY"));
        assert_eq!(config.app_config(None, None)["dependenciesReady"], true);
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
    fn group_configuration_disables_functional_and_fallback_tool_endpoints()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        fs::write(
            &settings,
            "endpoints:\n  groupsToRemove: [PageOps, qpdf, Ghostscript]\n",
        )?;
        let config = RuntimeConfig::from_files(settings, directory.path().join("missing.yml"));
        assert!(!config.is_group_enabled("PageOps"));
        assert!(!config.is_group_enabled("qpdf"));
        assert!(config.is_group_enabled("Convert"));
        assert!(!config.is_endpoint_enabled("merge-pdfs"));
        assert!(config.is_endpoint_enabled("repair"));
        assert!(!config.is_endpoint_enabled("pdf-to-pdfa"));
        assert!(config.is_endpoint_enabled("file-to-pdf"));
        let statuses = config.disabled_endpoint_statuses();
        assert_eq!(statuses.get("merge-pdfs"), Some(&false));
        assert!(!statuses.contains_key("repair"));
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

        // A legacy `aiEngine.rag` block from an existing install is ignored
        // (the retrieval pipeline was removed), never a startup failure.
        fs::write(
            &settings,
            "aiEngine:\n  enabled: true\n  pushConfigToEngine: false\n  models:\n    provider: ollama\n    smartModel: qwen3\n    apiKey: sk-yaml\n  rag:\n    topK: 7\n  limits:\n    maxPages: 42\n",
        )?;
        let config = RuntimeConfig::from_files(&settings, directory.path().join("missing.yml"));
        let push = config.ai_engine_push_settings();
        assert!(push.enabled);
        assert!(!push.push_config_to_engine);
        assert_eq!(push.models.provider, "ollama");
        assert_eq!(push.models.smart_model, "qwen3");
        // Unset keys inside a configured section keep their Java defaults.
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
        // Java parity: any pre-existing non-placeholder version (the template
        // ships one) means this is not a brand-new server.
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
    /// three `AutomaticallyGenerated` value lines (Java's snakeyaml writer
    /// keeps comments via `parseComments`/`dumpComments`).
    #[test]
    fn generated_identity_write_preserves_comments_and_every_other_byte()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let settings = directory.path().join("settings.yml");
        let original = "# banner line one\n# banner line two\nsecurity:\n  enableLogin: false # keep me\n\n# Automatically Generated Settings (Do Not Edit Directly)\nAutomaticallyGenerated:\n  key: example # inline key comment\n  UUID: example\n  appVersion: 0.35.0\n\nsystem:\n  defaultLocale: en-GB # locale comment\n";
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

    /// Regression for the wrong-version-source defect: the persisted
    /// `appVersion` is the canonical application version (Java writes its
    /// `version.properties` version), never the Rust crate version.
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

        // A '0.0.0' placeholder version also marks a new server (Java rule).
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
    fn settings_uuid_validation_matches_java_shapes() {
        assert!(super::is_valid_settings_uuid(
            "123e4567-e89b-12d3-a456-426614174000"
        ));
        // Java's UUID.fromString accepts short hex groups.
        assert!(super::is_valid_settings_uuid("1-1-1-1-1"));
        for invalid in [
            "",
            "example",
            "123e4567e89b12d3a456426614174000",
            "123e4567-e89b-12d3-a456",
            "123e4567-e89b-12d3-a456-42661417400g",
            "123e4567-e89b-12d3-a456-426614174000-extra",
        ] {
            assert!(!super::is_valid_settings_uuid(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn product_rooted_keys_prefer_rustling_and_accept_legacy_stirling() {
        let both = json!({
            "rustling": {"jobResultExpiryMinutes": 5},
            "stirling": {"jobResultExpiryMinutes": 9}
        });
        assert_eq!(
            super::product_value_at(&both, &["jobResultExpiryMinutes"])
                .and_then(serde_json::Value::as_u64),
            Some(5),
            "rustling.* must win when both roots are present"
        );
        let legacy = json!({"stirling": {"job": {"queue": {"baseCapacity": 7}}}});
        assert_eq!(
            super::product_value_at(&legacy, &["job", "queue", "baseCapacity"])
                .and_then(serde_json::Value::as_u64),
            Some(7),
            "pre-rename stirling.* keys must keep working"
        );
        assert!(super::product_value_at(&json!({}), &["jobResultExpiryMinutes"]).is_none());
    }
}
