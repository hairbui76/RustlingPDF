# Execution Plan: Decide crop deletion from document structure, not from pixels

Date: 2026-08-02

## Status

Active

## Outcome

`POST /api/v1/general/crop` with `autoCrop=true` and `removeDataOutsideCrop=true`
stops deciding what to delete from a rendered bitmap. Each page object is
classified from the document's own structure, only objects that are invisible in
**every** configuration may be deleted, and the `422` refusal for optional content
is withdrawn because the reason for it is gone.

Observable result: every fixture in
`auto_crop_removal_refuses_documents_that_use_optional_content` returns `200` with
the layer's content still extractable from the output, and no fixture anywhere in
`crop_endpoint.rs` loses in-crop ink.

## Context

- Behaviour contract: `rust/contracts/crop.md`, sections
  "What detection cannot see", "Page geometry", "Out-of-crop content removal".
- Code: `rust/crates/rustling-processing/src/pdf_crop.rs` (orchestration, the
  refusal, the resource walk) and `pdfium_backend.rs`
  (`try_detect_auto_crop_bounds`, `try_remove_content_outside_crop`).
- Tests: `rust/crates/rustling-processing/tests/crop_endpoint.rs`.

### Why this exists

`removeDataOutsideCrop=true` promises deletion, and automatic mode derives the
rectangle by rendering the page. Everything the render under-reports is therefore
destroyed rather than merely left out of frame. Four instances of that single
mechanism have been found, in four consecutive review rounds:

| Instance | Status |
|---|---|
| Pixel-to-PDF mapping ignored `/Rotate` and the `/CropBox` origin | fixed |
| Sparse pixel sampling missed hairlines on large pages | fixed |
| Optional content is rendered in one configuration only | **refused** — this plan |
| Ink at or above 250 on every channel counts as white | deliberate, documented |

The pattern is what matters more than any single entry: a rendered bitmap is a
lossy proxy for "what this document can show", and each gap in the proxy is silent,
permanent loss under a `200`. Patching gaps one at a time has now been tried four
times and found a new gap each time.

### A design that was proposed and does not work

"Deletion rectangle = the union of `FPDFPageObj_GetBounds` over every page object."
That rectangle contains every object by construction, removal deletes only objects
lying entirely outside the rectangle, so nothing is ever outside and removal
deletes nothing. `removeDataOutsideCrop=true` silently degrades to clip-only, which
is the original defect this whole area exists to fix — a `200` with the data still
in the file. `auto_crop_honours_remove_data_outside_crop` fails on it. Recorded
because it is superficially attractive: it does make "not detected" impossible, by
also making "deleted" impossible.

## Scope

In scope:

- A per-object classifier that decides deletability from document structure.
- Withdrawing `CropError::OptionalContent` and its `422`.
- Keeping the rendered rectangle for **framing** only.

Out of scope:

- The manual-coordinate branch. It is not measured, so it has none of this
  problem — verified against all five optional-content shapes.
- The white threshold. It stays as documented; it is a rendering decision about
  ink that is invisible against paper in every configuration.
- Fidelity losses inside `FPDFPage_GenerateContent` (pattern fills flattening,
  subset-font glyph loss). Different mechanism, separately documented.
- Preserving `/Rotate` in cropped output. Long-standing and orthogonal.

## Approach

### The two-class rule

Every page object is exactly one of:

1. **Reachable.** Every ordinary object, **plus** any object that carries `/OC` or
   sits inside an `/OC` marked-content section — *regardless of policy, `/VE`
   expression or `/Usage`*. Never deleted.
2. **Invisible in every configuration.** Decidable by a local check on the object
   itself, with no document-wide reasoning: text render mode `3 Tr`, a fill and
   stroke alpha of zero, a zero-area clip. Deletable when it also lies outside the
   crop rectangle.

Only class 2 may be deleted.

The load-bearing simplification is that **the presence of an optional-content
mechanism is itself the answer**. No configuration algebra is evaluated, so `/VE`
being an arbitrary boolean tree stops mattering: the classifier never asks whether
a layer is on, only whether the object's visibility is *conditional at all*.

### The invariant

**Every ambiguity resolves to "keep".** An object the classifier cannot read, a
mechanism it does not recognise, a structure it cannot parse — all reachable. The
only remaining failure direction is over-preservation, which costs fidelity and
file size and is inspectable in the returned file. Under-preservation is silent and
permanent. This is the same direction the resource walk already takes, and for the
same reason.

### Sequence

1. Land the classifier behind the existing removal path, with the `422` still in
   place, and prove it on the fixtures below.
2. Remove the `422` and `CropError::OptionalContent`; convert
   `auto_crop_removal_refuses_documents_that_use_optional_content` into a
   survival test.
3. Re-run the corpus sweep including rotated, `/CropBox` and optional-content
   variants.

## Risks And Recovery

- **Risk: removal becomes a no-op.** A classifier that marks everything reachable
  passes every data-loss test and delivers nothing. Mitigation:
  `auto_crop_honours_remove_data_outside_crop` and the `3 Tr` / transparent-image
  fixtures must still show deletion; a test that asserts *something* is removed is
  mandatory alongside every test that asserts something is kept.
- **Risk: pdfium-render does not expose what the classifier needs.** Marked-content
  membership per object may not be reachable through the current bindings. If so,
  the fallback is to read the structure with `lopdf` — but then the lopdf/PDFium
  boundary reappears, and every disagreement must resolve to "keep". Decide this
  before writing code, not during.
- **Recovery:** the classifier is additive. Reverting it restores the `422`, which
  is fail-safe, so a rollback is never worse than today.

## Progress

- [ ] Confirm how per-object `/OC` membership is obtained (pdfium-render, raw
      PDFium bindings, or `lopdf`), and record the answer here.
- [ ] Implement the class-2 local checks (`3 Tr`, alpha zero).
- [ ] Implement class-1 detection including marked-content enclosure.
- [ ] Prove on the fixture list below.
- [ ] Withdraw the `422` and rewrite its test as a survival test.
- [ ] Corpus sweep, contract update.

## Decisions

- 2026-08-02: Refuse optional content with `422` now rather than widen the
  measurement. A two-configuration union closed one of five shapes and would have
  read as coverage.
- 2026-08-02: Scope the refusal to automatic mode. Manual coordinates are not
  measured, and were verified clean against all five shapes.
- 2026-08-02: Do not build the classifier in the same branch as the four patches.

## Validation

Any implementation must pass all of these, and the first group must **fail** if the
classifier is replaced by "keep everything":

- Deletion still happens: out-of-crop text, an out-of-crop transparent image, and
  an out-of-crop `3 Tr` run are all absent from the output; in-crop content and
  images survive.
- Optional content survives at `200`, for every shape:
  `/D /OFF`; `/OCMD /P /AllOff`; `/OCMD /P /AnyOff`; `/OCMD /VE [/Not g]`;
  `/OCG` with `/Usage /View /ViewState /OFF` and `/Print /PrintState /ON`; a group
  visible by default; a group referenced by a page but absent from
  `/OCProperties`; `/BaseState /OFF`; `/OC` on a Form XObject; nested `BDC`/`EMC`.
- Page geometry unchanged: the twelve shapes in
  `auto_crop_maps_pixels_through_rotation_and_page_boxes`.
- Hairlines: `auto_crop_removal_samples_every_pixel_so_a_hairline_is_not_deleted`.
- No dangling resource names on any fixture, both modes:
  `every_fixture_comes_back_without_a_dangling_resource_name`.
- Repository-required checks: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`.

## Result

Not started. The `422` refusal is in place and is the current, fail-safe behaviour.
