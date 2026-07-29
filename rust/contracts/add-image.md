# Add Image Compatibility Contract

Route: `POST /api/v1/misc/add-image`

## Request

The route accepts `multipart/form-data` with:

- `fileInput`: required source PDF;
- `imageFile`: required raster image or SVG document;
- `x`: optional finite PDF user-space coordinate, default `0`;
- `y`: optional finite PDF user-space coordinate, default `0`;
- `everyPage`: optional boolean, default `false`.

As in the Java implementation, the actual coordinates are measured from the
PDF page's lower-left origin even though the legacy schema describes a
top-left corner.

## Processing

- Raster input is detected from its bytes, decoded with bounded dimensions,
  embedded with its intrinsic pixel width and height, and retains transparency
  through a PDF soft mask.
- SVG input is detected from its first 200 bytes, converted into vector PDF
  content, and embedded as a Form XObject at its intrinsic size.
- The overlay is appended to the first page unless `everyPage=true`, in which
  case it is appended to every page.
- Existing page content and inherited resources are preserved.
- The overlay is appended with PDFBox's `AppendMode.APPEND,
  resetContext = true` semantics: a stream holding a bare `q` is inserted at the
  front of the page's `/Contents` array and the overlay stream opens with the
  matching `Q`, so the image is always placed from the page's initial graphics
  state. A source page whose own stream leaves a `q ... cm` open therefore
  cannot shift or rescale the overlay. The wrapper is a visual no-op for
  balanced content and is omitted when the page has no existing content, where a
  lone `Q` would underflow the graphics state stack.
- Precisely, the overlay resumes from the state captured by the most recent `q`
  the page's own stream left unmatched, or from the page's initial state when
  that stream is balanced. The number of unmatched `q`s does not matter:
  `q q 0.5 0 0 0.5 300 400 cm` resets cleanly, because the innermost unmatched
  `q` saved the state from before the `cm`. What survives is state the stream
  changed at nesting depth zero, outside any `q` — a page that scales with
  `0.5 0 0 0.5 0 0 cm` and only afterwards leaves a `q` unmatched still scales
  the overlay. PDFBox writes the same single `q`/`Q` pair and behaves
  identically, so this residual is shared with upstream rather than a
  divergence.
- External SVG resources, XML document declarations/entities, and remote CSS
  imports are rejected before SVG parsing. Inline `data:` resources remain
  supported.

## Response

Success returns `200`, `Content-Type: application/pdf`, and an attachment named
`<input-base>_overlayed.pdf`, preserving the legacy spelling.

Missing uploads, malformed PDFs/images, unsafe or malformed SVG, invalid
booleans, and non-finite coordinates return `400`. Internal encoding or output
failures return `500`.

## Compatibility limits

- SVG font selection follows the host's installed fonts and can differ from
  Java/Batik font fallback.
- Unsafe SVG resources are rejected rather than silently removed by the Java
  sanitizer.
- Raster codec normalization can change the embedded byte representation while
  preserving decoded pixels and alpha.
