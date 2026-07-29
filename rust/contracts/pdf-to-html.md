# `POST /api/v1/convert/pdf/html`

Rust compatibility contract for `ConvertPDFToHtml` (`PDFToFile.processPdfToHtml`).

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- Success returns `<base>ToHtml.zip` (`application/octet-stream`).
- The ZIP remains flat and contains every generated HTML, CSS, and image file.

## Renderer selection

The standalone service discovers `pdftohtml` at startup, using
`RUSTLING_PROCESSING_PDFTOHTML_COMMAND` when set and the platform `PATH`
candidates otherwise.

When a usable executable was discovered, the endpoint preserves the existing
Poppler path and runs:

```text
pdftohtml -c <input> <base>
```

Poppler complex-mode output is preferred for upgrade compatibility. A command
that starts but reports failure remains a server error; it does not silently
switch renderers and conceal a Poppler regression.

When no executable is discovered (including a configured path that does not
exist), the endpoint uses the native PDFium-backed renderer. The selected
backend is recorded in an info log (`pdftohtml` or `native-pdfium`), and the
conversion API returns the backend enum internally for test assertions.

## Native output

The native ZIP contains:

- `<base>.html`, with one fixed-size `<section class="page">` per PDF page, each
  wrapping a single `<div class="page-canvas">`;
- `<base>.css`, containing the page and absolute-positioning styles; and
- deduplicated `<base>_page_<page>_<image>.png` image assets referenced by the
  HTML.

Text is reconstructed into visual lines from PDFium segment/glyph geometry.
Each line carries its page-space position, width, dominant font size, and
majority bold/italic flags where PDFium reports them. The existing Markdown
heading detector is reused to emit `h1`/`h2` elements, and the existing
two-column gutter detector is reused so DOM order is left-column-first and then
right-column while CSS retains visual placement. Source text is escaped with
Ammonia before insertion into HTML.

Top-level page image objects are extracted through the shared PDFium image
extractor, encoded as PNG, deduplicated by decoded pixels, and positioned from
their page-object bounds. Repeated placements reference the same asset.

### Page rotation

There is exactly one coordinate space for content: unrotated PDF user space.
PDFium reports text and image geometry there, so the renderer must not mix it
with PDFium's rotation-aware page extents. Each page therefore carries its
unrotated box plus its intrinsic `/Rotate`, normalised to 0/90/180/270:

- `<section class="page">` is sized to the **rotated** box, so page flow reserves
  the space a viewer shows (a `612 × 792` page with `/Rotate 90` lays out
  `792 × 612`), and records the angle in `data-rotation`;
- the inner `<div class="page-canvas">` is sized to the **unrotated** box and
  carries the single CSS transform that applies the rotation
  (`transform-origin: 0 0` plus `translate(...) rotate(90|180|270deg)`), so text
  and images keep their user-space offsets and stay legible at page scale.

All four rotations are handled, including `/Rotate 180`, which earlier revisions
ignored. Rotation changes neither the offsets of the emitted runs nor their DOM
order.

## Limits

Two groups of limits apply, and they differ in scope and in status code.

**Native-path input limits** — these judge the upload, so they return
`400 Bad Request`:

- pages: 10,000
- reconstructed text lines: 200,000 (from the shared geometry extractor)
- retained text: 32 MiB
- generated HTML: 64 MiB
- extracted images: 10,000
- pixels per decoded image: 25 million
- encoded image bytes: 128 MiB

**Shared output limits** — enforced when the workspace is archived, so they apply
to the **external Poppler path as well as the native one**:

- total flat ZIP entries: 100,000
- total archived output: 200 MiB

Because a legitimate large Poppler conversion can reach these, they are treated
as a server-side capacity policy and return `500 Internal Server Error`; the
request is not blamed for a cap it cannot see or influence. Malformed and
encrypted PDFs remain `400 Bad Request`, and no limit violation panics.

## Explicit native divergences from Poppler `-c`

The native renderer is a faithful approximation, not Poppler parity:

- positioning is per reconstructed visual line, not exact per-glyph placement;
  kerning, character transforms, clipping, and rotated/vertical text can differ;
- source fonts are not embedded or reproduced by family; CSS uses browser
  fallback fonts, and PDFium's weight/style reporting can be incomplete;
- Poppler's complex-mode page/background images are not generated;
- only top-level page image objects are extracted; inline images, image masks,
  images nested inside form XObjects, and some transparency/color effects are
  not reproduced;
- image placement uses an axis-aligned object bounding box, so rotation, skew,
  clipping, and unusual transformation matrices are approximate;
- vector artwork, shadings, patterns, annotations, form controls, and page
  decorations are not converted into equivalent HTML;
- links, tagged-PDF semantics, tables, lists, and reading order beyond the
  existing two-column heuristic are not reconstructed; and
- native output is one HTML file plus one CSS file, so its markup and filenames
  differ from Poppler's implementation-specific complex-mode files.

## Availability and failures

Missing Poppler no longer disables this endpoint. The native renderer requires
the shared PDFium runtime; `501 Not Implemented` is returned only when neither
the preferred executable nor PDFium is available. Malformed, damaged, or
encrypted inputs rejected by PDFium return `400 Bad Request`. Workspace/archive
I/O failures and a discovered Poppler process that fails still return a server
error.

## Verification

Module tests assert renderer selection, native semantic/positioned markup,
escaping, two-column order, a valid multi-page native ZIP, and clean damaged-PDF
failure. HTTP tests cover native page/text/image output, multi-page order,
damaged input, missing-file validation, and installed-Poppler precedence.
