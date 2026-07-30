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

> **It is best effort, not a guarantee.** The marks themselves are always deleted.
> The *resources* behind them — a pattern's tile, a font, a form — are deleted only
> where the port can fully parse the content that would name them. Where it cannot,
> those declarations are kept, and the data they reach stays recoverable from the
> output. [What limits the promise](#what-limits-the-promise) says when that
> happens, how to tell, and what to do if you need a hard guarantee.

### What is removed

Every page object whose bounding box lies entirely outside the crop rectangle —
text runs, paths, images, and Form XObject invocations alike — is deleted from the
page through PDFium, and the page's content stream is regenerated without it. The
deleted marks are gone from the saved bytes: their text is no longer extractable
and their image samples are no longer present. Annotations are dropped for every
page by the rebuild step that follows.

Resources reachable only from a deleted mark go with it. PDFium rebuilds a page's
own `/Font`, `/ExtGState`, and `/XObject` when it regenerates the page, but it
never touches `/Pattern` or `/Shading`, and it never rewrites a Form XObject's or a
pattern's **own** `/Resources` at all — while the rebuild copies the page's
`/Resources` verbatim into the new Form XObject. Anything left declared therefore
stays reachable. The port drops every entry in all five categories that no executed
path names, so a tiling pattern's text and images, a shading's sampled-function
data, and whole never-invoked forms leave the file rather than staying extractable
from a document whose caller asked for them to be deleted.

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

The corruption direction is pinned as a **property of the output**, not as a set of
cases someone thought to write down: no surviving content stream may name a
resource that no reachable dictionary declares, across all five categories. It is
asserted on every fixture in `tests/crop_endpoint.rs` in both modes, and
`RUSTLING_CROP_CORPUS_DIR` runs the same check over a directory of real PDFs using
the `removeDataOutsideCrop=false` output as the control. The checker tokenises raw
content bytes rather than reusing `lopdf`'s content decoder, because a checker
sharing that decoder's truncation blind spot would have confirmed the very defect
described below.

### What limits the promise

Deciding which declarations are dead means **parsing content streams**, and that
can fail. The parser stops at constructs it does not accept; a stream's filters can
fail to decompress; a scope can nest past the walk's bound; the walk can exhaust
its work budget. In every one of those cases the port **keeps the declarations in
the scopes that stream could have named into** and still returns the cropped file.

So the promise splits in two, and only the first half is unconditional:

| | Guaranteed? |
|---|---|
| Out-of-crop **marks** deleted from the page and its content stream regenerated without them | **Yes**, whenever the endpoint returns `200` |
| **Resources** reachable only from a deleted mark removed from the file | Only where the content naming them parsed in full |

Erring towards keeping is deliberate, and it is a reversal. An earlier revision
pruned whatever the parser did not report, on the theory that keeping data the
caller asked to delete is the worse failure. It is not, because the parser used
here — `lopdf`'s lenient content decoder — returns whatever prefix it managed to
read and silently discards the rest. A `%` comment followed by a blank line ends
that parse, and joining the members of a `/Contents` array inserts exactly such a
blank line, so a page whose first content stream is a comment decoded to *zero*
operators. Everything it painted with was judged dead and deleted, while the
content stream naming it was copied to the output byte for byte. On a real 1.1 MB
scanner output that removed `/Font` and `/XObject` from every page — 77% of the
page's ink — under a `200` indistinguishable from a clean one, with the source
already replaced. 3.6% of the real-world PDFs on hand carry the construct.

Refusing those documents with `422` was also tried, and is also wrong: the files
render perfectly in every viewer, and a document that renders must crop.

The other known trigger is **Type 3 fonts**, and it is a parser limitation rather
than anything in the document: `lopdf`'s operator parser matches `[A-Za-z*'"]+`, so
it cannot represent `d0` or `d1`, the only content operators carrying a digit and
required to open every Type 3 glyph procedure (ISO 32000-1 §9.6.5). A metrics-only
glyph — what subset Type 3 fonts emit for blank glyphs — therefore leaves an
unconsumed digit and its scope is preserved. Since a glyph procedure without its
own `/Resources` inherits the page's scope, one such glyph preserves the whole
page's declarations. Documents from `dvips`-style toolchains and some scanners use
Type 3 fonts throughout, so for them removal is effectively marks-only. Fixing this
means fixing the operator parser, not the walk.

Under-deletion is the failure to prefer because it is **bounded** and
**inspectable**. Bounded: the marks are already gone: what survives is the
resource declarations behind them, so what leaks is a pattern tile, a font
program, or a form body — not the page as the reader saw it. Inspectable: the
returned file is right there, and `qpdf --qdf` shows exactly which declarations
survived. Over-deletion offers neither.

It is also narrower than it first looks, because regeneration repairs much of it.
PDFium rewrites the content stream of every page it removed something from, and on
the pinned runtime that output has parsed in every case measured — so an unparsable
**page** stream and an actual removal on that page rarely coincide: where there was
something to delete, the stream that decides liveness is PDFium's, not the
original's. What remains is the case where the unreadable stream is one PDFium never
rewrites — a Form XObject's, a tiling pattern's, or a Type 3 glyph procedure's — and
it resolves names against a scope that also declares an out-of-crop mark's
resources. Then the page is regenerated, the mark is gone, and the declaration
behind it is kept anyway. Measured: of the ten secret-bearing fixtures covering the
known leak shapes, zero leak under this behaviour; a fixture purpose-built for the
shape above does.

Both give-up paths log a structured `WARN` — `event =
"crop_resource_pruning_partial"` with the scope and dictionary counts, or `event =
"crop_resource_pruning_skipped"` when the work budget ran out and nothing was
pruned at all. That is a server-side signal only: **the response carries no
indication that removal was partial**, and callers should not infer one from
response size.

#### If you need a hard guarantee

Do not rely on this endpoint for it. It cannot promise more than its parser can
read, and no setting changes that. Instead:

- **Crop, then flatten.** Post the cropped output to `/api/v1/misc/flatten` with
  `flattenOnlyForms` absent. That renders every page to a single RGB image, so no
  resource dictionary survives to leak through and the guarantee no longer depends
  on parsing anything. It costs selectable text, vector fidelity, and file size —
  the honest price, and the same one `/api/v1/security/redact` already pays.
- **Redact instead of cropping** if the goal is removing specific content rather
  than reshaping the page. This port's `/api/v1/security/redact` always emits an
  image-only PDF and never copies source text, annotations, or page objects
  forward, so it does not infer what became unreachable — it does not carry it.
- **Verify, do not assume.** For a given document the only proof is the output:
  extract its text and embedded images and confirm the sensitive material is
  absent. The `WARN` events above tell an operator when to expect a preserved
  scope; they do not tell a caller whether this file is clean.

"Surviving content" is defined by one rule: **every content stream that will
still be in the output file**. The walk starts at each page's content and follows
only what the operators actually execute — `Do` into Form XObjects (recursively,
honouring resource inheritance), `scn`/`SCN` into tiling-pattern content, `sh`,
`Tf` into Type 3 glyph procedures, and `gs` into a graphics state's soft-mask
group and its `/Font` (ISO 32000-1 Table 58, §8.10.1, §9.6.5).

Anything a resource dictionary declares that no executed path names is **removed
from that dictionary** — `/Pattern`, `/Shading`, `/XObject`, `/ExtGState`, and
`/Font` alike — which orphans the stream behind it, and `prune_objects` then
deletes it. This is what makes the rule true by construction rather than by
enumerating traversal paths: a stream the walk did not visit is not in the output
to be visited.

Pruning declarations, rather than traversing them, is load-bearing in both
directions:

- **Leak.** PDFium never rewrites a Form XObject's or a pattern's *own*
  `/Resources`, so a declaration there survives whatever PDFium does. A
  never-invoked form declared inside a surviving form kept an out-of-crop
  secret extractable through the pattern it named; removing the declaration
  removes the whole subtree.
- **Corruption.** Nothing is left naming a resource no dictionary declares,
  because a stream and the entry that reaches it are removed together.
- **Cost.** Traversing declarations meant a stream reached under many scopes was
  re-decoded once per scope: cross-declaring forms turned a 532 KB upload into
  68 s of work and 57 MB, and a 3.5 MB one into over 10 minutes and 2.7 GB, all
  under the global PDFium lock. Dead declarations are now removed rather than
  followed, and the walk's budget counts **decoded content bytes** rather than
  streams so that the remaining rescan shape is priced correctly.

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
exhaust the stack. Its limits — 256 nested scopes, and 256 MB of **decoded content
bytes** rather than a stream count, since the same stream reached under two scope
chains is scanned twice — exist to stop pathological nesting, not to ration
ordinary work; measured documents sit orders of magnitude below them. Reaching
either limit keeps declarations rather than refusing the request, as
[What limits the promise](#what-limits-the-promise) describes.

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

Nor are the resources behind a removed mark, wherever the content that could name
them did not parse — see [What limits the promise](#what-limits-the-promise).

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

The corruption direction is covered as a property rather than a case list.
`every_fixture_comes_back_without_a_dangling_resource_name` crops every fixture in
the file in both modes and requires that no surviving content stream names a
resource nothing declares, over all five categories.
`the_dangling_name_checker_detects_a_name_no_dictionary_declares` pins the checker
itself against documents that *are* corrupt, so the assertions above cannot pass
vacuously; its second case places the undeclared name after a `%` comment, where
`lopdf`'s lenient decoder reports success with zero operators, which pins the
checker's independence from that decoder.
`keeps_declarations_when_a_comment_truncates_the_content_parse` covers both shapes
of the truncation — written into one stream, and produced by joining the members of
a `/Contents` array — and requires the exact names the surviving stream still uses
to still be declared.
`keeps_declarations_a_stream_it_cannot_parse_might_still_name` covers a form whose
content genuinely cannot be parsed at all. Setting `RUSTLING_CROP_CORPUS_DIR` runs
the same invariant over a directory of real PDFs, comparing each removal output
against its own `removeDataOutsideCrop=false` control so only names the pruning
introduced are reported; 1017 unique local PDFs were swept this way with none
introduced.
