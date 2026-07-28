# Rust AI Engine Foundation Contract

`rustling-ai-engine` owns the Rust process boundary and the stateless HTTP
agent surface. It binds to `127.0.0.1:5001` by default.
`RUSTLING_ENGINE_HOST` accepts an explicit IPv4 or IPv6 address and
`RUSTLING_ENGINE_PORT` accepts a port from `0` through `65535`; port `0` selects
an ephemeral port and the startup log reports the address actually assigned.

The engine keeps **no server-side document state**: the document/RAG store,
its `/api/v1/documents*` lifecycle routes, the `migrate-sqlite-vec` binary,
and the `pdf-question-answer` capability were removed with the product's
question-answer feature. Every surviving capability reasons only over content
supplied with the request.

## Implemented compatibility boundary

- `GET /health` is public and returns `status`, `smart_model`, and `fast_model`.
- Non-health routes use `X-Engine-Auth` when
  `RUSTLING_ENGINE_SHARED_SECRET` is configured. The comparison is constant-time.
- When `RUSTLING_ENGINE_REQUIRE_AUTH=true` but no secret is configured, non-health
  routes return `503` rather than run without authentication.
- There is no per-user identity gate: `X-User-Id` headers are neither required
  nor read. The former `RUSTLING_REQUIRE_USER_ID` variable — like every
  `RUSTLING_DOCUMENTS_*` / `RUSTLING_RAG_*` variable of the removed store —
  is ignored with a one-line startup warning when present, never refused.
- Environment-backed booleans and numeric limits are parsed strictly before the
  listener binds. A present malformed or non-Unicode value terminates startup
  instead of substituting a default; this applies in particular to
  `RUSTLING_ENGINE_REQUIRE_AUTH`, so a typo cannot silently weaken the request
  gate. Existing chunk, worker, contradiction, concurrency, and token bounds
  are validated at the same boundary.
- Every JSON POST request accepts both the Python `ApiModel` camel-case aliases
  and its snake-case field names, including nested request models. Unknown
  fields are rejected with `422` instead of being silently ignored. The one
  deliberate exception is the config-push contract, which mirrors the oracle's
  `TolerantApiModel` (`extra="ignore"`): a newer processor must be able to push
  to an older engine, so unknown push fields are ignored.
- Default model names match the existing engine configuration:
  `anthropic:claude-haiku-4-5` for both smart and fast models.

Structured model inference supports `anthropic:`, `openai:`, and the Python
oracle's self-hosted `ollama:` model prefix for both the smart and fast tiers.
`ollama:<model-id>` defaults to `http://localhost:11434`, honors
`OLLAMA_BASE_URL`, and does not require a credential for a local server. An
optional non-empty `OLLAMA_API_KEY` is sent as a bearer token for authenticated
remote gateways. Ollama uses its OpenAI-compatible chat-completions surface;
origins, `/v1` bases, and complete `/v1/chat/completions` URLs normalize to one
endpoint. Rust sends the caller-supplied schema through the native
`response_format.json_schema` contract, matching the Python oracle's
`NativeOutput` behavior; response content may be a JSON string or object and is
validated again by each typed agent after transport parsing.

## Ported ledger-auditor capabilities

`POST /api/v1/ai/math-auditor-agent/examine` now ports the first, deterministic
round of the ledger-auditor protocol. Its `FolioManifest` request and
`Requisition` response retain Python's camel-case wire shape and the Pydantic
numeric bounds. The Python prompt defines a fixed policy, so Rust computes it
directly: `text` and `mixed` pages request text plus table extraction; `image`
and `mixed` pages request OCR. This removes an unnecessary model call without
allowing the engine to invent page requirements.

`POST /api/v1/ai/math-auditor-agent/deliberate` also ports the terminal audit
round. It accepts typed `Evidence` plus optional `?tolerance=<decimal>` (the
default is `0.01`) and returns the Java-compatible `Verdict` envelope. Rust
first checks inline arithmetic and evaluates model-inferred CSV formulas using
fixed-point decimal. It then uses forced structured-output calls for named
figures, formula inference, prose statement verification, and the summary;
failed individual model calls are isolated just as in Python, and summary falls
back deterministically. An invalid tolerance returns `400`; invalid evidence
returns `422`.

`GET /api/v1/agents/capabilities` returns version 1 with the seven surviving
capabilities: PDF edit, agent draft, agent revision, both math-audit rounds,
PDF comments, and agent next-action (`pdf-question-answer` was removed with
the question-answer feature). The document-classifier route remains outside
the agent manifest. The public Math Auditor workflow is owned by
`rustling-processing` at `POST /api/v1/ai/tools/math-auditor-agent`, which
retains the PDF and calls these two engine rounds; see
`math-auditor-agent.md`.

The Rust ledger module also ports the deterministic validators used by the
future deliberation round: an exact fixed-point scanner for inline additions
and subtractions; a labelled-figure tracker for cross-page consistency; and a
CSV formula evaluator for `each_row`, `column_total`, and `single_cell`
checks. It supports the constrained `colN`, `cell(row, col)`, and
`sum(colN, start-end)` grammar without `eval` or binary floating point.
The deliberation orchestrator combines these validators with the typed model
calls, so model output is never allowed to bypass the deterministic checks.

## Ported PDF comment-generation route

`POST /api/v1/ai/pdf-comment-agent/generate` now ports the Python agent that
selects positioned PDF text chunks for review comments. It preserves the
camel-case wire response and also accepts the Python contract's snake-case
input aliases. The request caps session IDs, prompts, chunks, and chunk text;
the model sees only zero-based chunk ordinals and Rust maps valid ordinals back
to the caller's opaque IDs. Out-of-range ordinals are dropped, an empty chunk
list bypasses the model with the same explanatory response, and malformed or
provider-failed model output returns `502` rather than a false successful
empty response. Invalid client contracts return `422`.

The route is published as the `pdf-comment-generate` MCP capability, matching
the Python manifest. The separate public multipart PDF annotation workflow is
owned by
`rustling-processing` at `POST /api/v1/ai/tools/pdf-comment-agent`: it extracts
bounded PDFium text chunks, calls this engine route, resolves returned IDs
locally, and writes PDF annotations. It remains a processing API rather than an
engine capability. See `pdf-comment-agent.md` for the public contract.

## Removed document storage and PDF question-answer surface

The former document lifecycle routes (`POST /api/v1/documents`,
`DELETE /api/v1/documents/by-id/{documentId}`,
`DELETE /api/v1/documents/by-owner`), the SQLite/pgvector vector stores, the
embedding providers, the background reaper, the `migrate-sqlite-vec` binary,
and `POST /api/v1/pdf/questions` are deleted: the maintainers removed the PDF
question-answer feature together with its documents/RAG store, and the
stateless server keeps no ingested user content. Those routes now return
`404`. `POST /api/v1/documents/classify` — a stateless, per-request
classifier that merely shares the path prefix — survives unchanged.

## Ported orchestration and agent workflows

`POST /api/v1/orchestrator` streams newline-delimited heartbeat and result
frames and routes PDF edit, review, create, and saved-agent drafting requests;
question-style requests resolve to an `unsupported_capability` response
(`pdf-question-answer`). Dropping the NDJSON response immediately cancels the
active workflow instead of waiting for the next heartbeat; any in-flight
provider future is dropped and releases its shared model-concurrency permit.
No identity is required for any route: every delegate reasons only over
request-supplied content.
Resume capability dispatch is deterministic. PDF edit parameters are validated
against the generated snapshot of all current Java operation schemas and only
server-enabled operations may be selected. PDF review produces grounded sticky
comments for ordinary review, contradiction, and math-audit flows. The
contradiction flow is fully request-driven: when the turn carries no
`extracted_text` artifact, the engine answers `need_content`
(`resumeWith=pdf_review`, with the engine's `maxPages`/`maxCharacters`
bounds) and the processor resends the turn with extracted page text; the claim
extraction, canonicalisation, subject bucketing, pair detection and grounded
summarisation pipeline then runs over those pages alone. PDF creation uses a
typed metadata/outline/parallel-section pipeline and sends only a structured
document model to the fixed processing renderer.

`POST /api/v1/agents/draft` and `/revise` port the saved-agent workflow. Drafts
are built from validated PDF edit plans; revision replaces deterministic tool
steps and preserves existing `ai_tool` steps. Both `/api/v1/agents/...` and the
Python manifest's `/api/v1/ai/agents/...` draft/revise paths are accepted.
Every saved-agent step is validated at deserialization and model-output
boundaries. Deterministic `tool` steps use the generated Java operation catalog
plus the three Python-compatible agent operations (math audit, PDF comments,
and HTML document creation), while `ai_tool` steps accept only generated Java
processing endpoints. Unknown tool IDs and tool/parameter schema mismatches are
rejected; Python snake-case parameter aliases are canonicalized and declared
defaults are materialized. Previous-step tool IDs supplied to next-action use
the combined deterministic-operation registry.
`POST /api/v1/agents/next-action` intentionally preserves Python's current
terminal `cannot_continue` behavior rather than pretending execution planning
exists.

## Ported config push

`POST /api/v1/config` accepts the processor's startup AI settings push
(`AiEngineConfigSync`) with the `ConfigPushRequest` shape: `models`
(provider/smartModel/fastModel/smartMaxTokens/fastMaxTokens/apiKey/baseUrl)
and `limits` (maxPages/maxCharacters/modelMaxConcurrency). The historic `rag`
section is no longer part of the contract; an older processor still sending
one is tolerated and the section is ignored like any unknown field. Empty
strings and omitted numbers keep the engine's current values; camel-case and
snake-case names are both accepted. Responses use the `ConfigApplyResponse`
camel-case summary (without the removed `rag*` fields) and never echo
credentials.

Gating and authorization:

- `RUSTLING_ALLOW_CONFIG_PUSH` defaults to `true` and is strict-parsed at the
  fail-closed env boundary; when false the route returns `403` naming the
  flag.
- With a shared secret configured, the normal `X-Engine-Auth` middleware
  protects the route. With no secret, only a direct loopback transport peer is
  trusted; any forwarding header (`x-forwarded-for`, `x-forwarded-host`,
  `x-real-ip`, `forwarded`) or a non-loopback/unknown peer returns `403`
  naming `RUSTLING_ENGINE_SHARED_SECRET`. Peer addresses come from
  `into_make_service_with_connect_info`; a build without connect info (e.g.
  embedded router tests) fails closed.
- Out-of-range numbers (zero where a bound requires `>= 1`; negative anywhere)
  return `422`.

Apply semantics: an explicit provider/api-key/base-URL push rebuilds both
model tiers (`anthropic`, `openai`, keyless `ollama`, and `custom` as an
OpenAI-compatible endpoint); the first explicit push over an env engine strips
`provider:` prefixes from the running names while later pushes keep bare names
intact (tracked via the pushed `chat_provider`, so `llama3.1:8b` is never
truncated). A rebuilt runtime gets a fresh shared concurrency semaphore sized
by the effective `modelMaxConcurrency`. Construction failures reject the push
with `400` and leave the running config untouched. The swap is atomic:
in-flight requests keep the snapshot they started with.

The applied push is persisted encrypted as `data/ai_config_cache.enc` (with an
`ai_config_cache.key` 0600 fallback keyfile when no shared secret is set) and
re-applied at boot when `RUSTLING_ALLOW_CONFIG_PUSH` is enabled; an
unreadable, corrupt, or wrong-key cache logs a warning and boots from env.
This cache is deliberately kept after the statelessness cutover: it stores
only the last operator-pushed model/limit configuration (no user content),
lives in the engine's own working directory, and merely saves a re-push after
an engine-only restart — the processor re-pushes the same configuration from
`settings.yml` on every startup anyway, so deleting the cache loses nothing.

- The cipher is the repository's established AES-256-GCM AEAD with an
  HKDF-SHA256 key (info string `stirling-ai-config-cache/v1/aead-key`); a
  leftover legacy Fernet file is ignored like a corrupt cache and self-heals
  on the next push.
- A cached payload written before the RAG removal may still contain a `rag`
  section; the tolerant contract ignores it on restore.
- A push whose model rebuild cannot authenticate fails closed with `400`
  (e.g. provider `anthropic` with no pushed key and no `ANTHROPIC_API_KEY`).
- For pushed `ollama`/`custom` providers with an empty `baseUrl`, the engine
  falls back to `OLLAMA_BASE_URL`/`http://localhost:11434` (ollama) or
  `OPENAI_BASE_URL`/the hosted default (custom).

The provider-aware output-mode switch maps to the Rust adapters'
structured-output protocol: pushed `ollama`/`custom` providers use the native
json-schema `response_format` protocol, while `openai` keeps forced function
calls — the same per-provider split the env path already used. The capability
manifest deliberately stays at seven entries: config push is not an agent
capability.

## Operational runtime

All Task entry points run `rustling-ai-engine`: `task engine:dev`,
`engine:run`, `engine:test`, and `engine:check`. Consequently `task dev:all`
starts the Rust engine process and configures the processing backend's AI
proxy with its selected port. (The former Python commands and the `engine/`
dotenv-loading convenience existed only in the upstream Stirling-PDF monorepo,
whose Python `engine/` tree is not part of this repository; provider
credentials are supplied through the process environment.)

`RUSTLING_MODEL_MAX_CONCURRENCY` defaults to `32` and limits all structured
model completions through one process-wide semaphore shared by the smart and
fast model tiers. Agent-specific worker limits remain additional, narrower
bounds; switching tiers cannot bypass the provider-account ceiling.

The upstream Stirling-PDF monorepo built an engine container image
(`engine/Dockerfile` there): a pinned Rust builder producing both the server
and `migrate-sqlite-vec`, with a non-Python Debian runtime installing only CA
certificates, running as a non-root user, and binding `0.0.0.0:5001`. This
repository does not yet ship a Dockerfile; container packaging is a tracked
roadmap item and the description above is the reference shape for it.

`task engine:tool-models` reads the committed `SwaggerDoc.json` OpenAPI
snapshot at the repository root directly through the typed Rust
`rustling-operation-catalog` generator and updates the compile-time
`operation_catalog.json` without Python. The generator preserves the former
endpoint allow/exclude rules, camel-case acronym aliases, optional
field/default behavior, and transitive component schemas. (The parallel Python
`tool_models.py` generation and the CI step diffing both artifacts live in the
upstream Stirling-PDF monorepo alongside the Python oracle.)

## Remaining cutover constraints

The provider-independent document-classifier contract is ported in
`rustling_ai_engine::document_classifier`: request validation, bounded first/last
page selection, prompt construction, provider-neutral structured-output agent,
and caller-vocabulary output validation. `POST /api/v1/documents/classify` is
available through the Anthropic Messages adapter when
`RUSTLING_FAST_MODEL=anthropic:<model-id>` and `ANTHROPIC_API_KEY` are set. An
OpenAI-compatible and self-hosted gateways can instead use
`RUSTLING_FAST_MODEL=openai:<model-id>`, `OPENAI_API_KEY`, and (when needed)
`OPENAI_BASE_URL`. Native keyless Ollama uses
`RUSTLING_FAST_MODEL=ollama:<model-id>` plus optional `OLLAMA_BASE_URL`. An
invalid/missing provider configuration returns `503`; provider failures return
`502`; invalid classifier input returns `422`.

`app_with_classifier` remains the explicit seam for provider adapters beyond
Anthropic, OpenAI-compatible gateways, and Ollama.

Provider adapters implement `rustling_ai_engine::structured_output`, which
forces a named schema, tool, or function and returns only its JSON object to the
agent. The classifier, ledger auditor, PDF comment agent, and review/edit
agents use that seam. Anthropic, OpenAI-compatible, and Ollama adapters all
enforce the caller-supplied schema rather than carrying classifier-only response
parsing.

## Required proof before cutover

Every advertised capability has a typed request/response boundary and contract
coverage. A process-level smoke test starts the compiled binary on an ephemeral
port and verifies public health, shared-secret failures, the absence of the
removed document/question routes, authenticated capabilities, a representative
POST, and an authenticated config push independently of the in-process router
tests. The two server smoke tests
previously timed out everywhere — not environmentally: the binary emitted ANSI
escapes into piped stdout by default, so the harness's `address=` startup parse
could never match. The binary now colours only real terminals
(`with_ansi(stdout().is_terminal())`), and the smoke harness captures both
child streams and prints them on a startup timeout so any future hang is
diagnosable from the failure message alone. Production cutover still requires provider
credentials, Java proxy routing, storage selection, and the relevant processing
service to be verified in the target deployment.
