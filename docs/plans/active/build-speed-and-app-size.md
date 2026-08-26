# Build speed and app size — audit, plan, and results

First audited 2026-08-25 against `f7e1ebc` (v0.1.5); re-scanned 2026-08-26 at
`c119081` after batches 1 and 2 landed. Every number is measured unless marked
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
| Release wall-clock (dry-run skipped for non-packaging releases) | ~26 min | ~55 min |
| Local release build of the sidecar, deps cold | 8m51s (thin) | 11m39s for the workspace crate alone, deps warm (fat) |
| Local `cargo test --no-run`, processing crate | 2m44s, 19 targets | 5m06s compile + 91 links |
| Local processing suite runtime | 1m31s | (not measured; dominated by links) |
| setup.exe / MSI / deb | 43.6 / 50.1 / 40.9 MiB (v0.1.6) | 44.7 / 49.5 / 40.5 |
| Sidecar crates in graph | 500 | 500 |
| Crate names with ≥2 versions | 58 (`zip 0.6.6` gone; `zip` and `quick-xml` still ×2/×3 via the umya fork and citationberg) | 58 |
| Frontend `dependencies` | 60 | 73 |
| `frontend/node_modules` | 947 MB | 1.2 GB |
| Frontend dist, desktop-pruned, raw | 18 MB (assets 13, locales 2.5, fonts 1.6) | 19 MB (fonts 2.8) |
| Frontend code (js+css+wasm), gzip | 4.26 MB | not measured |
| Actions cache | 9.54 GB of 10 | 10.94 GB (over limit) |

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

## Findings still open, ranked by what they are worth

### F-A — The `tauri build` step (555s Windows / 196s Linux)

Now the largest stage thin LTO did not touch. It runs `vite build --mode core`
(the desktop frontend), then the shell's release build, then MSI+NSIS
packaging and signing. Not yet split into its parts on a runner; the local
desktop-mode vite build is fast, so the suspicion is the shell link plus two
installer packagings. Measure before touching: add `--timings`-style step
splits to the workflow, then decide.

### F-B — Cache pressure: 9.54 GB of 10

Thin-LTO caches are larger than fat (Linux 1.8 → 2.8 GB, Windows 2.3 →
3.2 GB). `save-if` stops branches from adding keys but does not shrink the
four that exist. Cheapest next step: drop `v0-rust-desktop` (1.1 GB, the
desktop *CI* workflow's shell-only debug cache, which overlaps the release
shell cache) by pointing that workflow at `cache-targets: false`, or accept
that the backend CI cache (1.8 GB) is the next to go under pressure.

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
