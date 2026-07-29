# `POST /api/v1/general/crop`

Rust compatibility contract for `CropController.cropPdf()`.

## Request

- Content type: `multipart/form-data`
- `fileInput`: one non-empty PDF, required
- `autoCrop`: boolean, default `false`
- Manual mode requires finite `x`, `y`, `width`, and `height` floating-point
  values. As in Java, the same rectangle is applied to every page.
- `removeDataOutsideCrop`: boolean, default `true` (see
  [Out-of-crop content removal](#out-of-crop-content-removal) — it is a data
  removal promise, not an optimisation)
- Automatic mode ignores manual coordinates. It renders each page at 150 DPI
  with the pinned PDFium runtime, considers RGB values of at least 250 white,
  samples every second pixel above 2,000 pixels in either dimension, and maps
  detected bounds back to PDF coordinates using the Java formulas. A configured
  but broken PDFium runtime is a server error; an unconfigured development
  fallback reports `501 Not Implemented` for this rendering-only branch.

## Out-of-crop content removal

`removeDataOutsideCrop=true` (the default, and what the SPA sends because it never
sets the field) **physically discards** page content that falls outside the crop
rectangle. It is a privacy promise, not a file-size optimisation, and the port is
explicit about exactly how much it promises.

### What is removed

Every page object whose bounding box lies entirely outside the crop rectangle —
text runs, paths, images, and Form XObject invocations alike — is deleted from the
page through PDFium, and the page's content stream is regenerated without it. The
deleted marks are gone from the saved bytes: their text is no longer extractable
and their image samples are no longer present. Annotations are dropped for every
page by the rebuild step that follows.

### What is NOT removed

A mark that **straddles** the crop boundary is kept whole. A text run that starts
inside the crop rectangle and continues past its edge keeps all of its glyphs,
including the ones outside; the same holds for a path or an image that overlaps the
edge, and for the interior of a Form XObject whose own bounding box overlaps the
edge. Such content is clipped at render time but remains in the file.

This is not a regression against the Ghostscript branch this replaces: it is the
same rule Ghostscript applied. `pdfwrite -dUseCropBox` culled marks that missed the
crop box and passed straddling text runs through unsplit, because `pdfwrite` cannot
split a glyph run at a clip edge either. The port reproduces that behaviour rather
than claiming a stronger guarantee than the endpoint has ever delivered.

### When the flag is not set

`removeDataOutsideCrop=false` only rebuilds the pages with the new media box and a
clipping path. Nothing is deleted: the original marks stay in the file, hidden but
fully recoverable. Callers that need the data gone must leave the flag at its
default.

### Automatic mode

`autoCrop=true` ignores `removeDataOutsideCrop` and always takes the clip-only path,
as it did before Ghostscript was removed — the flag has only ever been wired to the
manual-coordinate branch. Automatic mode therefore hides out-of-crop content rather
than deleting it. Callers who need removal must pass explicit coordinates.

### Requirements

Content removal runs on the pinned PDFium runtime — the same runtime automatic
detection already requires. A development environment without a configured PDFium
library returns `501 Not Implemented` for `removeDataOutsideCrop=true`; a configured
but broken runtime, or a removal failure, returns a server error. The endpoint never
silently falls back to the clip-only path, because that would answer a request to
delete data with a file that still contains it. Packaged environments install the
pinned native revision, so this is a development-only boundary.

Ghostscript is no longer involved in any form:
`RUSTLING_PROCESSING_GHOSTSCRIPT_COMMAND` is still accepted as an environment
variable and ignored.

## Response

- Success: `200 OK`, `Content-Type: application/pdf`
- Download name: `<base>_cropped.pdf`
- Rebuilt pages have media boxes `[x, y, x + width, y + height]`, clip source
  content to the same rectangle, and remove stale AcroForm/outlines associated
  with replaced pages.

## Verification

Endpoint tests cover multi-page manual crop geometry and clipping, response
naming, missing-coordinate validation, and native PDFium automatic detection
against rendered black content. Unit coverage verifies the white-threshold
coordinate conversion. A dedicated test pins the removal promise: a page carrying
text inside and outside the crop rectangle comes back with the outside text absent
from the saved bytes when `removeDataOutsideCrop=true`, and still present when it
is `false`.
