# Runtime configuration compatibility

Rust owns the public configuration surface used by the unchanged React client. It loads `configs/settings.yml` and then
`configs/custom_settings.yml` below `RUSTLING_BASE_PATH` (or the working
directory when unset). The custom file recursively overrides the base file.
The corresponding all-caps Spring-style environment variables take precedence
for the settings that this slice exposes.

The standalone service binds loopback by default. `RUSTLING_HOST`, followed by the
Spring-compatible `SERVER_ADDRESS`, selects an explicit IPv4 or IPv6 bind address;
`RUSTLING_PORT`, followed by `SERVER_PORT`, selects the port. Port `0` requests an
ephemeral port for desktop startup. A container can set `RUSTLING_HOST=0.0.0.0`
without weakening the loopback default used by local and desktop launches. Present
malformed or non-Unicode bind values fail startup instead of falling back silently.

The conditional SMTP route also resolves the existing `mail.*` tree and its
`MAIL_*` overrides. See [`send-email.md`](send-email.md) for its supported TLS
policy and multipart contract.

## Routes

| Route | Response |
| --- | --- |
| `GET /api/v1/config/app-config` | Public application configuration consumed during UI bootstrap. It includes UI/system toggles, legal links, timestamp presets, startup dependency-probe completion, verified dynamic license fields, and an externally visible `frontendUrl` derived from `Host` plus a safe `X-Forwarded-Proto`. |
| `GET /api/v1/config/login-disclaimer[?lang=<locale>]` | Enabled agreement markdown with locale fallback; always served openly (the legacy `security.enableLogin` key is ignored). |
| `GET /api/v1/config/endpoint-enabled?endpoint=<key>` | JSON boolean for one endpoint key. |
| `GET /api/v1/config/endpoints-enabled?endpoints=<key>,<key>` | JSON map of requested endpoint keys to booleans. |
| `GET /api/v1/config/endpoints-availability[?endpoints=<key>,<key>]` | JSON map containing `{ "enabled": boolean, "reason": null | "CONFIG" | "DEPENDENCY" }`. Without a query it returns the known endpoint key set plus configured disabled keys. |
| `GET /api/v1/config/group-enabled?group=<name>` | JSON boolean for a functional or tool group. |
| `GET /api/v1/settings/get-endpoints-status` | Explicitly disabled endpoint keys mapped to `false`, matching the Java settings controller's status map. |

Endpoint keys are normalized by removing one leading slash. `endpoints.toRemove`
or `ENDPOINTS_TOREMOVE` disables those keys with reason `CONFIG`.
`endpoints.groupsToRemove` / `ENDPOINTS_GROUPSTOREMOVE` follows the Java
`EndpointConfiguration` group map for retained functional groups and the actual
Rust dependency map for tool groups. Removing a functional group disables all
its endpoints; removing a tool group preserves an endpoint when another
registered tool-group alternative is available. Legacy Java-only group names
(`Java`, `Python`, `OpenCV`, `ImageMagick`, `Javascript`, `CLI`, and
`Unoconvert`) remain accepted as configuration strings but are inert:
`group-enabled` returns `false` and they cannot make a Rust endpoint available
or unavailable.

A group's members are the endpoint keys `endpoint_key_for_uri` derives from the
registered routes (`/api/v1/<area>/<endpoint>` yields `<endpoint>`;
`/api/v1/convert/<a>/<b>` yields `<a>-to-<b>`), not the tool's display name. The
group table also carries the SPA tool-registry spellings — `compare`,
`view-pdf`, `multi-tool`, `text-editor-pdf`, the `dev-*-docs` entries — which
match no route and gate nothing, but do appear in the `endpoints-availability`
map the UI reads to decide whether to advertise a tool. Both spellings therefore
coexist where they differ.

RustlingPDF diverges from Java's `EndpointConfiguration` by grouping endpoints
upstream leaves in no group, because an endpoint in no group cannot be disabled
by any group setting and the administrator gets no error saying so:
`redact-execute` under `Security` beside `redact` and `auto-redact`;
`pdf-to-text-editor` and `text-editor-to-pdf` (the derived keys upstream's
`text-editor-pdf` entry never matched), `edit-text`, `remove-image-pdf`, the
`extract-attachments`/`list-attachments`/`delete-attachment`/`rename-attachment`
family, the `extract-csv`/`extract-xlsx`/`fields-with-coordinates` form family,
and the eight `/api/v1/analysis/*` introspection routes under `Other`;
`pdf-to-xlsx` and `svg-to-pdf` under `Convert`; `decompress-pdf` under
`Advance`. The `/api/v1/analysis/*` routes are grouped with `get-info-on-pdf`
because they are the same surface — parse an uploaded PDF, report what is inside
it — and the SPA never calls them, so gating them cannot brick the UI.

Infrastructure stays ungated deliberately: `/api/v1/info/*`, `/api/v1/config/*`
(including the availability map itself), `/api/v1/ui-data/*`,
`/api/v1/settings/*`, the job and file plumbing, the mobile-scanner session
routes, and the AI tool-descriptor route, which `AIENGINE_ENABLED` governs
instead. Gating any of those would let an administrator brick the UI rather than
disable a tool. Nine processing routes are still in no group and so cannot be
disabled by any `groupsToRemove` value — `add-comments`, `extract-bookmarks`,
the six `/api/v1/filter/*` routes, and `split-for-poster-print`;
`every_processing_route_is_reachable_from_some_functional_group` pins that list
so it stays visible.

Adding a key to a group only makes an endpoint *disableable*. A default
`settings.yml` disables nothing, and `url-to-pdf` remains the one route that
ships off, by `system.enableUrlToPDF` rather than by any group.

`system.enableUrlToPDF` also controls both the `url-to-pdf` availability result
and the global API availability interceptor. It remains disabled by default, so
the normal router returns `403 This endpoint is disabled` before a controller
can process it. All `/api/` responses receive `Cache-Control: private, no-store`,
matching Java's `EndpointInterceptor`. The existing
`RUSTLING_PROCESSING_ENABLE_URL_TO_PDF` and `SYSTEM_ENABLE_URL_TO_PDF`
environment aliases take precedence.

## Runtime dependency discovery

The standalone Rust executable probes optional command-line tools once before
accepting requests. The discovery table is the single source of tool-group
names. It resolves configured command overrides and platform `PATH` candidates
for Ghostscript, OCRmyPDF, Tesseract, LibreOffice, WeasyPrint, `pdftohtml`,
QPDF, RAR creation, Calibre, FFmpeg, veraPDF, and RAR extraction through
`unrar` or `7z`/`7zz`. QPDF below 12.0.0 and WeasyPrint below 58.0 are treated
as unavailable, matching Java's minimum-version gates. A 7-Zip-shaped candidate
(any candidate whose `i` output parses as a `7z i` capability listing,
independent of the resolved file name) is accepted only when `7z i` reports
BOTH a RAR *format handler* under `Formats:` AND a RAR *decompression codec*
under the separate `Codecs:` section. Debian's DFSG `7zip` package ships the
format handler (listing/opening a `.rar` container, and extracting stored
entries, both still work) but deliberately excludes the RAR codecs — so a
`Formats:`-only check would wrongly accept it; decompressing a RAR-compressed
entry needs the non-free `7zip-rar` plugin, which adds only the missing
`Codecs:` entries. A candidate whose `i` output does not parse as a 7-Zip
listing at all (genuine `unrar`, which has no `i` subcommand) is assumed
capable, matching `unrar`'s unconditional RAR support. Image-scan extraction
is native and no longer probes Python or OpenCV. Each process probe has a
five-second kill timeout.
Missing groups feed the same endpoint-alternative logic as configured group
removal and surface as reason `DEPENDENCY`; explicit endpoint/group removal
still takes precedence as reason `CONFIG`. The exact executable paths accepted
by discovery are retained for runtime-owned native adapters such as PDF repair,
preventing a later request from resolving a different binary.

Route availability models unconditional requirements. Missing FFmpeg disables
`pdf-to-video`; missing RAR creation/extraction tools disables `pdf-to-cbr` or
`cbr-to-pdf`; and `pdf-to-markdown` is independent of `pdftohtml` because it
uses PDFium with a lopdf fallback. Conditional enhancements do not disable an
otherwise functional route: `verify-pdf` remains available without veraPDF for
inputs that declare no validation profile, while `group-enabled?group=veraPDF`
reports `false` and declared PDF/A, PDF/UA, or WTPDF profiles still receive a
request-time `501`. The same route-level rule keeps native repair/compression
and non-CMYK replace/invert modes available when their optional external tools
are absent. Because neither `qpdf` nor `veraPDF` gates any endpoint in the
tool-group map, `endpoints.groupsToRemove: [qpdf]` and `[veraPDF]` are both a
no-op for endpoint availability today: an operator who previously relied on
`groupsToRemove: [qpdf]` to disable `repair`/`compress-pdf`, or on
`groupsToRemove: [veraPDF]` to disable `verify-pdf`, will find both routes
silently stay enabled — only the corresponding `group-enabled?group=qpdf` or
`group-enabled?group=veraPDF` query still reports `false`.

`pdftohtml` discovery is optional for `pdf-to-html`: a discovered executable is
retained and preferred for compatibility, but its absence no longer disables the
endpoint because the PDFium-backed native renderer is the fallback.

`dependenciesReady` means startup probing has completed, not that every optional
tool is installed. Embedded/test router constructors intentionally remain
process-free; the service binary selects the discovery-enabled constructor.

The PDF editor's predefined Type0 CID mappings are passive data rather than an
executable dependency. It searches the path list in
`RUSTLING_PROCESSING_CMAP_PATH`, then the standard Poppler locations
`/usr/share/poppler/cMap` and `/usr/local/share/poppler/cMap`. The production
image already supplies the first location through `poppler-data`. Missing data
does not prevent startup; affected fonts retain conservative source-code metrics.

## Commercial license configuration

`premium.enabled`, `premium.key`, and `premium.maxUsers` resolve their existing
`PREMIUM_*` environment overrides. A `file:` key is read from the process
working directory when relative. The deprecated `enterpriseEdition.enabled`
and `.key` fields remain a migration fallback when the premium block is disabled
or still contains the zero UUID placeholder.

These values are configuration intent, not entitlement. The open router
reports `runningProOrHigher=false`, `runningEE=false`, and `license=NORMAL`;
it never treats a configured key as verified.

## Timestamp settings

The normal Rust `app()` constructor derives the timestamp allowlist from
`security.timestamp.defaultTsaUrl` and `security.timestamp.customTsaUrls` in
the same YAML configuration. Existing timestamp environment aliases still take
precedence, including `SECURITY_TIMESTAMP_DEFAULT_TSA_URL` and
`SECURITY_TIMESTAMP_CUSTOM_TSA_URLS`.

## Login disclaimer

The public agreement reader resolves locale-specific markdown from
`customFiles/disclaimer` below the same base path and is always served openly;
the disclaimer files are operator-provisioned (there is no runtime mutation
API). See [`login-disclaimer.md`](login-disclaimer.md) for lookup rules.

## Install identity (`AutomaticallyGenerated`)

At startup (before the serving configuration is loaded) the executable
resolves the install identity. Only the desktop (Tauri) sidecar persists it:
under `RUSTLING_PDF_TAURI_MODE=true` the executable runs
`RuntimeConfig::initialize_generated_identity`, the port of Java
`InitialSetup`; every other deployment is stateless and resolves the same
identity in memory per boot (`ephemeral_generated_identity`), still honoring
configured `AutomaticallyGenerated.*` values and the env spellings below
without ever writing. In persistent (desktop) mode an invalid or missing
`AutomaticallyGenerated.UUID` /
`AutomaticallyGenerated.key` is replaced with a fresh RFC 4122 v4 UUID and
persisted into the settings file, and the canonical application version
(`application_version()`, backed by the repo `VERSION` file — the Rust
equivalent of Java's `version.properties`, never the crate version) is
persisted as `AutomaticallyGenerated.appVersion`. The write is
comment-preserving, like Java's snakeyaml writer
(`GeneralUtils.saveKeyToSettings` with `parseComments`/`dumpComments`): only
the targeted value lines change, so a first desktop boot leaves the settings
file byte-identical to the bundled template except the three
`AutomaticallyGenerated` value lines. A previously empty or
`0.0.0` version marks the instance as a new server (Java
`InitialSetup.isNewServer`; the template's shipped placeholder version means
template-created files are *not* "new", matching Java). Values supplied via
Spring's relaxed env spellings (`AUTOMATICALLYGENERATED_UUID`, `…_KEY`,
`…_APPVERSION`) are honored without being written back, like Java's property
binding. Validation matches `UUID.fromString`'s accepted shapes (five
hyphen-separated hex groups); exotic short-group spellings are kept, not
rotated.

Documented divergences: Java rewrites the same values on every boot — the
Rust port writes only when something actually changes, so an unchanged boot
leaves the settings file byte-stable (preserving the desktop template-merge
idempotence); and a failure to persist (e.g. read-only config mount) is
fail-open with a warning and an ephemeral in-process identity, where Java
fails startup. The writer reuses an existing section/key spelling that
differs only by case instead of duplicating it. Hand-edited settings shapes
the comment-preserving editor cannot extend — a flow-collection root or
section value, an `AutomaticallyGenerated` section holding a block sequence
(`- item` children), or an identity leaf holding a block scalar (`UUID: |`)
— are refused with the file byte-for-byte untouched (fail-open, ephemeral
identity), and the edited text must reparse as a YAML mapping before any
byte reaches disk; the stock template contains none of those shapes. Java's `InitialSetup` legal-URL
defaulting (`legal.termsAndConditions`/`privacyPolicy`) is intentionally not
persisted by the Rust port; those defaults are applied at read time.

## Ignored legacy settings

Authentication, MCP, and server-side state were removed from the product.
Their configuration keys (`security.enableLogin`, `security.initialLogin.*`,
`security.oauth2.*`, `security.saml2.*`, `security.jwt.*`,
`security.loginMethod`, `security.databasePath`,
`security.credentialEncryptionKey[Path]`, `mcp.*`, `storage.*`, `policies.*`,
`premium.enterpriseFeatures.audit.*`, `mail.enableInvites`, `app.supabase.*`,
and their env spellings such as `SECURITY_ENABLELOGIN` and
`DOCKER_ENABLE_SECURITY`) are IGNORED with a one-line startup warning for the
notable ones — never refused. Existing installs whose `settings.yml` still
carries these keys (the historic desktop template shipped
`security.enableLogin: true`) keep booting unchanged. The
`security.validation.*`, `security.timestamp.*`, and `security.xFrameOptions`
keys guard non-login features and stay fully honored.

There is no server-side settings mutation API: the analytics-consent endpoint
was removed (consent is client-owned), and settings files are edited by the
operator or, on desktop, by the sidecar's own template/identity maintenance.

## Verification

Unit coverage proves YAML recursive override, legacy/current license resolution,
endpoint normalization and availability (including distinct
configuration/dependency reasons), the discovery-spec/tool-group invariant,
legacy phantom-group inertness, command-override semantics, 7-Zip RAR capability
detection, dependency version parsing, and timestamp configuration extraction.
HTTP integration coverage proves app-config bootstrap fields, host/proxy URL
reconstruction, endpoint availability, group status, batch status, settings
status, interceptor `403`, the API cache policy, and the login-disclaimer route
(including the ignored legacy `security.enableLogin` key).
