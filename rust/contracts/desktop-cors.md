# Desktop cross-origin (CORS) contract

The processing service sends CORS headers in exactly one situation: when it
runs as the desktop (Tauri) sidecar. Every other deployment behaves as if this
layer did not exist.

## Why the desktop needs it

The desktop shell loads its UI from the webview's own protocol and then calls
the sidecar at an absolute `http://127.0.0.1:<ephemeral-port>` base URL, so
every request is cross-origin. The SPA additionally sends an `X-Browser-Id`
request header on every call (`info-metrics.md`), which is not
CORS-safelisted, so each request is preceded by an `OPTIONS` preflight. Tool
routes are registered for `POST` only, so without this layer the preflight is
rejected with `405` and no `Access-Control-Allow-Origin`, and the desktop UI
reports a bare "Network error" for every operation.

## Activation

CORS is installed only when `RUSTLING_PDF_TAURI_MODE` is the exact
(case-insensitive, trimmed) value `true`, which the desktop launcher sets on
the sidecar it spawns (`desktop-native-startup.md`). This is the same switch
that governs SPA mobile-scanner behaviour, hardware signing, and the settings
bootstrap, and all four read one shared definition.

When the switch is absent or not `true` — every server, container, and
self-hosted deployment — no `Access-Control-*` response header is emitted on
any route, and `OPTIONS` on a `POST`-only route still returns `405`. Those
deployments serve the SPA same-origin from the same binary
(`spa-serving.md`) and need no CORS.

## Allowed origins

An exact byte-for-byte list, with no wildcard, no subdomain matching, and no
suffix matching. The list is **gated to the platform the binary is built for**,
because Tauri picks the webview origin at compile time and the sidecar is built
per platform (`stage-sidecar.sh` stages the host triple from `rustc -vV`, and
the desktop release workflow runs a per-OS matrix):

| Build target | Allowed origins |
| --- | --- |
| Linux, macOS, iOS | `tauri://localhost` |
| Windows, Android | `http://tauri.localhost`, `https://tauri.localhost` |

The `tauri.localhost` forms are not a stylistic variant. They are `wry`'s
workaround for WebView2 and Android being unable to navigate a non-standard
scheme at all, where `tauri://…` is rewritten to `http(s)://tauri.localhost/…`.
WebKitGTK and WKWebView handle the real `tauri://` scheme natively and never
present them.

A Linux or macOS build must therefore **not** carry the Windows origins.
`.localhost` resolves to loopback in Chrome and Firefox, so
`http://tauri.localhost` is really `127.0.0.1:80`; any local process already
serving script-capable HTML on port 80 (a `docker -p 80:80`, an nginx dev
stack, a reflected XSS in a local app) would otherwise be an allowed origin
able to read this service's responses.

`Origin: null` is **not** allowed — sandboxed iframes and `data:` documents
present it, and an opaque origin cannot be authenticated.

## Why the policy must stay narrow

The service has **no authentication of any kind** and listens on loopback. A
permissive policy — `CorsLayer::permissive()`, or any blanket allowance of
`http://localhost:*` or `http://127.0.0.1:*` — would let any web page the user
happens to visit issue cross-origin requests to their local service **and read
the responses**, i.e. read, convert, and exfiltrate their local documents.
Before this layer existed that was impossible precisely because no CORS headers
were ever sent, so the browser withheld every response. Restoring desktop
function must not trade that away. Do not widen this policy.

Credentials are not allowed. Nothing in the service reads a cookie or an
`Authorization` header, and the SPA does not set `withCredentials`, so the
browser's stricter credentialed-CORS rules never apply.

`Access-Control-Allow-Private-Network` is answered for Chromium's Private
Network Access preflight, but only for an origin that is already on the
allow-list. A rejected origin, and a request carrying no `Origin` at all, get
no private-network header — the service does not advertise its reachability to
callers this policy just refused.

## Allowed request headers

`accept`, `content-type`, `x-browser-id`. `x-browser-id` is required: it feeds
the in-memory active-browser count behind `/api/v1/info/load/*`
(`info-metrics.md`), which would silently read zero without it.

## Exposed response headers

A browser lets cross-origin script read only the CORS-safelisted response
headers, so every other header the UI needs is named explicitly. A missing
entry fails silently — the header arrives on the wire and the browser hides it.

- `content-disposition` — the backend's download filename. Without it, tools
  that preserve the backend filename quietly fall back to a generated one.
- `x-rustling-conversion-engine`, `x-rustling-conversion-degraded`,
  `x-rustling-conversion-warnings`, `x-rustling-conversion-warning-detail` —
  the Office pipeline's only channel for reporting a lossy conversion such as
  dropped slides, because the response body is an opaque PDF
  (`file-to-pdf.md`). Not exposing these would make a silent-data-loss signal
  silently unavailable.
- `x-rustling-tool-report` — the PDF comment agent's structured report, out of
  band for the same reason (`pdf-comment-agent.md`).
- `retry-after` — sent with the job queue's `503` so a client can back off
  (`job-management.md`).

`x-job-id` is deliberately not exposed, but the reason is narrow. The service
sets it only on `/api/v1/pdf-text-editor/metadata`, where it is header-only and
absent from the JSON body; that is safe today only because the SPA never calls
that endpoint, using `pdf-text-editor?async=true` instead, which returns
`jobId` in the body. Wiring the metadata endpoint into the UI requires adding
`x-job-id` here, or it will work same-origin and fail on desktop only.

## Methods and preflight

`GET`, `HEAD`, `POST`, `DELETE`; `OPTIONS` is answered by the layer itself.
The layer is the outermost element of the assembled router, so it wraps every
route, both fallbacks, and the transport guardrails' own rejections, and
answers a preflight with `200` before method routing can reject it. Preflight
results may be cached for 600 seconds.

## Known limitation: `task desktop:dev`

`task desktop:dev` points the webview at the Vite dev server
(`http://localhost:5173`) while the SPA still calls the sidecar over an
absolute loopback URL, so that workflow is not covered by this layer and still
fails its preflights. `http://localhost:5173` is deliberately not allowed:
sidecar staging installs the **release** binary even for `tauri dev`, so no
compile-time gate could keep such an exception out of a packaged build, and
5173 is the default port for every Vite project on the machine. Web
development (`task dev`) is unaffected because Vite proxies `/api`
same-origin.
