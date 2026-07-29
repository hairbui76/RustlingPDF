# `POST /api/v1/convert/pdf/markdown`

Rust compatibility contract for `ConvertPDFToMarkdown`.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- Success returns `<base>.md` with content type `text/markdown`.

## Behavior

When the `PDFium` runtime is available, the implementation reconstructs visual lines
from page text geometry and infers headings by font size, porting Java's
`HeadingDetector`: it computes the document's median glyph font size and median line
height, then for each line compares its dominant glyph size (or line height when sizes
are degenerate, i.e. ≤ 2.0) against the corresponding body median. A ratio above 1.4
becomes `#`, above 1.2 becomes `##`; lines longer than twelve words or ending in
`.`/`!`/`?` are never promoted (size/brevity signals only, never text matching).
Non-heading lines are rebuilt into paragraphs (broken at vertical gaps and page
boundaries), bullets become list items, lower-case soft-hyphen breaks are repaired, and
inline Markdown controls are escaped so text stays literal.

Two-column reading order is also inferred, porting Java's `detectsTwoColumns` and
`splitIntoColumns`. Per page, if there are at least eight lines spanning at least 200
units and a central-band gutter (35 %–65 % of the used width) exists that few lines
cross while both sides carry at least four lines, the page is split at the widest gap
between body-width (≥ 40 unit) line left edges; the left column is emitted top-to-bottom
before the right. Full-width, narrow, or short pages keep single-column extraction order.

When `PDFium` is unavailable the route falls back to a text-only `lopdf` baseline that
extracts each page in document order and applies the same paragraph, bullet,
soft-hyphen, and escaping rules but does not infer headings. Empty pages produce no
output block. Malformed PDFs are `400 Bad Request`.

## Completeness and limits

There is no page-count limit: every page of the document is converted, however long it
is. The shared geometry extractor bounds retained lines at 200,000 for memory safety;
reaching that bound means the reconstruction is incomplete, so the route returns
`400 Bad Request` naming the limit instead of a `200 OK` body with the tail of the
document silently missing. A successful response therefore always covers the whole
input.

## Parity gaps

Java's `PdfMarkdownConverter` also infers borderless/ruled tables, table continuation
across pages, bold-label emphasis, and images. Those layout-specific features are
deliberately not yet claimed; the ported slice covers size-based heading inference,
two-column reading order, and textual content in page order.

Column detection is only as good as the line assembly it runs on. Lines are built by
merging consecutive PDFium text segments that share a vertical centre, with no
horizontal-gap bound, so on a page whose content stream interleaves the columns on a
shared baseline the two columns are fused into one wide line before detection runs; the
gutter is then invisible and the merged text is emitted as a single paragraph. Pages
whose content stream emits one column at a time are detected as documented above.
`convert/pdf/html` shares this line assembler and the same limitation.

## Verification

Unit tests cover the ported `heading_prefix` decision (size-ratio thresholds, the
word-count cap, sentence suppression, and the line-height fallback), the `median`
helper, `detects_two_columns` (gutter layout accepted; full-width/narrow/short pages
rejected), `split_into_columns` (widest-gap split, single-cluster and no-body-width
fallbacks), line-based Markdown assembly (headings, bullets, paragraph breaks, page
separation, left-before-right column order), paragraph assembly, escaping, and
soft-hyphen repair. An end-to-end test
builds a PDF with a large-font line and body text and confirms the PDFium path promotes
only the large line to a heading. A regression test converts an 11,000-page document and
asserts the first, 10,000th, 10,001st, and last page markers all survive, pinning the
absence of a page cap. HTTP tests cover content type, output filename, page order, and
missing/malformed uploads.
