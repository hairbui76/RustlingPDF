# `POST /api/v1/misc/flatten`

Contract for form-only and full-page flattening.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- `flattenOnlyForms`: optional boolean, defaults to `false`
- `renderDpi`: optional integer; it applies only to full-page rasterization
- Success returns the original safe filename as `application/pdf`.

When `flattenOnlyForms=true`, the pinned `PDFium` runtime bakes interactive
widgets into page content and removes widget annotations. PDFium may also
flatten other printable annotations on a page that contains forms.

When `flattenOnlyForms` is false or absent, every page is rendered with form
data and annotations and replaced with a single RGB image sized to the same PDF
page. The result no longer contains selectable source text. Requested DPI is
clamped to at least 72 and at most `SYSTEM_MAXDPI`; a missing or invalid
positive system limit defaults to 500 DPI.
Pixel dimensions and total pixel count are checked before allocation.

PDFium embeds the rendered page images. Form-only output preserves the
source creator, valid dates, standard values, and custom Info keys while
rewriting producer to `RustlingPDF v<version>`. Full rasterization copies only
selected standard source values and valid dates into a fresh Info
dictionary, drops custom keys, and writes the RustlingPDF creator/producer label.
Visual page content, page count, page size, metadata, and loss of selectable
text are covered by tests; image encoding, rotated/cropped-page corpora, and
non-widget annotation preservation remain explicit validation targets.

`PDFium` is required for both branches. A development runtime without a
configured library returns `501 Not Implemented`; an explicitly configured but
broken runtime or processing failure returns a server error. Packaged cutover
environments install the pinned native revision.

## Verification

HTTP tests cover default request parsing, invalid DPI rejection, the 72-DPI
lower clamp, original response naming, page-size preservation, image-only full
flattening, and form-widget removal. The same tests run against the no-native
boundary and the pinned native runtime.
