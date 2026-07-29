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

Resources reachable only from a deleted mark go with it. PDFium rebuilds `/Font`,
`/ExtGState`, and `/XObject` itself; the port additionally drops `/Pattern` and
`/Shading` entries of the page's own resource dictionary that no surviving content
stream paints with, because PDFium leaves those two categories untouched and the
rebuild copies the page's `/Resources` verbatim into the new Form XObject. Without
that step a tiling pattern's text and images, and a shading's sampled-function
data, stayed fully extractable from a file whose caller had asked for them to be
deleted.

Which entries survive is decided by **resolving each reference to the object it
names, in the scope where the reference appears** — not by matching resource names.
Names are scope-local: a page and a Form XObject it paints routinely both call
their own, different patterns `/P0`, and a name-keyed decision would let a
reference made inside the form retain the page's unrelated secret. The search
follows Form XObjects and surviving patterns transitively, and **honours resource
inheritance**: a Form XObject without its own `/Resources` inherits the enclosing
scope (ISO 32000-1 §8.10.1), so a chain of inheriting forms still resolves both its
own `Do` targets and the page-level patterns it paints with. A form that does carry
`/Resources` has them searched first, with the enclosing scope behind it, so a name
its dictionary happens to omit resolves the way real viewers resolve it rather than
being lost.

A `/Resources` dictionary that is an indirect object may be **shared** — a Form
XObject pointing at the very dictionary its page uses is spec-legal and real
generators emit it. Each such dictionary is filtered once, in place, against a live
set collected across every page first, so every holder of it agrees and an entry
any page still paints survives. Writing a filtered copy onto one holder instead
leaves the others pointing at the unfiltered original, which keeps the dead entry
reachable.

Both directions are load-bearing and are pinned by tests: retaining an entry only
an out-of-crop mark referenced re-opens the leak, and dropping one that surviving
content still paints with leaves a dangling name and visibly corrupts the page.

If the walk cannot establish what is live — an undecodable content stream, or
nesting past its limits — the request **fails with `422 Unprocessable Content` and
no file is produced**, naming the cause and pointing at
`removeDataOutsideCrop=false`. The status is deliberate: this is an anticipated,
designed refusal about a property of the payload, not an unexpected server fault,
so RFC 9110 puts it outside 5xx; `400` would be wrong too, since deep nesting and
large form counts are legal PDF, not malformed requests. The abort is logged at
`WARN` with `event = "crop_resource_analysis_aborted"` so operators can watch how
often it fires without 5xx noise. It never falls back
to keeping everything, for the same reason this endpoint returns `501` rather than
cropping without removal when PDFium is missing: a `200` carrying a file that still
holds the data the caller asked to delete is indistinguishable from a clean one, so
it is the one failure the caller cannot detect or recover from. Retrying without the
flag costs nothing that is not recoverable.

"Surviving content" is defined by one rule rather than a list: **every content
stream that will still be in the output file**. Concretely the walk follows every
declared resource that reaches a content stream — `/XObject` forms, `/Pattern`
content, `/ExtGState` (its soft-mask group and its `/Font`), and `/Font` Type 3
glyph procedures — from the page's own resources and, recursively, from every
scope those reach, honouring resource inheritance (a Form XObject or Type 3 font
without its own `/Resources` resolves against the enclosing scope, ISO 32000-1
§8.10.1 and §9.6.5).

Declarations are followed, not merely the operators that invoke them, and that is
load-bearing in both directions:

- **Necessary.** A stream survives when some surviving dictionary declares it and
  nothing prunes that declaration — regardless of whether an operator executes it.
  PDFium regenerates a page's operators while keeping its resource declarations,
  so a graphics state that selected a Type 3 font outlives the `gs` that invoked
  it. More generally, a form's or a pattern's *own* `/Resources` is never rewritten
  by PDFium and is not touched by this pruning, so an `/XObject` declared there
  always survives even if nothing does `Do` on it. Following operators alone prunes
  what such a stream paints with and leaves it naming a resource no dictionary
  declares — the counter-example that produced this rule was a form whose own
  resources declared a second, never-invoked form painting a page-level pattern.
- **Safe, not merely conservative.** The walk runs on the *post-PDFium* document,
  so "declared" already means "survived PDFium's own resource rebuild". Following
  declarations therefore follows exactly what is still in the file; it cannot
  retain something PDFium already dropped. Fixtures where an out-of-crop-only
  secret is reachable solely through a declared-but-uninvoked path confirm the
  secret is still removed.

A path that is *not* followed causes **corruption, not a leak**: the entry is
pruned while a stream that names it survives, leaving a dangling resource name.
That is the direction the residual risk points, and it is why the walk errs
towards keeping entries whenever it cannot decide.

The following were enumerated and need no traversal: annotation appearance streams
(the rebuild emits no `/Annots`, so they never survive — pinned by
`rebuilt_pages_carry_no_annotations`); form `/Group`, image `/SMask` and `/Mask`,
`/Function`, halftones, `/Properties` and `/OC` (none are content streams);
ShadingType 4–7 meshes (data, not operators); and inline images (already inside the
stream being scanned).

The walk itself is linear in the size of the document — it visits each
(stream, scope) pair once and carries scopes by identity rather than by value, and
it is iterative rather than recursive so a long chain of inheriting forms cannot
exhaust the stack. Its limits (256 nested scopes, 100 000 scoped streams) exist to
stop pathological nesting, not to ration ordinary work; measured documents sit
orders of magnitude below them.

Verified end to end against plain text, images, Form and nested-Form text, inline
images, invisible (`3 Tr`) text, optional-content (OCG) marks, `/Contents` arrays,
Type 3 font text, subset fonts, annotations with appearance streams, rotated pages,
multi-page documents, patterns and forms sharing a resource name, and nested forms
relying on inherited resources.

### Fidelity cost of asking for removal

Removal regenerates the content stream of **each page something was removed
from**, and `FPDFPage_GenerateContent` does not round-trip every construct.
Measured on the pinned runtime: on such a page **a pattern fill comes back as a
flat colour, and an `sh` shading mark is dropped entirely** — including for marks
*inside* the crop rectangle, since regeneration rewrites the whole page. Those
resources are then unreferenced, so their bytes are removed with them.

A page with nothing outside the crop rectangle is never regenerated and keeps its
marks and resources verbatim, so this cost applies only where a removal actually
happened.

`removeDataOutsideCrop=false` never regenerates content and preserves both the
marks and their resources exactly. Callers who need pattern and shading fidelity
more than they need deletion should use it; this is a trade-off of asking for
deletion, not a silent loss, and a test pins both halves so a future PDFium upgrade
that starts round-tripping patterns fails loudly instead of leaving this stale.

### What is NOT removed

A mark that **straddles** the crop boundary is kept whole. A text run that starts
inside the crop rectangle and continues past its edge keeps all of its glyphs,
including the ones outside; the same holds for a path or an image that overlaps the
edge, and for the interior of a Form XObject whose own bounding box overlaps the
edge. Such content is clipped at render time but remains in the file.

This straddling rule is not a regression against the Ghostscript branch this
replaces: it is the same rule Ghostscript applied. `pdfwrite -dUseCropBox` culled
marks that missed the crop box and passed straddling text runs through unsplit,
because `pdfwrite` cannot split a glyph run at a clip edge either. The port
reproduces that behaviour rather than claiming a stronger guarantee than the
endpoint has ever delivered.

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
coordinate conversion. Dedicated tests pin the removal promise: a page carrying
text inside and outside the crop rectangle comes back with the outside text absent
from the saved bytes when `removeDataOutsideCrop=true`, and still present when it
is `false`; a tiling pattern painting text, a tiling pattern painting an image, and
a shading with a Type 0 sampled function all lose their payloads when only an
out-of-crop mark referenced them; and a Type 3 font text run outside the crop is
removed without crashing the process.
