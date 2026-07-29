# Execution Plan: Remove Authentication And All Server-Side State

Date: 2026-07-28

## Status

Completed

## Outcome

RustlingPDF becomes a pure stateless PDF server: requests in, PDFs out. No
login of any kind, no server-side database or accumulating writes, no MCP
server, no AI question-answer feature. The only durable writes are the desktop
(Tauri) sidecar's `settings.yml` maintenance on the user's own machine. The
SPA/desktop client owns all user state (signatures, watched folders,
automations, preferences) locally.

## Context

- Maintainer decisions (2026-07-28, recorded in session): no authentication;
  no server state ("only local state on client"); MCP deleted entirely; PDF
  Q&A + its RAG documents store deleted entirely.
- Authoritative reconnaissance maps (exact files/lines/routes/tests):
  - Auth blast-radius map: session scratchpad `auth-removal-map.md`
  - Server-state map: session scratchpad `server-state-map.md`
  - (Scratchpad root: `/tmp/claude-1001/-mnt-ssdvolumes-repo-RustlingPDF/44e49136-ecf6-443e-95c3-98f5925d6849/scratchpad/`)
- Key structural fact: the secured router (`with_reviewed_security`) is
  unreachable in production (the binary refuses secured startup), so the ~14
  subsystems mounted only there delete with near-zero user-visible impact.
- Work branch: `batch7/no-auth-stateless` in worktree
  `/mnt/ssdvolumes/repo/rustling-wt-noauth` (based on `origin/main` @ b48e766,
  i.e. post-rename, post-v2.15.0).

## Scope

In scope:

- Backend: all auth modules (login/session/OIDC/MFA/teams/invites), the
  fail-closed startup guard, all secured-only subsystems (storage vault,
  workflow group-signing + server certificate, policy engine + webhook
  receiver, integrations/purview/resource grants + credential key file, audit
  + portal audit + fleet stats, portal API keys, admin settings/license
  admin/tessdata upload, personal-signatures backend, classification label
  store), MCP (`mcp.rs` + `mcp_oauth.rs`), the server watched-folder daemon,
  SQLite entirely, non-Tauri settings write-backs.
- AI engine: documents/RAG store (sqlite + pgvector + migration bin) and the
  pdf-question-answer capability; audit of other store consumers.
- Frontend/desktop: saas/cloud/portal/portal-saas layers, proprietary auth UI
  and routes, desktop SaaS account link (`commands/auth.rs` + wizard screens),
  apiClientSetup reduced to `X-Browser-Id`.
- Docs/infra: Docker volumes, compose, Taskfile, workflows, contracts
  (~19 delete / ~7 rewrite), PORT_STATUS, ROADMAP, CLAUDE.md, READMEs.

Out of scope (hard keep-list):

- Every PDF operation; PDF password/redact/sanitize (document crypto);
  cert-sign/hardware signing/timestamp/validation (+ their `security.*`
  config keys); SSRF guards (extract `ip_addr_is_reserved` before OIDC module
  deletion); rate limiting (minus auth buckets); mobile-scanner; job manager
  (minus `JobOwner`); `RUSTLING_*`/`STIRLING_*` env aliases; desktop
  Tauri-mode settings write-backs; client-side watched-folder flow (it is the
  replacement, not a casualty).
- Legacy config keys must be ignored-with-warning, never refused (the shipped
  template contains `security.enableLogin: true`; a hard refusal would brick
  every existing desktop install).

## Approach

Single worktree, sequential dev stages (coupled surfaces), then one
adversarial tester over the whole branch with up to 2 fix rounds:

1. Backend removal (rustling-processing) + contracts.
2. AI engine removal (documents store, Q&A) + proxy cleanup.
3. Frontend + desktop removal, first-run collapse to local mode.
4. Docs/infra coherence sweep.
5. Tester: reruns all gates + empirical statelessness proof (fs-diff around a
   real serving session), no-auth 404 probes, survivor-feature probes, sweep
   audit, docs truth spot-checks.

Orchestrated by Workflow run `wf_975b8042-572` (script:
`.../workflows/scripts/no-auth-stateless-wf_975b8042-572.js`).

## Risks And Recovery

- Host process death mid-run: resume with
  `Workflow({scriptPath, resumeFromRunId: "wf_975b8042-572"})`; completed
  stages replay from cache. Check the worktree first
  (`git -C /mnt/ssdvolumes/repo/rustling-wt-noauth log --oneline` + `status`);
  interrupted dev partials stay in place for the next dev to review;
  interrupted tester probes get stashed.
- Accidental deletion of shared plumbing: the keep-list above plus tester
  survivor probes are the guard; any red gate blocks merge.
- Existing installs: config-compat layer (ignore-with-warning) is validated
  empirically by booting against a full legacy settings.yml.
- Rollback: the branch is unmerged until sign-off; `main` stays releasable at
  v2.15.0 throughout.

## Progress

- [x] Reconnaissance maps completed (auth + server-state).
- [x] Maintainer decisions recorded (no-auth, stateless, MCP delete, Q&A delete).
- [x] Worktree + workflow launched (stages 1-4 + tester).
- [x] Stage 1: backend removal green (813+1 tests; boot proofs incl.
      legacy-config ignore + SECURITY_ENABLELOGIN=true no longer refusing).
- [x] Stage 2: engine removal green (115 tests; contradiction/pdf_review
      adapted to per-request content; config-push cache kept with rationale).
- [x] Stage 3: frontend/desktop removal green (1051 vitest; 611+ files
      deleted; desktop local-only; en-US pruned 3,737 dead keys).
- [x] Stage 4: docs/infra sweep green (PORT_STATUS/ROADMAP rewritten; route
      census ≈321 → ≈164; sweep triage zero unclassified).
- [x] Tester sign-off: statelessness proven byte-identical via fs-diff around
      a full workload + reboot; desktop Tauri-mode contrast verified; 41
      removed-route 404 probes; survivor probes green; 5 minors (all dead
      code), 3 fixed by the PM pre-merge, 2 recorded as cleanup follow-ups.
- [x] PM: merged to main as d11491a and pushed (branch diff ≈1151 files,
      −242k lines).
- [x] CI green on main post-merge (Backend/Frontend/Desktop CI all success on d11491a).
- [ ] Release decision: v3.0.0 proposed, awaiting maintainer confirmation.

## Decisions

- 2026-07-28: MCP deleted rather than re-mounted open (maintainer choice over
  the scout's keep recommendation).
- 2026-07-28: PDF Q&A deleted outright rather than converted to a per-session
  ephemeral cache (maintainer choice).
- 2026-07-28: Desktop keeps Tauri-mode `settings.yml` write-backs — the
  sidecar runs on the user's machine, so that state is client-local by
  definition.
- 2026-07-28: Legacy/removed config keys are ignored with a one-line startup
  warning, never refused (desktop-install compatibility).

## Validation

- Focused proof: per-stage gates (workspace fmt/clippy/tests, engine tests,
  `task frontend:check` + build, container src-tauri gate, actionlint).
- Integration or end-to-end proof: tester's statelessness fs-diff (boot, use,
  diff, reboot), no-auth 404 probes across the deleted route census, survivor
  probes (password/cert-sign/SSRF/rate-limit/aliases), desktop smoke
  (Tauri-mode writes still work).
- Repository-required checks: all CI workflows green on the branch push after
  merge; sweep audit with zero unclassified legacy references.

## Result

Verified outcome (2026-07-29): RustlingPDF is a stateless, authentication-free
PDF server. Merge d11491a (≈1151 files, −242k lines net) landed on main with
Backend/Frontend/Desktop CI green. Empirical proofs: a full representative
workload left the server install directory byte-for-byte identical (fs-diff +
sha256; only TTL-bound system-temp job dirs were created) and reproduced after
reboot; 41 removed routes return 404; SECURITY_ENABLELOGIN=true and every
legacy security/mcp/storage/policy key boot with a one-line warning; the
desktop Tauri-mode contrast still creates and stably maintains settings.yml.
Gates: rustling-processing 813+1, rustling-ai-engine 115, catalog 7, tauri
shell 10, vitest 1051, actionlint clean.

Limitations / follow-ups: gated-dead client code remains in the file-manager
and onboarding surfaces (unreachable; cleanup pass recorded in ROADMAP);
Playwright api-stubs still stub deleted auth routes (test scaffolding
cleanup); release with the new surface pending the maintainer's version
decision (v3.0.0 proposed).
