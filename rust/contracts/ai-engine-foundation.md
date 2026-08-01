# Rust AI engine

`rustling-ai-engine` is the optional stateless reasoning service used by the
processing runtime. It binds to `127.0.0.1:5001` by default.
`RUSTLING_ENGINE_HOST` and `RUSTLING_ENGINE_PORT` select an explicit address;
port `0` requests an ephemeral port.

The engine keeps no document database, embedding store, user account, or
server-side conversation state. Every capability operates only on bounded
content supplied with its request.

## Routes

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | Runtime and configured-model health |
| GET | `/api/v1/agents/capabilities` | Stateless capability manifest |
| POST | `/api/v1/documents/classify` | Request-scoped document classification |
| POST | `/api/v1/ai/math-auditor-agent/examine` | Determine required math evidence |
| POST | `/api/v1/ai/math-auditor-agent/deliberate` | Validate arithmetic and produce a verdict |
| POST | `/api/v1/ai/pdf-comment-agent/generate` | Generate grounded review comments |
| POST | `/api/v1/ai/document/summary` | Produce a page-cited summary |
| POST | `/api/v1/ai/document/extraction` | Extract fields against a caller schema |
| POST | `/api/v1/ai/document/translation` | Translate ordered content blocks |
| POST | `/api/v1/pdf/edit` | Produce a validated PDF edit plan |
| POST | `/api/v1/agents/draft` | Draft a saved-agent workflow |
| POST | `/api/v1/agents/revise` | Revise a saved-agent workflow |
| POST | `/api/v1/agents/next-action` | **Not implemented.** Always answers `cannot_continue` with a reason naming the step, whatever the request contains. It is registered so a caller receives that refusal rather than a 404, and it is deliberately absent from `/api/v1/agents/capabilities` so nothing discovers it as a working planner. A multi-step saved-agent workflow therefore cannot advance past its first step. |
| POST | `/api/v1/orchestrator` | Stream orchestration progress and result frames |
| POST | `/api/v1/config` | Apply bounded model and limit configuration |

The `/api/v1/ai/agents/draft` and `/api/v1/ai/agents/revise` aliases are also
accepted by the router.

## Request boundary

- `/health` is public.
- Other routes require a constant-time `X-Engine-Auth` match when
  `RUSTLING_ENGINE_SHARED_SECRET` is configured.
- If `RUSTLING_ENGINE_REQUIRE_AUTH=true` and no secret is configured, protected
  routes return 503.
- JSON request models reject unknown fields with 422, except config push, which
  ignores unknown fields so processor and engine versions can roll
  independently.
- Environment booleans, numeric limits, and ports are parsed strictly before
  binding. Malformed values fail startup.
- Request, page, character, chunk, token, worker, and concurrency limits are
  enforced before model work.

## Providers

Smart and fast model tiers support:

- `anthropic:<model>`;
- `openai:<model>`;
- `ollama:<model>` for local or authenticated Ollama-compatible gateways; and
- configured OpenAI-compatible endpoints.

Provider credentials come from the engine process environment or an authorized
config push. Ollama defaults to `http://localhost:11434` and needs no key for a
local server. Structured outputs are validated again against each operation's
typed schema after transport parsing.

`RUSTLING_MODEL_MAX_CONCURRENCY` defaults to 32 and applies one process-wide
semaphore across model tiers. Agent-specific worker counts are additional,
narrower bounds.

## Deterministic safeguards

Model output cannot bypass deterministic validation:

- math audit uses fixed-point arithmetic, constrained formula parsing, and
  cross-page figure checks;
- PDF comments expose only bounded chunk ordinals to the model and map accepted
  ordinals back to caller-owned IDs;
- extraction validates the caller schema and returned value;
- translation preserves block order and validates block identity;
- PDF edit and saved-agent steps must match the generated operation catalog;
- unknown tools, invalid parameters, and out-of-range references are rejected.

Provider failures and malformed structured output return an upstream-service
error rather than a false successful empty result.

## Orchestration

`POST /api/v1/orchestrator` streams newline-delimited heartbeat, progress, and
result frames. Dropping the response cancels the active workflow and releases
its model-concurrency permit.

PDF review and edit flows request bounded document content through the
processing service. Document creation sends a structured document model to the
fixed renderer. No route ingests content for later retrieval.

## Config push

`POST /api/v1/config` accepts `models` and `limits`. Credentials are never
returned. `RUSTLING_ALLOW_CONFIG_PUSH` defaults to true.

When a shared secret exists, normal engine authentication protects the route.
Without a secret, config push accepts only a direct loopback peer and rejects
forwarding headers. Invalid bounds return 422; provider construction failures
return 400 and leave the running configuration untouched. A successful swap is
atomic, so in-flight requests retain the snapshot they started with.

The last operator-pushed model/limit configuration may be cached as
`data/ai_config_cache.enc`. AES-256-GCM protects it with an HKDF-SHA256-derived
key; when no shared secret exists, a mode-0600 local key file is used. The
cache contains no document or user content. A missing, unreadable, corrupt, or
wrong-key cache is ignored and can be replaced by the next push.

## Operation catalog

`task engine:tool-models` generates
`rust/crates/rustling-ai-engine/src/operation_catalog.json` from the committed
`SwaggerDoc.json` through `rustling-operation-catalog`. The engine therefore
validates deterministic operation IDs and parameters without a runtime OpenAPI
dependency.

## Running and validation

```bash
task engine:dev
task engine:check
task dev:all
```

The Docker `ai-engine` target runs as a non-root user on port 5001 and includes
only the engine binary plus basic runtime certificates and health tooling.
Provider credentials are supplied at deployment time.

Tests cover route authentication, strict startup parsing, provider adapters,
structured-output validation, deterministic safeguards, cancellation, config
push authorization, encrypted cache recovery, operation-catalog validation,
and stateless document-understanding workflows.
