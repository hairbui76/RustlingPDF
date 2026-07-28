# Single-binary SPA serving contract

Rust compatibility contract for serving the built React SPA directly from the
processing binary, porting the Java `ReactRoutingController` and the
`WebMvcConfig` static-resource pipeline. Implemented in
`crates/rustling-processing/src/spa.rs` as a config-gated router fallback.

## Gate

- `RUSTLING_FRONTEND_DIST` (env) or `system.frontendDist` (settings) names the
  absolute path of a built Vite `dist/` directory. Upstream has **no**
  equivalent Spring property: the Java build bakes the dist onto the servlet
  classpath, so this key is owned by the Rust runtime.
- Unset (the default): the module is completely inert. No fallback is attached
  and unmatched requests keep axum's plain empty `404`, so the Vite dev-proxy
  workflow (`task frontend:dev` against the backend) is byte-for-byte
  unchanged.
- Set: a fallback handler is attached at the router-assembly funnel
  (`ProcessingRuntime::into_router`), covering both the OSS and the
  reviewed-security routers. Explicitly registered routes — all of `/api/**`,
  `/health`, `/robots.txt`, MCP/OIDC, everything — always win over the
  fallback by construction; only unmatched requests reach the SPA layer. The
  transport DoS guardrails (timeouts, concurrency cap, per-IP rate limits)
  wrap the fallback like every other route.

Only `GET`/`HEAD` reach the SPA logic; other methods on unmatched paths keep
returning `404` (divergence: servlet Spring would answer `405` for a mapped
GET-only path — documented, not ported).

## Index page (`ReactRoutingController` parity)

- `GET /` and `GET /index.html` serve the SPA index:
  `200`, `Content-Type: text/html; charset=utf-8`,
  `Cache-Control: no-cache, must-revalidate`.
- Resolution order, cached once at startup exactly like the Java
  `@PostConstruct`: `customFiles/static/index.html` (the
  `InstallationPathConfig.getStaticPath()` override) first, then
  `<dist>/index.html`, then an embedded lightweight fallback page (the port of
  `buildFallbackHtml`, including the desktop SSO deep-link script) when
  neither exists. Changing index.html on disk requires a restart, as upstream.
- Transformation before serving (`processIndexHtml` parity, context path fixed
  to `/`): `%BASE_URL%` → `/`, any existing `<base href="…">` tag is rewritten
  to `<base href="/" />`, and `<script>window.RUSTLING_PDF_API_BASE_URL =
  '/';</script>` is injected before `</head>`.
- The upstream SaaS landing-page swap at `/` is out of scope (no saas module).

## Deep links and forwards

- `GET /auth/callback`, `GET /share/{token}` (token may contain dots), and
  `GET /mobile-scanner` serve the SPA index.
- `GET /auth/callback/tauri` serves the embedded standalone OAuth-completion
  page (the port of `buildCallbackHtml`; forwards tokens/errors to the desktop
  app via the `stirlingpdf://` deep link). Upstream sets no cache-control
  header here; neither do we.
- `/mobile-scanner` in desktop mode (`RUSTLING_PDF_TAURI_MODE=true`, captured
  at startup): serves `mobile-upload.html` (external `customFiles/static/`
  first, then the dist) when present, because a phone scanning the QR cannot
  load the SPA route from the Tauri webview. The RustlingPDF frontend does not
  currently bundle `mobile-upload.html`, so this path serves the SPA index
  until it does (documented gap; the lookup semantics are already in place).
- Generic client-route forwards mirror the two upstream regex mappings:
  - one dot-free segment whose name does not **start with** any of
    `api static pipeline pdfjs pdfjs-legacy pdfium vendor fonts images css js
    assets locales modern-logo classic-logo Login og_images samples`
    (prefix semantics and case-sensitivity are deliberate — the Java negative
    lookahead `(?!api|…)` also blocks e.g. `jsx-tool`, and forwards `/login`
    while excluding `/Login`);
  - two dot-free segments whose first segment is not excluded (this is what
    lets `/files` and `/files/{uuid}` reach the SPA file manager).
  - Anything else — deeper paths, dotted names, excluded prefixes — is a
    static-file lookup and 404s when no file exists. `GET /audit` lands on the
    single-segment forward and serves the SPA index: the OSS upstream build
    has no audit web controller (the proprietary dashboard is secured-mode
    only), and its effective behavior resolves to the SPA shell.

## Static files (`WebMvcConfig` parity)

- Files are served from the dist root with extension-derived MIME types and
  the upstream cache tiers:
  - `sw.js`, `manifest.json`, `site.webmanifest`, `browserconfig.xml` →
    `no-store`;
  - `assets/**`, `images/**`, `fonts/**` →
    `max-age=31536000, public, immutable`;
  - `favicon.*`, `apple-touch-icon.png`, `android-chrome-*.png`,
    `mstile-*.png`, `safari-pinned-tab.svg`, `3rdPartyLicenses.json`,
    `manifest-classic.json`, and `icons|modern-logo|classic-logo|pdfjs|
    pdfjs-legacy|pdfium|locales|css|js|vendor|samples|og_images|Login/**` →
    `max-age=86400, public, stale-while-revalidate=604800`;
  - everything else → `no-cache`.
- Precompressed variants (Vite emits `.br` siblings; `.gz` also honoured) are
  negotiated via `Accept-Encoding` with `q=0` opt-out, served with
  `Content-Encoding` and the original file's MIME type
  (`EncodedResourceResolver` parity). `Vary: Accept-Encoding` is set on file
  responses.
- `Last-Modified` is emitted and `If-Modified-Since` answers `304` with
  second granularity.
- Divergence: upstream also layers `customFiles/static/` over the classpath
  for **all** static assets; the Rust port honours the external override for
  `index.html` and `mobile-upload.html` only. Range requests are not
  implemented.

## Hardening

- Request paths are split and percent-decoded per segment; empty segments
  (`//`, trailing `/`), `.`/`..` dot-segments, decoded `/`, `\`, NUL bytes,
  and non-UTF-8 escapes are rejected with `404` before any filesystem access.
- Every served file (including compressed siblings) is canonicalized and must
  remain physically below the canonicalized dist root — symlinks pointing
  outside the dist (files or directories) 404. Symlinks that stay inside the
  dist keep working.
- Directories are never listed or served; segments containing `:` are refused
  at file lookup (Windows drive/ADS names).
- Only regular files are served; missing files 404 with no body.

## Tests

- `crates/rustling-processing/tests/spa_serving_endpoint.rs` — synthetic-dist
  integration suite: inertness when unset, index transformation, deep links,
  API/`/health`/`/robots.txt` precedence, cache tiers and MIME types,
  precompressed negotiation, `304` revalidation, traversal/encoded-traversal/
  symlink attacks, directory requests, forward-exclusion quirks, non-GET
  methods, missing-index fallback page, `customFiles/static/index.html`
  override.
- `crates/rustling-processing/src/spa.rs` unit tests — path sanitizer,
  classifier, cache tiers, index transformation, `Accept-Encoding`
  negotiation, MIME table, and the desktop-mode `/mobile-scanner` variants.
