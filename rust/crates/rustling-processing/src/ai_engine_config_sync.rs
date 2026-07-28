//! Best-effort push of admin-configured AI settings to the engine.
//!
//! Port of Java `AiEngineConfigSync`: the processor pushes the engine-relevant
//! `aiEngine.*` configuration (models, limits) to the engine's
//! `POST /api/v1/config` on startup, so model/limit changes apply without an
//! engine restart. Pushes are strictly serialized (each carries the full
//! configuration and the engine keeps whatever lands last), never block
//! startup, and give up quietly when the engine is down — the engine then
//! keeps running its environment configuration until the next restart. The
//! historic `rag` section is gone: retrieval settings died with the document
//! store / PDF question-answer feature.

use std::{collections::BTreeSet, sync::OnceLock, time::Duration};

use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::runtime_config::{AiEnginePushSettings, RuntimeConfig};

const ENGINE_CONFIG_PATH: &str = "/api/v1/config";
const ENGINE_AUTH_HEADER: &str = "X-Engine-Auth";
/// Java `AiEngineConfigSync.MAX_ATTEMPTS` for the startup push.
const MAX_STARTUP_ATTEMPTS: u32 = 5;
/// Java `AiEngineConfigSync.RETRY_DELAY_MS`.
const RETRY_DELAY: Duration = Duration::from_secs(3);

// Java `ApplicationProperties.AiEngine` defaults, used to detect whether a
// section was configured or left at built-in values.
const DEFAULT_MODEL_PROVIDER: &str = "anthropic";
const DEFAULT_SMART_MODEL: &str = "claude-haiku-4-5";
const DEFAULT_FAST_MODEL: &str = "claude-haiku-4-5";

const MODEL_IDENTITY_KEYS: &[&str] = &[
    "aiEngine.models.provider",
    "aiEngine.models.smartModel",
    "aiEngine.models.fastModel",
    "aiEngine.models.apiKey",
    "aiEngine.models.baseUrl",
];

/// One queued push: the full JSON body plus how many delivery attempts it gets.
struct PushJob {
    body: String,
    attempts: u32,
}

#[derive(Clone)]
struct PushTransport {
    base_url: String,
    timeout: Duration,
    shared_secret: Option<String>,
    retry_delay: Duration,
}

/// Startup/save-time configuration pusher for the separately deployed engine.
pub struct AiEngineConfigSync {
    settings: AiEnginePushSettings,
    transport: PushTransport,
    // Lazily started single consumer keeps pushes strictly ordered, mirroring
    // Java's single-thread executor; created on first submit so construction
    // works outside an async runtime.
    sender: OnceLock<mpsc::UnboundedSender<PushJob>>,
}

impl AiEngineConfigSync {
    #[must_use]
    pub fn from_runtime_config(runtime_config: &RuntimeConfig) -> Self {
        let (_, base_url, timeout_seconds) = runtime_config.ai_engine_settings();
        Self::new(
            runtime_config.ai_engine_push_settings(),
            base_url,
            Duration::from_secs(timeout_seconds),
            crate::env_compat::var("RUSTLING_ENGINE_SHARED_SECRET")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            RETRY_DELAY,
        )
    }

    #[must_use]
    pub(crate) fn new(
        settings: AiEnginePushSettings,
        base_url: impl Into<String>,
        timeout: Duration,
        shared_secret: Option<String>,
        retry_delay: Duration,
    ) -> Self {
        Self {
            settings,
            transport: PushTransport {
                base_url: base_url.into(),
                timeout,
                shared_secret,
                retry_delay,
            },
            sender: OnceLock::new(),
        }
    }

    /// Pushes the full configuration on startup with bounded retries, mirroring
    /// Java `pushConfigOnStartup`. Never blocks: work happens on a background
    /// task, and a down engine only produces log warnings.
    pub fn push_on_startup(&self) {
        if !self.settings.enabled {
            return;
        }
        if !self.settings.push_config_to_engine {
            tracing::debug!(
                "skipping AI engine config push: aiEngine.pushConfigToEngine is disabled \
                 (the engine is configured from its own environment)"
            );
            return;
        }
        let mut node = build_config_node(&self.settings);
        keep_env_for_unconfigured_identity(&mut node, &BTreeSet::new());
        self.submit(PushJob {
            body: node.to_string(),
            attempts: MAX_STARTUP_ATTEMPTS,
        });
    }

    fn submit(&self, job: PushJob) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                "no async runtime available for the AI engine config push; the engine keeps \
                 its own environment configuration"
            );
            return;
        };
        let sender = self.sender.get_or_init(|| {
            let (sender, receiver) = mpsc::unbounded_channel();
            handle.spawn(run_push_worker(receiver, self.transport.clone()));
            sender
        });
        // A closed channel only means the worker ended at shutdown.
        let _ = sender.send(job);
    }
}

async fn run_push_worker(mut receiver: mpsc::UnboundedReceiver<PushJob>, transport: PushTransport) {
    let client = reqwest::Client::builder()
        .connect_timeout(transport.timeout)
        .timeout(transport.timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let Ok(client) = client else {
        tracing::warn!("could not build the AI engine config push client; pushes are disabled");
        return;
    };
    let endpoint = format!(
        "{}{ENGINE_CONFIG_PATH}",
        transport.base_url.trim().trim_end_matches('/')
    );
    while let Some(job) = receiver.recv().await {
        for attempt in 1..=job.attempts {
            match post_config(&client, &transport, &endpoint, &job.body).await {
                Ok(()) => {
                    tracing::info!(attempt, "pushed AI engine configuration");
                    break;
                }
                Err(error) => {
                    tracing::warn!(
                        attempt,
                        max_attempts = job.attempts,
                        %error,
                        "AI engine config push failed; the engine keeps running its previous \
                         configuration. If the engine is not on localhost, set \
                         RUSTLING_ENGINE_SHARED_SECRET on both the engine and the processor \
                         so it accepts the push."
                    );
                    if attempt < job.attempts {
                        tokio::time::sleep(transport.retry_delay).await;
                    } else if job.attempts > 1 {
                        tracing::warn!(
                            attempts = job.attempts,
                            "giving up pushing AI engine configuration; the engine will use its \
                             own environment configuration until the next restart"
                        );
                    }
                }
            }
        }
    }
}

async fn post_config(
    client: &reqwest::Client,
    transport: &PushTransport,
    endpoint: &str,
    body: &str,
) -> Result<(), String> {
    let mut request = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .body(body.to_owned());
    if let Some(secret) = &transport.shared_secret {
        request = request.header(ENGINE_AUTH_HEADER, secret);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("engine returned status {}", response.status()))
    }
}

/// The full engine payload from the live configuration, mirroring Java
/// `buildConfigNode` (camelCase spellings match the engine's wire contract).
fn build_config_node(settings: &AiEnginePushSettings) -> Value {
    json!({
        "models": {
            "provider": settings.models.provider,
            "smartModel": settings.models.smart_model,
            "fastModel": settings.models.fast_model,
            "smartMaxTokens": settings.models.smart_max_tokens,
            "fastMaxTokens": settings.models.fast_max_tokens,
            "apiKey": settings.models.api_key,
            "baseUrl": settings.models.base_url,
        },
        "limits": {
            "maxPages": settings.limits.max_pages,
            "maxCharacters": settings.limits.max_characters,
            "modelMaxConcurrency": settings.limits.model_max_concurrency,
        },
    })
}

fn text(section: &Value, field: &str) -> String {
    // Jackson `asText("")`: strings verbatim, scalars stringified, containers
    // and missing values become the empty default.
    match section.get(field) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn touched_identity(touched_keys: &BTreeSet<String>, identity_keys: &[&str]) -> bool {
    identity_keys.iter().any(|key| touched_keys.contains(*key))
}

fn blank_fields(section: &mut Value, fields: &[&str]) {
    if let Some(map) = section.as_object_mut() {
        for field in fields {
            map.insert((*field).to_owned(), Value::String(String::new()));
        }
    }
}

/// Blanks the identity (provider/model/credentials) of an unconfigured models
/// section so the push keeps the engine's env values; edited sections are sent
/// as-is so a cleared key really clears. Mirrors Java
/// `keepEnvForUnconfiguredIdentity`.
fn keep_env_for_unconfigured_identity(root: &mut Value, touched_keys: &BTreeSet<String>) {
    if let Some(models) = root.get_mut("models").filter(|value| value.is_object()) {
        let configured = touched_identity(touched_keys, MODEL_IDENTITY_KEYS)
            || !text(models, "apiKey").trim().is_empty()
            || !text(models, "baseUrl").trim().is_empty()
            || text(models, "provider") != DEFAULT_MODEL_PROVIDER
            || text(models, "smartModel") != DEFAULT_SMART_MODEL
            || text(models, "fastModel") != DEFAULT_FAST_MODEL;
        if !configured {
            blank_fields(
                models,
                &["provider", "smartModel", "fastModel", "apiKey", "baseUrl"],
            );
        }
    }
}

/// Engine-side `POST /api/v1/config` stub shared by the config-sync tests and
/// the admin-settings save-trigger test.
#[cfg(test)]
pub(crate) mod test_support {
    use std::{net::SocketAddr, sync::Arc};

    use axum::{Router, extract::State, http::HeaderMap, routing::post};
    use serde_json::Value;
    use tokio::sync::{Mutex, mpsc};

    pub(crate) struct Recorded {
        pub(crate) headers: HeaderMap,
        pub(crate) body: Value,
    }

    type Recordings = Arc<Mutex<(u32, mpsc::UnboundedSender<Recorded>)>>;

    /// Starts a loopback engine stub whose first `fail_first` requests return
    /// HTTP 500; every later push is recorded on the returned channel.
    pub(crate) async fn start_engine_stub(
        fail_first: u32,
    ) -> Result<(SocketAddr, mpsc::UnboundedReceiver<Recorded>), std::io::Error> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let state: Recordings = Arc::new(Mutex::new((fail_first, sender)));
        let app = Router::new()
            .route(
                "/api/v1/config",
                post(
                    |State(state): State<Recordings>, headers: HeaderMap, body: String| async move {
                        let mut guard = state.lock().await;
                        if guard.0 > 0 {
                            guard.0 -= 1;
                            return axum::http::StatusCode::INTERNAL_SERVER_ERROR;
                        }
                        let _ = guard.1.send(Recorded {
                            headers,
                            body: serde_json::from_str(&body).unwrap_or(Value::Null),
                        });
                        axum::http::StatusCode::OK
                    },
                ),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((address, receiver))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, time::Duration};

    use serde_json::{Value, json};

    use super::{
        AiEngineConfigSync, build_config_node, keep_env_for_unconfigured_identity,
        test_support::start_engine_stub,
    };
    use crate::runtime_config::AiEnginePushSettings;

    fn default_settings() -> AiEnginePushSettings {
        AiEnginePushSettings {
            enabled: true,
            ..AiEnginePushSettings::default()
        }
    }

    #[test]
    fn payload_uses_the_engine_wire_spellings() -> Result<(), Box<dyn std::error::Error>> {
        let node = build_config_node(&default_settings());
        let models = node
            .get("models")
            .and_then(Value::as_object)
            .ok_or("models section")?;
        for key in [
            "provider",
            "smartModel",
            "fastModel",
            "smartMaxTokens",
            "fastMaxTokens",
            "apiKey",
            "baseUrl",
        ] {
            assert!(models.contains_key(key), "models.{key} missing");
        }
        // The removed retrieval settings must no longer be pushed.
        assert!(node.get("rag").is_none(), "rag section should be gone");
        let limits = node
            .get("limits")
            .and_then(Value::as_object)
            .ok_or("limits section")?;
        for key in ["maxPages", "maxCharacters", "modelMaxConcurrency"] {
            assert!(limits.contains_key(key), "limits.{key} missing");
        }
        assert_eq!(node["limits"]["maxPages"], json!(200));
        assert_eq!(node["limits"]["maxCharacters"], json!(200_000));
        assert_eq!(node["limits"]["modelMaxConcurrency"], json!(32));
        Ok(())
    }

    #[test]
    fn unconfigured_identity_is_blanked_so_the_engine_keeps_env_values() {
        let mut node = build_config_node(&default_settings());
        keep_env_for_unconfigured_identity(&mut node, &BTreeSet::new());
        for key in ["provider", "smartModel", "fastModel", "apiKey", "baseUrl"] {
            assert_eq!(node["models"][key], json!(""), "models.{key}");
        }
        // Non-identity values keep their concrete defaults.
        assert_eq!(node["models"]["smartMaxTokens"], json!(8_192));
    }

    #[test]
    fn configured_or_touched_identity_is_sent_as_is() {
        // An api key makes the models section configured.
        let mut settings = default_settings();
        settings.models.api_key = "sk-live".to_owned();
        let mut node = build_config_node(&settings);
        keep_env_for_unconfigured_identity(&mut node, &BTreeSet::new());
        assert_eq!(node["models"]["provider"], json!("anthropic"));
        assert_eq!(node["models"]["apiKey"], json!("sk-live"));

        // Touching an identity key keeps the section even at default values,
        // so an admin's explicit clear really clears.
        let mut node = build_config_node(&default_settings());
        let touched = BTreeSet::from(["aiEngine.models.apiKey".to_owned()]);
        keep_env_for_unconfigured_identity(&mut node, &touched);
        assert_eq!(node["models"]["provider"], json!("anthropic"));
    }

    #[tokio::test]
    async fn startup_push_delivers_the_secret_header_and_retries_through_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let (address, mut received) = start_engine_stub(1).await?;
        let sync = AiEngineConfigSync::new(
            default_settings(),
            format!("http://{address}"),
            Duration::from_secs(5),
            Some("push-secret".to_owned()),
            Duration::from_millis(20),
        );
        sync.push_on_startup();
        let recorded = tokio::time::timeout(Duration::from_secs(10), received.recv())
            .await?
            .ok_or("stub records the push")?;
        assert_eq!(
            recorded
                .headers
                .get("X-Engine-Auth")
                .and_then(|value| value.to_str().ok()),
            Some("push-secret")
        );
        // Unconfigured identity travels blank so the engine keeps env values.
        assert_eq!(recorded.body["models"]["provider"], json!(""));
        assert_eq!(recorded.body["limits"]["maxPages"], json!(200));
        Ok(())
    }

    #[tokio::test]
    async fn disabled_engine_or_push_flag_suppresses_all_pushes()
    -> Result<(), Box<dyn std::error::Error>> {
        let (address, mut received) = start_engine_stub(0).await?;
        let mut disabled = default_settings();
        disabled.enabled = false;
        let sync = AiEngineConfigSync::new(
            disabled,
            format!("http://{address}"),
            Duration::from_secs(1),
            None,
            Duration::from_millis(10),
        );
        sync.push_on_startup();

        let mut pinned = default_settings();
        pinned.push_config_to_engine = false;
        let sync = AiEngineConfigSync::new(
            pinned,
            format!("http://{address}"),
            Duration::from_secs(1),
            None,
            Duration::from_millis(10),
        );
        sync.push_on_startup();

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(received.try_recv().is_err());
        Ok(())
    }
}
