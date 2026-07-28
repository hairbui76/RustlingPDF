# AI engine proxy contract

This contract covers the proprietary Java `AiEngineController` surface ported
into `rustling-processing`, including both transparent engine proxies and the
Java-facing multipart workflow state machine.

## Mounted routes

### `GET /api/v1/ai/health`

- Returns `503 application/problem+json` with `AI engine is not enabled` when
  `aiEngine.enabled` is false.
- Otherwise sends `GET {aiEngine.url}/health` with `Accept: application/json`.
- Sends `X-Engine-Auth` only when `RUSTLING_ENGINE_SHARED_SECRET` is nonblank.
- Never sends `X-User-Id` (the product has no user identity). An inbound
  caller-supplied `X-User-Id` is never forwarded.
- On a successful upstream response, returns status `200`, content type
  `application/json`, and the upstream JSON body. As in Java, a non-error 3xx
  upstream response is not followed and its body is surfaced as a 200.

### `POST /api/v1/ai/pdf/edit`

- Requires `Content-Type: application/json` (parameters such as `charset` are
  accepted).
- Returns 400 with Java's controller messages when the body is invalid JSON or
  is not a JSON object.
- Always overwrites client-supplied `enabled_endpoints`. The replacement is the
  sorted intersection of the Rust processing routes and the Rust engine's
  generated PDF-operation catalog, filtered through `RuntimeConfig` endpoint
  availability. Java sends every enabled Spring mapping and the engine drops
  unknown mappings, so the engine-visible planning catalog is equivalent.
- Sends the rewritten JSON to `{aiEngine.url}/api/v1/pdf/edit` with the same
  engine-auth and identity rules as the health proxy.
- Returns status `200`, content type `application/json`, and the successful
  upstream response body without interpreting the plan.

### `POST /api/v1/ai/orchestrate`

- Accepts Java-compatible multipart fields: `userMessage`, repeated
  `fileInputs[i].fileInput`, and optional
  `conversationHistory[i].role`/`content` pairs.
- Streams uploads to a private temporary workspace and assigns the first 16
  hexadecimal characters of each file's SHA-256 digest as its stable engine ID.
- Sends typed JSON turns to `{aiEngine.url}/api/v1/orchestrator`, consumes its
  bounded NDJSON stream incrementally, and preserves engine progress,
  heartbeat, result, and error semantics.
- Resolves `need_content` by extracting the requested one-based PDF pages with
  Java-compatible global page/UTF-16 character budgets and resuming the
  requested capability with an `extracted_text` artifact. The legacy
  `need_ingest` protocol was removed with the engine's document store: a
  legacy engine frame carrying it fails the turn cleanly (500,
  `AI engine stream ended without a result`) and no ingest request is sent.
- Executes `tool_call` and multi-step `plan` outcomes through the in-process
  policy dispatcher. Only configured processing endpoints and the exact
  `/api/v1/ai/tools/*` namespace are permitted; recursive orchestration and
  arbitrary internal paths are rejected.
- Preserves Java tool metadata for single/multi-input dispatch and ZIP
  fan-out. JSON responses and `X-Stirling-Tool-Report` headers become typed
  report artifacts when an engine resume is requested.
- Stores every generated or processed output individually under one
  owner-scoped Rust job. The response includes `resultFiles` descriptors and
  mirrors the first descriptor into `fileId`, `fileName`, and `contentType` for
  older clients. Files remain downloadable through
  `GET /api/v1/general/files/{fileId}`.
- One-to-one same-extension transforms reuse the input filename and expose a
  `sourceIndex`. One-to-many outputs keep their tool filenames and omit
  `sourceIndex`, matching the workbench replacement contract.

### `POST /api/v1/ai/orchestrate/stream`

- Runs the same workflow and returns `text/event-stream`.
- Emits named `progress` events for `analyzing`, `calling_engine`,
  `extracting_content`, `executing_tool`, `processing`, and nested
  `engine_progress`; upstream heartbeats become named `heartbeat` events.
- Terminates with exactly one named `result` or `error` event. The timeout is
  controlled by `stirling.ai.streamTimeoutMs`/
  `RUSTLING_AI_STREAMTIMEOUTMS` and defaults to 1,800 seconds.
- A downstream disconnect drops the workflow future and its upstream reqwest
  response, cancelling engine generation and preventing further turns or tool
  steps from being scheduled. A native blocking operation already inside a
  non-cancellable library call may still finish before its temporary workspace
  is released.

## Upstream error mapping

The proxy retains `AiEngineClient` behavior:

| Condition | Public status | Detail |
| --- | ---: | --- |
| Engine disabled | 503 | `AI engine is not enabled` |
| Connect/read failure | 503 | `AI engine unreachable: ...` |
| Timeout | 504 | `AI engine timed out` |
| Upstream 4xx | same 4xx | `AI engine returned client error: {body}` |
| Upstream 5xx | 502 | `AI engine returned error: {status}` |

Errors use `application/problem+json` and include the Java-facing type, title,
status, detail, timestamp, and request path fields.

## Identity boundary

The routes are open like every processing route. No user identity exists or is
fabricated for the engine; the only cross-service credential is the optional
`X-Engine-Auth` shared secret.

## Resource bounds

Multipart text fields and individual NDJSON frames are capped at 1 MiB, field
indices are capped at 10,000, and workflows stop after 16 engine turns. Tool
uploads and outputs remain streamed through files instead of accumulated in
memory. Engine long-running calls use
`aiEngine.longRunningTimeoutSeconds`/`AIENGINE_LONGRUNNINGTIMEOUTSECONDS`
(default 600 seconds).

## Processor→engine config push

Mirroring Java `AiEngineConfigSync` (`ai_engine_config_sync.rs`), the
processor pushes the engine-relevant `aiEngine.*` configuration to the
engine's `POST /api/v1/config`:

- **On startup** (fired from `spawn_background_maintenance`, the Rust analog
  of `ApplicationReadyEvent`): the full models/limits payload, retried up
  to 5 times with a 3-second delay, entirely off-thread — a down or
  still-booting engine never blocks startup and only produces warnings.

(The former after-admin-save live push was removed with the admin settings
API.) The gates match Java: nothing is pushed unless `aiEngine.enabled` is true
and `aiEngine.pushConfigToEngine` (default `true`, env
`AIENGINE_PUSHCONFIGTOENGINE`) is on — pin it false for env-driven
deployments so the engine stays environment-controlled. Pushes carry the
`X-Engine-Auth` shared secret (`RUSTLING_ENGINE_SHARED_SECRET`) when set and
are strictly serialized through one queue (Java's single-thread executor), so
overlapping pushes cannot leave the engine on a stale payload.

Payload rules ported from Java: camelCase spellings match the engine's
tolerant wire contract; unconfigured model identity
(provider/model/credential fields all at Java defaults with blank keys) is
blanked to empty strings so the engine keeps its own env credentials, while a
section that is configured — or whose identity keys were touched — travels
as-is so an explicit clear really clears. Settings resolution uses Spring's
relaxed spellings (`AIENGINE_MODELS_SMARTMODEL`, `AIENGINE_LIMITS_MAXPAGES`,
…) over the `aiEngine.models/limits` YAML sections with Java's defaults
(`anthropic`/`claude-haiku-4-5`, 8192/2048 tokens, maxPages 200,
maxCharacters 200000, modelMaxConcurrency 32). The historic `rag` section is
no longer read or pushed: retrieval settings died with the engine's document
store and PDF question-answer feature. A legacy `aiEngine.rag.*` block or
`AIENGINE_RAG_*` variable in an existing install is ignored with a one-line
startup warning, never refused.
