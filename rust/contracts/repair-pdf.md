# `POST /api/v1/misc/repair`

Rust compatibility contract for `RepairController`.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- Success returns `200 OK`, `application/pdf`, as `<base>_repaired.pdf`.
- PDFs that cannot be parsed by the in-process fallback return a route-specific
  `400 Bad Request`. External-tool failures and output write failures return
  `500 Internal Server Error`.

## Repair order

The standalone service retains the exact executable path accepted during startup
dependency discovery:

1. qpdf: `--replace-input --qdf --object-streams=disable <input> <output>`. Exit
   code `3` is accepted as success-with-warnings, matching Java's shared process
   executor.
2. When discovery found no qpdf, parse the PDF object graph and save a normalized
   in-process rewrite. This repairs structural issues the parser can tolerate and
   removes obsolete incremental layout during serialization.

Java tried Ghostscript first and fell back to qpdf. Ghostscript was removed from
this product for its AGPL-3.0-or-commercial licence, so qpdf is now the only
external tier. Inputs that only Ghostscript could rescue are therefore no longer
repairable here; qpdf is bundled with the desktop app, so desktop repair is not
weakened.

If qpdf was discovered but its attempt fails, the route does not silently
substitute the less-capable parser rewrite. qpdf uses a shared Java-compatible
process pool (2 sessions by default), a 30-minute configurable timeout,
concurrent output draining, and child-tree termination on timeout. Embedded/test
router construction does not probe or invoke native tools and therefore retains
deterministic in-process behavior.

The legacy `processExecutor.sessionLimit.ghostscriptSessionLimit` and
`processExecutor.timeoutMinutes.ghostscriptTimeoutMinutes` settings keys are still
accepted and ignored, so existing `settings.yml` files keep booting.

## Verification

The HTTP test verifies multipart handling, output naming, MIME type, successful
reload, and retained page structure after the normalized rewrite. Unit tests
verify qpdf arguments and warning exit-code handling, in-process fallback, and
external failure behavior.
