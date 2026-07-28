# Native desktop processing startup

The Tauri desktop launcher starts the Rust processing backend as its bundled
sidecar by default. `task desktop:stage-sidecar` builds the release
`rustling-processing` binary plus the pinned PDFium runtime and stages them
into `src-tauri/` (`bundle.externalBin` entry `binaries/rustling-processing`,
`bundle.resources` entry `resources/pdfium`); the bundler installs the sidecar
next to the app executable, where the launcher resolves it via the shell
plugin's sidecar API. `STIRLING_NATIVE_BACKEND_PATH` is a development-only
override that points the launcher at an arbitrary processing executable
instead. There is no further fallback: the upstream Java JRE/JAR launch path
has been removed, and a bundle without the sidecar fails startup with a
reported error.

The native path provides:

- an unconditional `Stirling-PDF running on port: <port>` handshake even without `RUST_LOG`;
- an ephemeral loopback port, bounded 90-second launcher wait, stderr/stdout handshake parsing,
  early-exit reporting, stale-process protection, and stale-port cleanup;
- desktop/base/config/log/work environment parity and legacy-workspace migration
  (the backend itself reads only `STIRLING_BASE_PATH` and
  `STIRLING_PDF_TAURI_MODE` from this set; the Java-era
  `STIRLING_PDF_CONFIG_DIR`/`LOG_DIR`/`WORK_DIR` variables are still passed for
  contract parity and are harmless);
- PDFium wiring: when the launcher's own environment does not already carry
  `STIRLING_PDFIUM_LIBRARY_PATH` (an operator-set value is inherited untouched)
  and the bundle ships `resources/pdfium`, the launcher sets
  `STIRLING_PDFIUM_LIBRARY_PATH` to that directory — the backend resolves the
  platform library filename inside it. In unpackaged development runs the
  variable stays unset (logged) and the backend falls back to a system PDFium;
- PID-plus-start-time parent monitoring through `TAURI_PARENT_PID`, with orphan shutdown normally
  observed within one 250 ms poll interval;
- fresh-install configuration initialization in Tauri mode: the packaged Java
  `settings.yml.template` is atomically persisted only when `configs/settings.yml` is absent, and
  an empty `custom_settings.yml` is created only when absent;
- short-file backup recovery: a `settings.yml` shorter than `MIN_SETTINGS_FILE_LINES` (31) is
  treated as truncated/corrupted, moved aside to `settings.yml.<epoch-millis>.bak`, and recreated
  from the template (`custom_settings.yml` is never subject to this);
- upgrade-time template merge: when `settings.yml` already exists and is long enough, any keys the
  bundled template has gained across app versions are folded into the user's file while their
  customized values are preserved.

## Upgrade-time template merge

Matches Java's `ConfigInitializer` upgrade path (and the `YamlHelper` it drives):

- the output is **template-shaped** — the template's structure, comments, blank lines and inline
  comments are kept verbatim, not the user's;
- for each leaf key present in **both** files, the template's default value is replaced by the
  **user's** value, keeping the template's inline comment;
- brand-new template keys absent from the user file keep their template **default**;
- user keys **absent from the template are dropped** (the merge walks the template, so unmatched
  user keys are never carried);
- the file is rewritten **only when the merged result differs** from what is on disk, so re-running
  on an already-current file is a no-op (idempotent).

**Value rendering (quoting).** A carried-over value is re-emitted in the template leaf's own quoting
style: a double- or single-quoted template value keeps that style, and a **plain-styled** value is
emitted as an inline scalar that reparses to **exactly** the user's value. A plain value that is not
plain-safe — one carrying `#`, `:`, `*`, `!`, `@` or another leading/embedded indicator, a
leading/trailing space, an empty string, or text that would otherwise reparse as a bool/number/null
(`true`, `123`, `null`) — is **automatically quoted** (the decision is delegated to serde_yaml's own
scalar emitter, not a hand-maintained character list). So a real database password or secret is never
silently truncated at an inline `#` comment and the file always reparses; a plain-safe value
(`postgres`) still renders bare, with no quoting churn. This matches Java's snakeyaml, which likewise
quotes such values on write — there is no plain-scalar corruption on carry-forward.

`custom_settings.yml` is never merged. Java's two historical `migrate*` key renames
(`migrateEnterpriseEditionToPremium`, `migrateProFeaturesKeyCasing`) are intentionally **not**
ported — they are Java-schema-specific migrations, out of scope.

**Documented scope limitation (follow-up):** the merge carries across only values that live inline
on their key's line — scalars and inline flow sequences (`[]`, `[a, b]`). The template currently has
**no block sequences**, so this covers effectively the whole file; but a user override expressed as a
nested mapping (or a block sequence) under a key is not carried, and that key falls back to the
template default. A `settings.yml` that is long enough to reach the merge path but no longer parses
as YAML is left untouched (a warning is logged) rather than failing desktop startup — Java throws
here; the Rust port prefers not to regress a previously-tolerated file into a hard boot failure.

The desktop updater endpoint points at RustlingPDF's own releases
(`https://github.com/hairbui76/RustlingPDF/releases/latest/download/latest.json`). The committed
`updater.pubkey` is a repo-controlled key (minisign id `9ADA2DC8FC4FAF0B`); the private key is held
outside the repository (maintainer machine) and as the `TAURI_SIGNING_PRIVATE_KEY` GitHub Actions
secret (empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). If the key is ever lost, installed apps only
accept updates signed by the committed pubkey — generate a new pair and ship it in a manually
distributed build.

**Signed-bundle upgrade proof — Linux leg proven** (2026-07-28, via a containerized e2e harness
that was subsequently removed by maintainer decision — the harness and its runs are preserved in
git history before commit `9f42a3d`): a release v0.0.1 AppImage
carrying the Rust sidecar, built against a throwaway dev signing key and a localhost update
endpoint, detected a served signed v99.0.0 update (`check_for_update` → 99.0.0), **rejected** a
manifest signed by a different valid key ("signature was created with a different key") and a
byte-tampered artifact under the good signature ("signature verification failed") — both leaving
the installed AppImage untouched — then downloaded, signature-verified, and installed the good
update (on-disk AppImage byte-identical to the served artifact, sha256-asserted) and reported
99.0.0 after relaunch. The verifying key was cryptographically confirmed to be the dev throwaway
key (minisign id match between the served signature and the pubkey pinned in the app config).
macOS and Windows legs remain — they are release-runner work, not runnable on this host.
