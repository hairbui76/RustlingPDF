# Add Watermark Compatibility Contract

Route: `POST /api/v1/security/add-watermark`

## Request

The route accepts `multipart/form-data` with:

- one PDF `fileInput`;
- `watermarkType`, normally `text` or `image`;
- text options `watermarkText`, `alphabet`, and `customColor`;
- shared `fontSize`, `rotation`, `opacity`, `widthSpacer`, and
  `heightSpacer`; and
- optional `convertPDFToImage`, default `false`.

Documented defaults are `RustlingPDF`, `roman`, 30-point size, zero
rotation, 0.5 opacity, 50-point spacers, and `#d3d3d3`. Image watermarks also
require `watermarkImage`. The image height is `fontSize`; width retains the
source aspect ratio.

The Java controller treats an unknown or absent watermark type as a no-op and
still returns a rewritten PDF. The Rust route preserves that behavior.

## Processing

- Text watermarks split literal `\n` sequences into lines, select a host font
  fallback for roman, Arabic, Japanese, Korean, Chinese, or Thai text, decode
  Java-style hexadecimal colors with light-gray fallback, and remain vector
  Form XObjects.
- Text rows and columns retain Java's rotated bounding-box step and inclusive
  edge placement.
- Raster watermarks retain aspect ratio and alpha masks. Their rows, columns,
  center rotation, and exclusive edge placement follow the Java formulas.
- Opacity is installed through an `ExtGState`; inherited page resources and
  existing page content remain intact.
- The watermark grid is appended with PDFBox's `AppendMode.APPEND,
  resetContext = true` semantics: a stream holding a bare `q` is inserted at the
  front of the page's `/Contents` array and the watermark stream opens with the
  matching `Q`, so the tiling is always laid out from the page's initial
  graphics state. A source page whose own stream leaves a `q ... cm` open
  therefore cannot shift or rescale the grid. The wrapper is a visual no-op for
  balanced content and is omitted when the page has no existing content, where a
  lone `Q` would underflow the graphics state stack.
- Precisely, the grid resumes from the state captured by the most recent `q` the
  page's own stream left unmatched, or from the page's initial state when that
  stream is balanced. The number of unmatched `q`s does not matter:
  `q q 0.5 0 0 0.5 300 400 cm` resets cleanly, because the innermost unmatched
  `q` saved the state from before the `cm`. What survives is state the stream
  changed at nesting depth zero, outside any `q` — a page that scales with
  `0.5 0 0 0.5 0 0 cm` and only afterwards leaves a `q` unmatched still scales
  the grid. PDFBox writes the same single `q`/`Q` pair and behaves identically,
  so this residual is shared with upstream rather than a divergence.
- Every page receives the watermark.
- `convertPDFToImage=true` sends the completed document through the shared
  native `PDFium` full-page rasterization path, using the configured maximum
  render DPI.

## Response

The route returns `application/pdf` named `<base>_watermarked.pdf`.

## Compatibility limits

- Fonts come from the Rust host's font database, so exact glyph selection and
  metrics can differ from the Java-bundled Noto files.
- To bound generated content and memory, the Rust route rejects more than
  250,000 placements on one page and restricts each spacer to 0–65,535 points.
  Normal UI values are far below these limits; Java's per-axis cap can still
  generate roughly 100 million draw operations.
- The rasterized branch returns `501` when `PDFium` is not installed; an
  explicitly configured but failing runtime returns `500`.
