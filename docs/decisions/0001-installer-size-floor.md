# 0001 Installer Size Floor — the ~30 MB Question

Date: 2026-08-04

## Status

Proposed — awaiting a maintainer decision on the target and the path.

## Context

Two measurement-driven size audits (v0.0.5→v0.0.7 and the round-2 sweep
shipped in v0.0.9) have taken the Windows download from 82 MB to 44.7 MiB
(`-setup.exe`) with no feature loss. The maintainer has asked what reaching
**~30 MB** would take. Every figure below is a *compressed* (installer-side)
measurement from those audits — raw byte deltas overstate savings on this
project by roughly 3.5–8×.

The remaining mass at 44.7 MiB: the Rust sidecar with 166 endpoints
(~20 MB LZMA), the Tauri shell with the embedded 6.17 MB (brotli) frontend,
PDFium twice (native for the sidecar + wasm for client-side rendering), the
qpdf and Tesseract closures, and the Typst-based Office conversion stack.

**Technical levers cannot reach 30.** Exhausting every remaining one lands at
~35–37 MiB. Reaching ~30 requires removing product features (a "lite" SKU).

## Decision

None yet. The concrete options, each priced:

### Option A — NSIS dictionary bump: −3 MB, the last cheap win

Tauri exposes no dictionary knob; requires forking the NSIS `installer.nsi`
template into the repo (same maintenance shape as the existing
`windows/wix/main.wxs` fork: silent drift on every `@tauri-apps/cli` bump,
must be diffed and re-ported by hand). Measured: dict 64 MiB captures −2.9 MB
of the −3.1 MB that 192 MiB gives, and dictionary size is decompression RAM
on the *user's* machine during install — 64 MiB is the recommended setting;
192 MiB can hurt low-end machines. Reversible by deleting the fork.
Result: ~41.7 MiB.

### Option B — Typst embedded-font subset: −1 to −2.9 MB, tiered fidelity loss

The Office engine embeds 17 fonts (8.76 MB raw / 3.67 MB compressed) so every
document converts identically everywhere. Requires a small `typst-assets`
fork + one `[patch]` line.

- Safe tier (−1.3 MB): drop auxiliary weights (DejaVu Mono bold/oblique,
  Libertinus semibold). Bold code-block text substitutes to regular. Nearly
  nobody notices.
- Judgement tier (−1.5 MB more): drop NewCM10 + math-bold — the **equation
  fonts**. Word documents with complex math get glyph substitution: shifted
  alignment, missing rare symbols. If users convert academic/technical docx,
  this tier will generate reports — and they are *silent wrong-output* bugs,
  the hardest kind to trace from a user report.

The differential test harness can render fixtures under both configurations
**before** deciding. Easily reversible (re-pin).

### Option C — Self-built no-curl Windows Tesseract: −1.5 MB, supply-chain ownership

The Windows bundle currently comes from the official installer (community-
tested; someone else patches CVEs). Its libcurl chain (curl + unistring +
ssh2 + idn2 + psl + brotli) is ~1.5 MB compressed and *never used* — we only
OCR local files. Removing it means building Tesseract for Windows ourselves,
as already done for Linux (`build-tesseract-musl.sh` + pinned-digest release
asset — the template exists). One-time effort ~a day; the lasting cost is
owning the update cadence for Tesseract/leptonica/libpng CVEs on Windows.
Users see no behavioural change.

### Option D — `typst-library` fork: −2.2 MB, REJECTED unless desperate

wasmi (Typst plugins), citations, syntect (code highlighting) and hypher
(48-language hyphenation) are unconditional deps of `typst-library` and
*unreachable* from Office conversion — users lose nothing today. But Typst
moves fast; every office2pdf Typst bump means rebasing the fork, and a botched
rebase kills Office conversion outright. Worst risk/benefit ratio on this
list: an invisible benefit mortgaged against a flagship feature.

### Option E — Lite SKU: the only road to ~30

Drop Office conversion (−6–7 MB: Typst+office2pdf code and all embedded
fonts) and OCR (−3 MB: Tesseract closure + traineddata). Lite ≈ 34–35 MiB;
with Option A ≈ **31–32 MiB**.

The code side is the easy part — the repo already has the "endpoint whose
dependency is missing disables itself, frontend hides the tool" pattern
(used for optional native tools); cargo feature gates flow into it. The real
costs are operational:

- **Release matrix doubles.** The Windows leg is already the slowest; QA
  covers two SKUs every release.
- **The updater must fork channels.** A shared `latest.json` would silently
  "update" lite installs into full ones (or vice versa). Needs per-SKU
  platform keys (e.g. `windows-x86_64-nsis-lite`) and the app must know its
  own SKU — the same class of problem just solved for MSI-vs-NSIS, in one
  more dimension.
- **User-facing complexity.** The release page goes from 2 Windows choices
  to 4. "Which one do I download?" and "why can't my install convert Word?"
  become standing support traffic.
- Rolling back is easy in code but installed lite users remain in the field;
  their update channel must be maintained or migrated.

### Non-option — download-on-demand components

Downloading OCR/Office data on first use would shrink the installer while
keeping features, but it directly violates the privacy model rewritten in
v0.0.8 ("the startup update check is the only self-initiated request").
Listed for completeness; not recommended without deliberately amending that
promise.

## Alternatives Considered

Covered above as Options A–E; also RELR relocation packing (−0.28 MB,
rejected: requires glibc ≥ 2.36, Ubuntu 22.04 LTS deb users are on 2.35 and
the binary would not start) and worker chunk dedup (rejected: Vite worker
builds share no chunks with the main build; the estimated saving does not
materialise).

## Consequences

The decision tree, by what the ~30 MB target is *for*:

- Perception / "small download" marketing → **stop now**; 44.7 MiB already
  clears every sub-50 psychological bar. Every remaining MB costs more than
  the last.
- Bandwidth-constrained users → **A (+ B safe tier)** ≈ 40–41 MiB at near-zero
  risk. The recommended last profitable stop.
- A genuine minimal-deployment audience (low-end fleets, slow networks, no
  Office/OCR need) → **E**, sized 31–32 MiB, bought with permanent
  two-SKU operations. Decide based on whether that audience exists, not on
  the number.

## Follow-Up

- If A is taken: the NSIS template fork also becomes the natural home for the
  "Add desktop icon" installer option and any future NSIS customisation.
- If B is taken: run the differential harness across both font configurations
  first and attach the per-fixture diff to this record.
- If E is taken: design the per-SKU updater keys before the first lite
  artifact ships — retrofitting the channel split after installs exist in the
  field is much harder.
- Robustness item discovered during the same audit (not size): the bundled
  `libgnutls.so.30` declares `DT_NEEDED libgmp.so.10` but libgmp is not
  shipped; qpdf fails to load on minimal distros. Ship libgmp in the qpdf
  closure.
