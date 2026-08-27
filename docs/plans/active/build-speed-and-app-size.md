# Build speed and app size — audit, plan, and results

First audited 2026-08-25 against `f7e1ebc` (v0.1.5); re-scanned 2026-08-26 at
`c119081` after batches 1 and 2 landed; cache and `tauri build` re-measured
2026-08-27 at `3371854`. Every number is measured unless marked
*est.*; sizes are compressed bytes (what a user downloads) unless marked raw.
This file exists so the next person — or the next model — starts from
evidence, not from a fresh scan. Sections are ordered so the current state
comes first and the history of how it got there comes after.

## Current state (2026-08-26)

| Metric | Now | Was (v0.1.4 / first audit) |
|---|---|---|
| Windows desktop leg, warm cache | 1542s (v0.1.6 release run) | 1829s |
| Linux desktop leg, warm cache | 663s | 752s |
| Windows `stage-sidecar` | 680s | 1009s |
| Windows `tauri build` | 555s | 571s |
| Release wall-clock (dry-run skipped for non-packaging releases) | ~19 min *est.*, next release confirms | ~55 min |
| Windows desktop leg | 1099s (sidecar overlaps the shell compile, F-A2) | 1829s |
| `tauri build` phases, Windows | 62% of it the shell crate's codegen+link — at its floor short of accepting installer growth (F-A) | cause unknown |
| Local release build of the sidecar, deps cold | 8m51s (thin) | 11m39s for the workspace crate alone, deps warm (fat) |
| Local `cargo test --no-run`, processing crate | 2m44s, 19 targets | 5m06s compile + 91 links |
| Local processing suite runtime | 1m31s | (not measured; dominated by links) |
| setup.exe / MSI / deb | 43.1 / 49.6 / 40.4 MiB (v0.1.7) | 44.7 / 49.5 / 40.5 |
| Sidecar crates in graph | 500 | 500 |
| Crate names with ≥2 versions | 58 (`zip 0.6.6` gone; `zip` and `quick-xml` still ×2/×3 via the umya fork and citationberg) | 58 |
| Frontend `dependencies` | 60 | 73 |
| `frontend/node_modules` | 947 MB | 1.2 GB |
| Frontend dist, desktop-pruned, raw | 18 MB (assets 13, locales 2.5, fonts 1.6) | 19 MB (fonts 2.8) |
| Frontend code (js+css+wasm), gzip | 4.26 MB | not measured |
| Actions cache | 7.20 GB of 10 | 10.94 GB (over limit) |

Largest single items, unchanged: `pdfium.wasm` 4.5 MB raw (1.83 compressed),
`index.js` 2.07 MB raw, `pdf.worker.min.mjs` 1.26, `pdf-engine` 1.02, the two
remaining NotoSans faces 1.2, `pixelCompareWorker` 0.46. Three PDF stacks still
ship in the frontend (pdf.js, embedpdf/PDFium-wasm, pdf-lib).

## What shipped

### Batch 1 — v0.1.6 (`e045d19`)

- **Thin LTO** in both cargo workspaces (was fat). Sidecar stage −33% on
  Windows, −28% on Linux; +~0.2 MB compressed per binary. The `tauri build`
  step barely moved (−3%): it is mostly vite build, bundling and signing, not
  the shell's link.
- **Docker layer cache `mode=max` → `mode=min`** in `release.yml`. The
  intermediate-stage blobs (~3.3 GB, regenerated every release) were what
  pushed the repository over the 10 GB Actions limit and got the desktop Rust
  caches evicted — the cause of identical builds alternating between ~30 and
  ~50 minutes.
- **NSIS `SetCompressorDictSize 64`** in the forked `installer.nsi`. Decision
  0001 had priced it at −3 MB and declined it because the fork did not exist;
  the fork has existed since 0.1.0. Net on setup.exe: −1.0 MiB after thin
  LTO's cost.
- **Dry-run scoped** in RELEASING.md to releases that touch the packaging
  surface (`src-tauri/`, desktop workflows, desktop-tools scripts).
- Four unreferenced frontend packages removed; five tooling packages refiled
  under `devDependencies`.

### Batch 2 — v0.1.7 (`d631d1c`..`c119081`)

- **Integration tests: 91 targets → 19** (`d631d1c`). 78 files moved under
  `tests/cases/` as modules of six domain binaries; 13 stay standalone
  because they need a fresh process (environment variables, current
  directory, `current_exe`, and `text_editor_to_pdf_endpoint`, which binds
  PDFium itself — pdfium-render allows one binding per process and the
  crate's `PDFIUM` OnceLock has usually done it first). The set of test names
  is unchanged: 950 before and after, module prefixes stripped.
- **Icon set migrated** (`c3e2809`): 414 `@mui/icons-material` imports in 124
  files now use the offline Material Symbols bundle via `LocalIcon`.
  `@mui/material` and `@emotion/*` — zero references, present only as peers —
  are gone. Component-shaped call sites (icon maps, `typeof` fields,
  ternaries) go through `materialSymbol(name)` in `LocalIcon.tsx`, and
  `generate-icons.js` scans for that literal. 170 of 181 icons mapped by name,
  11 by hand. Material Symbols glyphs differ slightly from Material Icons;
  accepted.
- **Two Latin fallback faces instead of four** (`d89063e`): italic text in a
  document that failed to embed its font now renders upright; ~0.5 MB per
  installer.
- **`save-if`** on every rust-cache step (`ed083c6`): only main and release
  tags write cache keys.
- **`office2pdf` fork: `zip 0.6 → 8.6`** (fork commit `b62c705` on `perf/dedupe-deps`,
  pinned here). `SimpleFileOptions` was the only source change; the fork's
  suite passed on it. `zip 0.6.6` and its private dependencies leave this
  graph; `zip 2.4` stays via the umya fork (F-D). A `quick-xml 0.38 → 0.41`
  bump was tried and reverted, for two reasons worth recording: (1)
  `citationberg` (typst) still needs 0.38, so the bump removed no duplicate;
  (2) in this workspace another crate enables quick-xml's `encoding`
  feature, which hides `Attribute::unescape_value` — 91 call sites in
  office2pdf that compile fine in the fork's own workspace fail here.
  Feature unification is per graph; a dependency bump verified only in its
  own repo is not verified.

### Measured after batch 2 (v0.1.7, run 32929594162)

| | v0.1.6 | v0.1.7 | Δ |
|---|---|---|---|
| setup.exe | 43.6 MiB | 43.1 MiB | −0.5 |
| MSI | 50.1 MiB | 49.6 MiB | −0.5 |
| deb | 40.9 MiB | 40.4 MiB | −0.5 |
| Windows leg | 1542s | 1467s | −5% |
| Linux leg | 663s | 1138s | cold: `Cargo.lock` changed with the office2pdf pin, so the rust-cache key missed and the sidecar stage ran 602s instead of 288s; the tauri step also doubled (396s). One-time; the next tag restores the warm key. |

The −0.5 MiB is the two dropped fallback faces plus the smaller frontend
bundle after the icon migration. The zip dedupe and the test regroup are
build-time changes and do not show in the artifacts.

## Findings still open, ranked by what they are worth

### F-A — The `tauri build` step: 62% of it is one crate's codegen

**Measured** on the v0.1.7 Windows leg (run 32929594162, job 98059222803) by
reading the step's own timestamps out of the CI log — no workflow change was
needed, and none should be made to measure this again:

| Phase | Time | Share |
|---|---|---|
| `vite build --mode core` (the `beforeBuildCommand`) | 24s | 4.2% |
| cargo: the shell's 359 dependencies | 40s | 7.0% |
| **cargo: the `rustlingpdf` shell crate + link** | **350s** | **61.6%** |
| MSI — WiX candle ×2 + light | 71s | 12.5% |
| NSIS — makensis | 81s | 14.3% |
| | 568s | |

An earlier note here guessed this step was "mostly vite build, bundling and
signing, not the shell's link". That was wrong, and the correction is the
useful part: it is almost entirely the shell's own codegen and link, which is
also why thin LTO barely moved it (571s → 555s → 568s across three releases).

**`codegen-units = 1` was the suspect, and it is not the answer.** Measured on
branch `perf/shell-codegen-units` (dry-run 32994685855, since deleted), with
16 — cargo's release default — against the shipped v0.1.7 build:

| | cgu = 1 (shipped) | cgu = 16 | Δ |
|---|---|---|---|
| shell crate + link | 350s | 325s | **−25s (−7%)** |
| setup.exe | 43.05 MiB | 43.34 MiB | **+0.29 MiB** |
| MSI | 49.61 MiB | 49.98 MiB | **+0.37 MiB** |

Not merged. Batch 1 and batch 2 together bought about 0.5 MiB an installer;
handing 0.3 back for 25 seconds a leg is the wrong direction for a project
that has chosen size at every previous fork in this file.

What the negative result tells us is worth more than the 25s would have been:
the cost is not parallelism in codegen, it is the crate itself — one unit that
embeds the whole frontend, the Tauri runtime and 359 linked dependencies. The
remaining knob pointing the same way is `lto = false` for the shell, which
would trade more size for more time on the same axis; do not spend a dry-run
on it without a decision to accept installer growth first.

**On the compile itself, F-A is closed: no cheap win there.** But the
measurement pointed somewhere else that was free.

### F-A2 — Overlapping the two cargo builds: −25% a leg, no size cost (done)

The leg was building two *independent* cargo workspaces back to back — the
sidecar (645s) and then the shell (350s) — and both are largely
single-threaded codegen and linking, on a four-core runner. Nothing needs the
sidecar binary until the bundler does.

Since `perf/parallel-sidecar-shell`: the sidecar starts in the background
right after the Rust cache restore, `tauri build` is split into its
`--no-bundle` and `bundle` halves, compile-gate stubs stand in for the
externalBin and resources the compile only checks the existence of, and a
join step waits on the background build's exit code before staging the real
binary. `task desktop:prepare:no-sidecar` exists so the prepare step does not
block on the very build that is already running.

Measured (dry-run 33054543374, against the v0.1.7 release run):

| | before | after | Δ |
|---|---|---|---|
| Windows leg | 1467s | **1099s** | −368s (−25%) |
| Linux leg | 663s | **465s** | −198s (−30%) |
| — shell compile (windows) | 414s | 631s | +217s: it now shares the runner with the sidecar |
| — separate sidecar stage | 659s | 0s | absorbed into the overlap |
| — join (wait + stage) | — | 75s | |
| — bundle | 152s | 136s | |
| setup.exe / MSI / deb | 43.05 / 49.61 / 40.4 MiB | 43.04 / 49.60 / 40.42 | identical |

The overlap is not free — CPU contention stretched the shell compile by
217s — and it is still worth 368. Release wall clock should land near
**19 minutes**, from 25.

With that, the desktop legs are near their floor unless the size policy
changes: what is left inside them is the two crates' own codegen and link.

To re-measure the phases:

```bash
JOB=$(gh api repos/hairbui76/RustlingPDF/actions/runs/<id>/jobs   --jq '.jobs[] | select(.name|contains("windows-x86_64")) | .id')
gh api repos/hairbui76/RustlingPDF/actions/jobs/$JOB/logs > win.log
grep -nE "beforeBuildCommand|Running .cargo build|Compiling rustlingpdf|Built .tauri_cli|msi] Verifying|nsis] Verifying|bundles at" win.log
```

### F-B — Cache pressure: fixed, 7.20 GB of 10 (was 9.54)

Two leaks, both of them GitHub's ref scoping, found by listing caches with
their `ref` (`gh cache list --json key,sizeInBytes,ref`):

1. `save-if` let release **tags** write rust-cache keys. A cache is scoped to
   the ref that wrote it, so those landed under `refs/tags/vX.Y.Z` where the
   next tag — a different ref — cannot read them, and neither can main.
   v0.1.7 left a 1.79 GB Linux copy nothing would ever restore. Tag runs
   still restore from main; they must not write. Fixed in `3371854`.
2. The **Docker layer cache** in `release.yml` had the same scoping problem
   and no upside: only that workflow writes `scope=release` and only tags run
   it, so `cache-from` had never hit once. The two image builds share their
   common stages through the builder's local state inside the run, not
   through the export — which is why `mode=max` → `mode=min` made
   publish-images *faster* (1275s at v0.1.5 → 836s at v0.1.6), not slower.
   Removed entirely in `3371854`; same-tag re-runs now rebuild the images,
   the accepted trade.

Deleting the orphans took the repository from 9.49 GB to 7.20 GB. What
remains is four legitimate keys: Windows release 3.2 GB, Linux release
1.8 GB, backend CI 1.8 GB, npm 0.6 GB. `v0-rust-desktop` (1.1 GB, the
Desktop CI shell debug cache) had already been evicted before this was
looked at, which is its own signal about how tight the budget had become.

Do not chase the remaining four with `cache-targets: false`: Desktop CI
finishes in ~120s *because* of its cache, and rebuilding 359 shell
dependencies per run would trade 2 minutes of wall clock for 1 GB.

### F-C — Remaining duplicate crate versions (58 names)

With the office2pdf pin moved: `zip` ×2 (2.4 ← umya fork; 8.6 ← ours),
`quick-xml` ×3 (0.37 ← umya fork, 0.38 ← citationberg, 0.41 ← ours), `kurbo`
×3 (0.11 ← usvg, 0.12 ← hayro/typst, 0.13 ← ours), `rand` ×3 (0.8 ← lipsum,
0.9 ← governor, 0.10 ← ours), `hashbrown` ×3 (0.14 ← dashmap, 0.16 ←
governor), `getrandom` ×3, `png`/`sha2`/`moxcms` ×2. The umya pair is the
umya decision below; the rest are upstream typst/resvg/governor version skew
and clear themselves on the next typst bump. Worth ~0.3 MB *est.* and a few
dozen crates of compile; not worth forking for.

### F-D — `umya-spreadsheet` stays on the fork (decision, 2026-08-26)

Upstream 3.x already uses the zip/quick-xml/sha2/rand versions we want, but it
drops `set_argb(&str)`, `Border::get_color_mut`, `DataBar::set_{min,max}_length`
and `IconSet::set_icon_set_type`, so office2pdf's xlsx test fixtures cannot be
built with the 3.x writer, and the fork's four behaviour fixes (whitespace,
xfId, number-format literal, dataBar cfvo) are not all upstream. The port was
carried far enough to know this — the office2pdf lib compiles against 3.1.1;
only the fixtures block — and stopped there: Office→PDF fidelity for ~0.3 MB
and ~35 crates is the wrong trade. Revisit only if upstream regains the
setters or the fork's fixes land there.

### F-E — Typst forks (unchanged, still priced)

| Lever | Saves | Effort |
|---|---|---|
| Embedded font subset: `typst-assets` ships 8.7 MB raw of fonts; office2pdf names Libertinus Serif (default), Liberation Sans/Serif, and Noto CJK — NewCM10 text faces, Libertinus Semibold pair and DejaVu Mono obliques are candidates | 1.0–2.9 MB | typst-assets fork; fidelity call |
| typst-library dead subsystems (`syntect`, `hypher`, `hayagriva`, `citationberg`, `wasmi`) | ~2.2 MB *est.* | typst-library fork |

### F-F — Other size levers (unchanged)

| Lever | Saves | Effort |
|---|---|---|
| Drop `pdfium.wasm`, render through the sidecar on desktop | 1.83 MB | architectural: all client-side rendering crosses IPC |
| Tesseract Windows no-curl build | ~1.5 MB | high |
| DejaVu in sidecar (1.4 MB raw) vs NotoSans in frontend | ~0.4 MB *est.* | check shareability |
| Consolidate three frontend PDF stacks | large | product-level |

## Do not

- Strip `libqpdf` — loads, answers `--version`, segfaults on the first document.
- Change `panic = "unwind"` — 7.8 MB raw, but one bad PDF would take the service down.
- Re-run `cargo bloat` looking for more — Office (15.4 MB) and OCR (4.5 MB) are content, not bloat.
- Bump `umya-spreadsheet` to 3.x without reading F-D.
- Re-add `@mui/material` for a component: 193 MB of node_modules for a peer nothing imports.
- Retry `codegen-units` on the desktop shell: measured, +0.3 MiB for −25s (F-A).
- Let release tags write Actions caches: GitHub scopes them to the tag ref, so
  nothing can ever read them back (F-B).

## How to re-measure

- CI stage times: `gh api repos/hairbui76/RustlingPDF/actions/runs/<id>/jobs`
  and diff `completed_at − started_at` per step.
- Local build: `cargo build --release --locked -p rustling-processing --timings`
  from `rust/` with `RUSTLING_PDFIUM_LIBRARY_PATH` set.
- Test targets: `ls rust/crates/rustling-processing/tests/*.rs | wc -l`.
- Duplicates: `cargo tree -p rustling-processing --locked -e normal --duplicates`.
- Frontend dist: `RUSTLING_DESKTOP_BUILD=1 npx vite build --mode core` in
  `frontend/editor`, then `du -sh dist/*`.
- Cache: `gh cache list --json key,sizeInBytes`.
