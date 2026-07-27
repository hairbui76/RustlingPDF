# Administrator settings and server certificate

One contract for two administrator-only lifecycle surfaces that the code keeps
as sibling modules and mounts together on the secured router:
`admin_settings.rs` (upstream Stirling-PDF's Java `AdminSettingsController`,
upstream `app/proprietary/.../security/controller/api/`) and
`server_certificate.rs` (upstream Java `ServerCertificateController`). They are documented together because
both are `/api/v1/admin/*` configuration surfaces registered side by side in
`lib.rs`; the modules themselves are cleanly separate, so this file has two
sections rather than two files.

All routes below require an authenticated administrator:
`security_policy::is_administrator_path` covers every `/api/v1/admin/` path,
matching Java's class-level `@PreAuthorize("hasRole('ADMIN')")` on both
controllers. Non-admins get `403`, unauthenticated callers `401`.

## Section 1 — `/api/v1/admin/settings` (bounded YAML delta mutation)

The live runtime remains immutable: successful writes update the settings
YAML file **and** a process-local pending delta, then take effect only after
restart. This matches Java's restart-pending semantics.

| Method | Path | Java counterpart | Behavior |
| --- | --- | --- | --- |
| `GET` | `/api/v1/admin/settings?includePending=` | `AdminSettingsController.getSettings` | Full masked settings tree; `includePending=true` overlays the pending delta. |
| `PUT` | `/api/v1/admin/settings` | `AdminSettingsController.updateSettings` | JSON `{ settings: { "dot.path": value, … } }`; responds with Java's "Successfully updated N setting(s)… restart" message. |
| `GET` | `/api/v1/admin/settings/delta` | `AdminSettingsController.getSettingsDelta` | `{ pendingChanges (masked), hasPendingChanges, count }`. |
| `GET` | `/api/v1/admin/settings/section/{section}?includePending=` | `AdminSettingsController.getSettingsSection` | One allowlisted section; pending keys appear under `_pending`. |
| `PUT` | `/api/v1/admin/settings/section/{section}` | `AdminSettingsController.updateSettingsSection` | JSON object of section-relative keys. Setting a nonblank `premium.key` also implies `premium.enabled=true`, as in Java. |
| `GET` | `/api/v1/admin/settings/key/{key}` | `AdminSettingsController.getSettingValue` | `{ key, value }`; unknown keys are `400`. |
| `PUT` | `/api/v1/admin/settings/key/{key}` | `AdminSettingsController.updateSettingValue` | JSON `{ value }`; plain-text success message. |
| `POST` | `/api/v1/admin/settings/restart` | `AdminSettingsController.restartApplication` | See divergence below. |

After a **successful bulk save** (`PUT /api/v1/admin/settings`) that touched
any `aiEngine.*` key, the processor forwards the accumulated pending
`aiEngine.*` changes to the engine's `POST /api/v1/config` — Java
`AdminSettingsController.maybePushAiEngineLive` → `AiEngineConfigSync.pushLiveAfterSave`.
Exactly like Java, section (`PUT …/section/{section}`) and single-key
(`PUT …/key/{key}`) saves do **not** trigger the push; their values still ride
along on the next bulk save because the accumulated pending delta is sent.
The push is best-effort and asynchronous: it can never fail or delay the save
response. See `contracts/ai-proxy.md` ("Processor→engine config push") for the
payload rules and the startup push.

That is 5 `.route()` registrations / 8 method+path pairs (PORT_STATUS's
"delta/section/key (5)" counts the same five registrations, `restart`
included). The sibling read-only
`GET /api/v1/admin/settings/policies/implied-folder-roots` shares the path
prefix but is registered in `policy_http.rs` and documented in
`contracts/policy-config.md`, not here.

**Allowlisting and bounds.** Section names must be one of the 14 canonical
sections (`security`, `system`, `ui`, `endpoints`, `metrics`, `mail`,
`storage`, `premium`, `processexecutor`, `autopipeline`, `legal`, `telegram`,
`aiengine`, `mcp`), case-insensitively. Keys are trimmed, at most 256 bytes
and 10 dot-parts; values at most depth 10, 2,048 collection items, and 64 KiB
per string; request bodies at most 256 KiB and the settings file at most
2 MiB. Violations are `400`.

**Secret masking.** Every read path masks sensitive values as `********`
before serialization — field names matching the sensitive list (`password`,
`dbpassword`, `mailpassword`, `smtppassword`, `clientsecret`, `apisecret`,
`secret`, `apikey`, `accesstoken`, `refreshtoken`, `token`, `key`,
`enterprisekey`, `licensekey`, plus any field name containing `password` or
`secret`) are masked wherever they appear, including inside the pending delta
and single-key reads. One deliberate exemption, matching Java: `premium.key`
is **not** masked, so admins can read back the license key they configured.
Other secrets are never emitted by this module.

**Write path.** Updates are validated, persisted to the YAML file under a
serialized write lock (read-modify-write of the on-disk file), and then
recorded in the in-memory pending map. `persist_immediate` is an internal
variant used by license activation to write without joining the
pending-restart delta, matching Java's immediate application of those values.

**Comment preservation.** The file write goes through the shared
comment-preserving settings editor (`settings_yaml`, the same machinery the
install-identity writer uses), not a serde round-trip: only the targeted
value lines change and every other byte — comments, blank lines, key order,
quoting style — survives each save, matching Java's snakeyaml
`parseComments`/`dumpComments` persistence (`GeneralUtils.
updateSettingsTransactional` via `YamlHelper`). Consequences, each pinned by
tests:

- Existing keys are rewritten in place (ASCII-case-insensitively); missing
  keys — at any dotted depth — are inserted at the end of their nearest
  existing ancestor mapping, creating intermediate openers as needed. (Java
  only updates keys already present in the file; inserting missing ones is a
  deliberate, kept Rust improvement.)
- A JSON **object** value is decomposed into one nested leaf per scalar with
  **merge** semantics: sibling keys already in the file survive (the serde
  writer used to replace the whole subtree). This is a deliberate, documented
  divergence from Java's `YamlHelper.updateValue`, which replaces the whole
  subtree with a freshly built block `MappingNode`. Object keys must be
  plain-safe path segments (ASCII alphanumeric/`_`/`-`, no `.`).
- Values with no single-line inline YAML rendering — multi-line strings,
  empty objects, arrays containing objects — are rejected `400 Invalid`, the
  same refusal `settings_yaml` applies everywhere.
- A settings file whose root is a populated flow collection (`{a: 1}` /
  `[…]`) or not a mapping at all cannot be edited in place: the save fails
  `500 { "error": "the settings file cannot be updated in place: …" }` and
  the file is left byte-for-byte untouched (the serde writer used to corrupt
  or silently replace such files). A file that is exactly `{}` — the old
  serde writer's empty output — is treated as empty and rebuilt as a block
  mapping. Replacing an existing nested block mapping with a scalar leaf is
  refused the same way.
- Hand-edited block shapes the inline editor cannot rewrite refuse the same
  `500` way with the file untouched: a targeted leaf (or any key on the
  path) holding a **block scalar** (`key: |` / `key: >` — rewriting only the
  indicator would fold the continuation lines into the new value, silently
  persisting the wrong data; Java's snakeyaml replaces the whole scalar node,
  so refusal is a deliberate conservative divergence), and a mapping on the
  path holding **block-sequence** children (`- item` lines that inserted
  mapping keys cannot join). The stock settings template contains neither
  shape.
- Before any byte reaches disk the edited text is reparsed: it must still be
  a YAML mapping **and every targeted leaf must read back as exactly the
  requested value** (a final proof against silent wrong-value persistence);
  a failed proof is the same `500` with the file untouched. The write itself
  keeps all prior hardening (symlink refusal, 2 MiB cap, `0o600` mode,
  fsync, serialized write lock). Lines inserted into a CRLF file use CRLF,
  so a save never introduces mixed line endings.
- A single batch whose entries target the **same leaf with conflicting
  values** (e.g. an object value `security.oauth2: {clientSecret: X}`
  alongside the dotted key `security.oauth2.clientSecret: Y`) fails the
  read-back proof — the same `500` with the file untouched — because both
  values are applied and at most one can read back. Java's
  `YamlHelper.updateValue` applies updates sequentially so the last one wins;
  the Rust behavior is a deliberate fail-closed divergence (a conflicting
  batch is a caller bug, and refusing beats persisting an
  order-dependent winner).

**Restart divergence.** `POST /restart` intentionally returns
`503 { "error": "In-process restart is unavailable…" }` instead of Java's
Spring-context restart: the Rust process expects its supervisor (container
runtime, systemd) to restart it. This is a deliberate operational divergence,
not a gap in route coverage.

## Section 2 — `/api/v1/admin/server-certificate` (PKCS#12 lifecycle)

The server-held signing certificate used by workflow signing's
server-certificate mode (`contracts/workflow-signing.md`). Configured by
`system.serverCertificate.{enabled, organizationName, validity,
regenerateOnStartup}` (env `SYSTEM_SERVERCERTIFICATE_*`). At startup an
enabled service generates a keystore if missing (or unconditionally when
`regenerateOnStartup` is set) and otherwise proves the existing one loads.

| Method | Path | Java counterpart | Behavior |
| --- | --- | --- | --- |
| `GET` | `/api/v1/admin/server-certificate/info` | `ServerCertificateController.getServerCertificateInfo` (`/info`) | `{ exists, subject, issuer, validFrom, validTo }`. |
| `POST` | `/api/v1/admin/server-certificate/upload` | `ServerCertificateController.uploadServerCertificate` | Multipart `file` (`.p12`/`.pfx` extension required, 1 byte–4 MiB) + `password` (≤1 KiB UTF-8). |
| `DELETE` | `/api/v1/admin/server-certificate` | `ServerCertificateController.deleteServerCertificate` | Removes the managed keystore; plain-text success message. |
| `POST` | `/api/v1/admin/server-certificate/generate` | `ServerCertificateController.generateServerCertificate` | Generates a fresh RSA-2048 self-signed certificate (CN "<org> Server", non-CA, digital-signature key usage, configured validity days). |
| `GET` | `/api/v1/admin/server-certificate/certificate` | `ServerCertificateController.getServerCertificate` | DER download as `application/pkix-cert`, `server-cert.cer`; `404` when absent. |
| `GET` | `/api/v1/admin/server-certificate/enabled` | `ServerCertificateController.isServerCertificateEnabled` | Bare JSON boolean. |

**Re-wrap under a server-held password.** An uploaded PKCS#12 archive is
first validated as a usable signing key, then its private-key chain is
re-written into a fresh keystore encrypted with the **server's own** random
password (created once, stored beside the keystore as a private file, held
zeroized in memory). The admin's upload password is used only to open the
upload and is never persisted, so the at-rest keystore never depends on a
user-chosen secret. Uploads with no private-key chain or an empty certificate
list are `400`. All operations serialize on one lock; keystore/password files
are written via private-permission atomic replacement.

Uploaded archives are recorded into the request's `SecurityAuditContext`
(name, size, type) at the same boundary as other file uploads.

Error mapping: disabled feature `403`-shaped error response, missing
certificate `404` (download) or `exists: false` (info), invalid
archive/password `400`, generation/IO failures `500`.

## Verification

`admin_settings.rs` `mod tests` covers masked reads (tree, section, delta,
single key), pending overlay and `_pending` sections, allowlist and bound
rejection, the premium-key implication, serialized persistence, and the
restart divergence. `server_certificate.rs` `mod tests` covers startup
generation/regeneration, upload validation and re-wrap, info parsing, DER
download, delete, and disabled behavior. Administrator gating for
`/api/v1/admin/*` (401/403 matrix) is proven by the secured-router policy
tests in `security_policy.rs` / `security_http.rs`.

## Open questions

- Java's `AdminSettingsController` response shapes for error cases (e.g.
  unknown key) were not diffed field-by-field; Rust uses
  `400 { "error": … }` JSON. If a consumer depends on Java's exact error
  bodies, that comparison still needs doing.
