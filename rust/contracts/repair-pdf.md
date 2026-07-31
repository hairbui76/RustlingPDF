# `POST /api/v1/misc/repair`

Current contract for PDF repair.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- Success returns `200 OK`, `application/pdf`, as `<base>_repaired.pdf`.
- PDFs that cannot be parsed by the in-process fallback return a route-specific
  `400 Bad Request`. External-tool failures and output write failures return
  `500 Internal Server Error`.

## Repair order

The service retains the exact executable path accepted during startup
dependency discovery:

1. qpdf: `--qdf --object-streams=disable <input> <output>`. Exit code `3` is
   accepted as success-with-warnings.
2. When discovery found no qpdf, parse the PDF object graph and save a normalized
   in-process rewrite. This repairs structural issues the parser can tolerate and
   removes obsolete incremental layout during serialization.

If qpdf was discovered but its attempt fails, the route does not silently
substitute the less-capable parser rewrite. Embedded/test router construction
does not probe or invoke native tools and therefore retains deterministic
in-process behavior.

The shared process pool allows 2 sessions by default, uses a configurable
30-minute timeout, drains output concurrently, and terminates the child process
tree on timeout. The qpdf arguments preserve structural normalization and write
the repaired document to the caller's output path.

## Verification

The HTTP test verifies multipart handling, output naming, MIME type, successful
reload, and retained page structure after the normalized rewrite. Unit tests
verify qpdf arguments and warning exit-code handling, in-process fallback, and
external failure behavior. A further unit test drives a *real* discoverable qpdf
against a document with a dangling `startxref` and asserts the repaired output
passes `qpdf --check`, so the argument list is validated against the tool rather
than against a shell stub; it skips when no qpdf is installed.
