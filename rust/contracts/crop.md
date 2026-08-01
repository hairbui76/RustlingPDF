# `POST /api/v1/general/crop`

Current contract for cropping PDF pages.

## Request

- Content type: `multipart/form-data`
- `fileInput`: one non-empty PDF, required
- `autoCrop`: boolean, default `false`
- Manual mode requires finite `x`, `y`, `width`, and `height` floating-point
  values. As in Java, the same rectangle is applied to every page.
- `removeDataOutsideCrop`: boolean, default `true` (see
  [Out-of-crop content removal](#out-of-crop-content-removal) — it is a data
  removal promise, not an optimisation)
- Automatic mode ignores manual coordinates, but not `removeDataOutsideCrop` (see
  [Automatic mode](#automatic-mode)). It renders each page at 150 DPI with the
  pinned PDFium runtime, considers RGB values of at least 250 white, and maps the
  detected pixel box back to PDF coordinates through PDFium's own display matrix
  (see [Page geometry](#page-geometry)), producing one rectangle per page. A
  configured but broken PDFium runtime is a server error; an unconfigured
  development fallback reports `501 Not Implemented`.
- **Sampling density depends on the flag.** With `removeDataOutsideCrop=false` the
  scan skips every second pixel on pages over 2,000 px in either dimension, as in
  Java. With `true` it samples every pixel, because a skipped pixel then means
  deleted ink rather than a slightly tight frame — see
  [What detection cannot see](#what-detection-cannot-see). On a large page carrying
  a hairline the two settings can therefore return **different rectangles**, the
  removal one being the larger and more accurate.

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
cases someone thought to write down: every reachable content stream is walked from
its page, and each `/XObject`, `/Font`, `/ExtGState`, `/Pattern` and `/Shading`
name it uses must resolve **in that stream's own scope chain** — the chain a viewer
walks (ISO 32000-1 §8.10.1), not "somewhere in the document". It is asserted on
every fixture in `tests/crop_endpoint.rs`, at four crop rectangles, in both modes,
and `RUSTLING_CROP_CORPUS_DIR` runs it over a directory of real PDFs against the
`removeDataOutsideCrop=false` output as the control.

Two earlier versions of this check were weaker than this paragraph claimed, and
both weaknesses were found by an independent tester rather than by the suite:

- It unioned every declared name in the document into one set per category and
  accepted a used name if *any* dictionary declared it. That passes the exact bug
  it exists to catch — prune `/P0` from a page while an unexecuted form declares
  its own unrelated `/P0`, and a glyph procedure resolving through the page scope
  finds nothing while the check reports clean.
- It read `/Subtype` and category dictionaries with raw lookups, so it was blind to
  values written indirectly in precisely the places the walk was.

What it still does **not** verify: that a resource resolves to the *same object* a
viewer would pick. It checks that the name resolves to something in scope, not that
the something is right. A checker strong enough for that would have to reimplement
the resolution the walk performs, and would then share its assumptions — which is
the failure mode both bullets above are examples of.

### When removal is refused

`removeDataOutsideCrop=true` returns **`422 Unprocessable Content`** for one shape:
a document whose Form XObject graph expands without bound. Removal works by having
PDFium regenerate each page, PDFium expands every `Do` while doing so, and it
bounds neither recursion nor fan out. ISO 32000-1 §8.10.1 already forbids a form
from invoking itself directly or indirectly; PDFium follows such a graph anyway,
and an acyclic one can be just as expensive.

This is not a theoretical bound. Measured against this endpoint before the check
existed: a **1,982-byte** upload whose six forms each invoke all six reached 45 GB
resident and never returned, and a 557 KB document fanning 60 ways across 8 levels
reached 84 GB. Both took the process down, so every concurrent caller lost their
request too. The check runs before PDFium — after it, there is nothing left to
refuse with — and answers both in about 30 ms.

The graph is built **only** from the `Do` operators a page and its forms actually
execute, never from declarations. A form may be declared in a resource scope it
shares with its page (ISO 32000-1 §8.10.1) — spec-legal and common — so that scope
names the form; a graph built from declarations makes such a form its own child and
refuses an acyclic document as a cycle. An earlier revision fell back to the
declared entries whenever a form's content would not fully parse (a `Do` naming
nothing, trailing NUL padding, a comment then a blank line, a Type 3 `d0`/`d1`
stray digit), and that fallback manufactured exactly this false cycle. It is gone:
a stream that cannot be read contributes no edges, which can only fail to refuse,
never refuse wrongly. That gives up the "cannot hide recursion behind a parse
failure" property, which is acceptable because unbounded expansion is a
performance concern the desktop target treats as out of scope, while a false `422`
on a renderable document reached real users under the default
`removeDataOutsideCrop=true`.

What is refused is genuine: a form whose executed content invokes itself directly
or indirectly, or an acyclic graph that still expands past a million invocations.
Both are documents no renderer completes — verified independently, poppler and
Ghostscript each hang on them with memory unbounded. Measured false-refusal rate on
real documents: **zero**. The only files in the local corpus that refuse are the
amplification fixtures written to be refused, every one of which has forms that
invoke themselves by executed `Do` (checked with an independent parser). The
fixtures earlier rounds were tuned against still return `200` in 0.13 s and 0.76 s,
and `removeDataOutsideCrop=false` never invokes PDFium and answers the refused
documents in under a fifth of a second.

### A crash that was not what it looked like

Some documents used to abort the **process** on the removal branch — a stack
overflow, which cannot be caught, so an unauthenticated request killed the service.
These were recorded for a while as "pre-existing lopdf crashers". That was wrong
twice: `removeDataOutsideCrop=false` loads the same files through the same lopdf
and returns `200`, and the crash is not at load. PDFium completes them on its own
and the resource walk is iterative; what recurses is the rebuild's traversal of
PDFium's output, whose depth is a property of the uploaded file.

Sweeping the stack size on one such document — a 932 KB file with 1,353 forms — is
unambiguous: 1 MiB and 2 MiB abort, 4 MiB and above succeed, and `tokio` gives its
blocking threads 2 MiB. The crop now runs on a scoped 32 MiB thread, and all 19
reproducible crashers return `200`.

That is containment, not a proof. A document nested deeper still overflows, just
further out. The bound that would make it impossible is an iterative traversal in
`lopdf`.

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

How often the second row falls short, measured on a run that completed: of **123
unique real-world PDFs** collected from this machine — de-duplicated by content,
and excluding this repository's own fixtures, staging copies and packaging assets —
**one** preserved a scope, and it is the Type 3 case below. An earlier figure of
"3 of 181 (1.7%)" quoted here came from a run that processed half its list before
timing out, over a set whose "real-world" bucket included 47 repo fixtures and 12
staging copies of synthetic ones; it should not have been cited.

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

A soft mask's `/G` is followed as a form **whatever its `/Subtype`**, including
when the key is absent: `/G` is a transparency group, which §11.6.5.2 defines to
be a Form XObject and which renderers paint without consulting `/Subtype`. The
subtype check that stays on the `Do` path — where an object without `/Subtype
/Form` is an image with no content to follow — does not apply here, because a
group named only from inside the mask would otherwise have its resources judged
dead while the mask stream survived painting with them.

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
exhaust the stack. Its limits are 256 nested scopes, 256 MB of **decoded content
bytes** (rather than a stream count, since the same stream reached under two scope
chains is scanned twice), and a hard cap on the (stream, scope) pairs it will
track: 50,000 pairs and 500,000 scope links. The pair caps are not redundant with
the byte budget — a pair's cost is its chain depth, so a few cross-declaring forms
generate chains combinatorially while charging almost no bytes. Every path that
touches a chain is charged, including the give-up paths that admit no pair and
decode no byte; those were the last unpriced work.

Measured documents sit orders of magnitude below all of these — the deepest scope
nesting across a thousand-document corpus is single digits. Reaching any of them
keeps declarations rather than refusing the request, as
[What limits the promise](#what-limits-the-promise) describes.

Verified end to end against plain text, images, Form and nested-Form text, inline
images, invisible (`3 Tr`) text, optional-content (OCG) marks, `/Contents` arrays,
Type 3 font text, subset fonts, annotations with appearance streams, multi-page
documents, patterns and forms sharing a resource name, and nested forms relying on
inherited resources.

Page **geometry** — `/Rotate`, `/CropBox`, a `/MediaBox` with a non-zero origin,
and each of those written on an ancestor `/Pages` node instead of the page — is
covered separately by `auto_crop_maps_pixels_through_rotation_and_page_boxes`, and
only became true when that test was written. This list previously claimed "rotated
pages" with nothing behind it: no test in `crop_endpoint.rs` and no code in
`pdf_crop.rs` mentioned rotation, and automatic crop was in fact broken on every
rotation except `0`. See [Page geometry](#page-geometry).

### Fidelity cost of asking for removal

Everything in this section happens while **rewriting** a page. Loss that happens
before that, by measuring the wrong rectangle in the first place, is a separate
mechanism and lives in [What detection cannot see](#what-detection-cannot-see).

Removal regenerates the content stream of **each page something was removed
from**, and `FPDFPage_GenerateContent` does not round-trip every construct.
Measured on the pinned runtime: on such a page **a pattern fill comes back as a
flat colour, and an `sh` shading mark is dropped entirely** — including for marks
*inside* the crop rectangle, since regeneration rewrites the whole page. Those
resources are then unreferenced, so their bytes are removed with them.

**Text is not exempt.** On some documents regeneration also drops glyphs from
subset fonts, and the marks it drops are inside the crop rectangle.

Measured on this machine over **124 documents and 493 pages** — 41 real documents
plus, for each, a `+90`-rotated copy and a copy with the crop box inset 20 points,
so that rotation and `/CropBox` are represented rather than assumed absent — with
each removal output compared against its own `removeDataOutsideCrop=false` control
page by page at 72 DPI: **99 of 124 lose no in-crop ink at all**, and **no page at
any setting came back blank**. Of the rest, most only *gain* dark pixels, which is
the pattern-to-flat-colour effect above (a page of pale statistic cards comes back
with black ones, every glyph intact). Four document families genuinely lose visible
text: a Skia/Google-Docs page whose Identity-H subset fonts come back with ~6% of
the page's ink blank, a Word document at ~3%, and two others near ~1.8%, one of
which loses its section headings while the body survives.

This is a property of PDFium's content generator, not of the resource pruning or of
which branch asked. Disabling the pruning entirely reproduces the loss to three
decimal places on every document checked, and the manual branch reproduces it given
the same rectangle. It has been there for as long as removal has. It is recorded
here because it was not, and because it is a larger cost than the pattern and
shading losses above: a caller who crops to *keep* something can lose it.

If that is unacceptable for a document, `removeDataOutsideCrop=false` regenerates
nothing and is exact — at the price of keeping the out-of-crop data, which is the
whole trade.

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

`autoCrop=true` answers `removeDataOutsideCrop` exactly as manual coordinates do.
Everything above — what is removed, what limits the promise, when removal is
refused, the fidelity cost — applies unchanged; the two modes run the same removal
pass and differ only in where the rectangle comes from.

The rectangle is **per page**. Automatic detection measures each page's own
rendered content, so page two's marks are judged against page two's box. Applying
one page's rectangle to the whole document would delete marks the caller can still
see, which is why the removal pass takes a list rather than a single rectangle and
refuses outright if that list does not cover every page PDFium opens.

This is a fix, not the original behaviour. Automatic mode used to ignore the flag
and always take the clip-only path, so a caller who asked for the data to be
deleted received `200` and a file that still contained every mark — no header, no
warning, and a text extractor recovered all of it. That failed the rule the rest of
this document is built on: a request to delete data is answered with the data gone
or with an error, never with a `200` that misdescribes the file.

Automatic mode makes one class of leak reachable that manual coordinates cannot:
content that **renders as nothing**. Detection measures rendered pixels, so an
invisible (`3 Tr`) text run, a fully transparent image, or anything else the
renderer draws no pixel for falls outside the detected rectangle by construction,
and no rectangle the caller could have supplied would have covered it — the
rectangle is the one the endpoint chose. Pinned by
`auto_crop_honours_remove_data_outside_crop`, whose fixture carries both shapes and
whose control run at `removeDataOutsideCrop=false` proves they are still there when
removal is not asked for.

Because removal now runs in automatic mode, the `422` refusal below applies to it
too, and the check runs **before PDFium is asked for anything at all** — before
detection, not just before removal. Detection renders every page through the same
PDFium that expands the form graph, so a check placed after it would never run.
A clip-only automatic crop does not pay for the check, and is unaffected.

### What detection cannot see

Automatic removal deletes what falls outside a rectangle **measured by rendering**.
That makes every limit of the measurement a deletion path, and it is a different
mechanism from the fidelity losses below: those happen while rewriting a page, this
one happens before the page is ever touched, by deciding the wrong rectangle.

The rule for the whole area: *anything detection under-reports is destroyed, not
merely mis-framed.* Two instances of it have already been shipped and fixed — the
coordinate mapping in [Page geometry](#page-geometry), and pixel sampling.

**Sampling.** The scan skipped every second pixel on pages over 2,000 px. Measured:
a 1000x1200 pt page (2084x2500 px at 150 DPI) with a 0.2 pt rule at `x = 40` — the
rule renders 833 dark pixels, every one of them in bitmap column 83, and the scan
visited 82 and 84. The operator drawing it was absent from the output under `200`.
On A3 the same effect swallowed a 0.25 pt rule and a 0.3 x 0.3 pt dot. Removal now
samples every pixel, and the detected box is grown by one sampling step on all four
sides rather than on two. Re-measured afterwards: rules from 0.5 pt down to 0.02 pt
all survive, because PDFium renders a sub-pixel fill into a whole pixel, so once
nothing is skipped, thinness alone is no longer a way to be missed.

**The white threshold — the remaining one, and it is deliberate.** A pixel counts
as ink only if some channel is below 250. Measured on the same page shape: a fill
at 96% white (RGB 245) is detected and kept; at 98% white (RGB 250) and lighter it
is outside the rectangle and therefore deleted. That boundary is a 5/255 difference
against paper — invisible on screen and in print — so it belongs to the
"renders as nothing" class this endpoint deletes on purpose, along with `3 Tr` text
and fully transparent images. It is recorded here because it is the same shape as
the two defects above, and because a caller storing a near-white watermark should
know it will not survive `removeDataOutsideCrop=true`.

Checked and **not** deletion paths, so that the list is exhaustive rather than
merely the ones that were found:

- Annotation and form-field appearances are rendered by default, so they widen the
  rectangle rather than being missed. (They are dropped from the output by the
  rebuild in both modes, which is separate and long-standing.)
- A page with no ink at all yields the whole page, so nothing is removed.
- An unreadable pixel offset counts as ink, which can only widen the rectangle.
- Content outside the page's own `/CropBox` is never rendered and never detected,
  in both modes alike — it is out of crop by the document's own definition.
- Optional content whose default configuration is off renders as nothing, which is
  the documented class above rather than a new one.
- A render failure is an error, never a silently empty rectangle.

### Page geometry

Automatic detection is a **rendering** measurement feeding two consumers that do
not share its coordinate system, and getting that wrong is not a cosmetic bug once
removal is wired to it. Three systems are in play:

| | System |
|---|---|
| The rendered bitmap | pixels, y down, origin at the **crop box** corner, **rotated** by `/Rotate` |
| Out-of-crop removal (`FPDFPageObj_GetBounds`) | PDF content space, unrotated |
| The rebuilt page (`page_form`, `add_cropped_page`) | content space shifted so the **media box** corner is the origin |

Detection converts pixels to content space through PDFium's own
`FPDF_DeviceToPage`, which inverts the very display matrix it rendered with, so
rotation, the crop box origin and the scale are all handled by the component that
knows what it drew. All four corners are mapped and the extremes taken, because a
quarter turn swaps which corner is which. Removal then uses those rectangles
unchanged, and the rebuild subtracts each page's media box origin — read with
`lopdf`, the same library `page_form` reads it with, so the two agree.

This replaced a bare `page.width() / bitmap.width()` scale, which cannot express
any of it. The scale silently assumed `/Rotate 0`, `/CropBox == /MediaBox` and a
media box at the origin; on the fixtures above, **12 of 15 geometry shapes came
back with not one dark pixel at 150 DPI** — a blank page under `200`. Clip-only was
mis-cropped the same way and merely kept the data; with removal wired to the same
rectangle the marks were physically deleted. One shape, a non-zero media box
origin, was worse still: the clip was correct and the removal deleted everything
anyway, because the single rectangle was right for one consumer and wrong for the
other.

Rotation itself is still **not preserved** in the output, in either mode: the
rebuild has never carried `/Rotate` over, so a rotated page comes back cropped to
the right region but drawn upright. That is unchanged and separate from the mapping.

### Requirements

Content removal runs on the pinned PDFium runtime — the same runtime automatic
detection already requires. A development environment without a configured PDFium
library returns `501 Not Implemented` for `removeDataOutsideCrop=true` in **both**
modes; a configured but broken runtime, or a removal failure, returns a server
error. The endpoint never silently falls back to the clip-only path, because that
would answer a request to delete data with a file that still contains it. Packaged
environments install the pinned native revision, so this is a development-only
boundary.

Both modes reach the same `501` for the same reason, by different routes: the
manual branch when the removal pass reports no runtime, the automatic branch when
detection does, since automatic detection needs the runtime removal needs. Pinned
by `crop_refuses_removal_it_cannot_honour`, which asserts the status directly
rather than depending on a machine that happens to have no PDFium.

Ghostscript is no longer involved in any form:
`RUSTLING_PROCESSING_GHOSTSCRIPT_COMMAND` is still accepted as an environment
variable and ignored.

## Response

- Success: `200 OK`, `Content-Type: application/pdf`
- `422 Unprocessable Content` when `removeDataOutsideCrop=true` and the document's
  form graph expands without bound (see
  [When removal is refused](#when-removal-is-refused)); logged at `WARN` with
  `event = "crop_form_expansion_refused"`
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

`auto_crop_maps_pixels_through_rotation_and_page_boxes` crops twelve page-geometry
shapes — all four rotations, `/CropBox` with and without rotation, a `/MediaBox`
with a non-zero origin with and without rotation, both together, and the inherited
form of each — at both flag values. For every one it requires the visible run to
still be extractable from the returned file, the invisible out-of-crop run to be
gone when removal was asked for, and the returned media box to be a *tight*
rectangle sitting on the ink. Reverting the mapping to a scale factor, or dropping
only the media box shift, each fails it.

`auto_crop_removal_samples_every_pixel_so_a_hairline_is_not_deleted` pins the
sampling density against the shape that defeats it: a 0.2 pt rule on a page large
enough for the sparse scan, landing in a single odd pixel column. It requires the
operator to still be in the returned file and the media box to actually span from
the rule to the anchor block, so a rectangle that merely contains the ink by luck
does not pass. Putting removal back on sparse sampling fails it.

Automatic mode is pinned on both directions of the flag.
`auto_crop_honours_remove_data_outside_crop` crops a page whose only out-of-crop
content renders as nothing — an invisible `3 Tr` text run and a fully transparent
image — and requires both to be absent from the returned bytes at
`removeDataOutsideCrop=true` and present at `false`, while the in-crop text and
image survive and the media box is still the detected one. It fails on the previous
behaviour with the message "automatic crop returned 200 with the data
removeDataOutsideCrop=true asked it to delete".
`auto_crop_measures_each_page_against_its_own_rectangle` crosses two pages over, so
that reusing one page's rectangle for the document deletes the other page's visible
text *and* spares its secret; either mistake fails it. Both are additionally
verified outside the suite by extracting text with `pdftotext` and images with
`pdfimages` from the returned file, since a test that only searches the response
bytes is a weaker instrument than the tools an attacker would reach for.

`every_fixture_comes_back_without_a_dangling_resource_name` and
`rebuilt_pages_carry_no_annotations` run automatic mode as well as manual, because
the rectangles automatic detection chooses are not reachable from any list of
coordinates and a pruning defect that only appears at one of them would otherwise
be invisible. `the_form_expansion_bound_still_refuses_genuine_blowups` covers
automatic mode for the same reason it covers manual, and its automatic case pins
the ordering: the check has to run before detection, or the process is gone before
it runs — that case hangs rather than fails if the check ever moves.

Four fixtures cover values written as **indirect references**, because a PDF may
always write one and the walk had four places that assumed otherwise:
`/ExtGState` `/Font` as an indirect array, the same with a decoy declaration of the
same name elsewhere in the document, a Form `XObject` with an indirect `/Subtype`,
and a `/Pattern` dictionary reached through a reference chain.
`follows_resources_written_as_indirect_references` reports all four rather than
stopping at the first, since which shape broke is the diagnosis. Reverting any of
the four reads to a raw lookup fails both it and the property test below, and the
property test names the exact resource and crop rectangle.

`an_unbalanced_delimiter_does_not_blind_the_checker` pins the tokeniser against one
stray `(`, which used to consume the rest of the stream and hide every later name.

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
introduced are reported.
