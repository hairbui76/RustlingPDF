# Build speed and app size — audit and plan

Audited 2026-08-25 against `main` at `f7e1ebc` (v0.1.5). Every number below is
measured unless marked *est.*; sizes are compressed bytes (what a user
downloads) unless marked raw. This file exists so the next person — or the next
model — starts from evidence, not from a fresh scan.

## Where the time goes

| Stage | Windows leg | Linux leg | Source |
|---|---|---|---|
| `task desktop:stage-sidecar` (release build of `rustling-processing` + PDFium/qpdf/Tesseract) | 1009s | 398s | run 32802314790, warm cache |
| `tauri build` (Tauri shell release build + MSI/NSIS or deb) | 571s | 197s | same |
| Everything else (checkout, node, npm ci, cache, prepare, MSI lifecycle) | ~180s | ~120s | same |
| **Leg total, warm cache** | **~27 min** | **~13 min** | |
| **Leg total, cold cache** | **~50 min** (2911–2971s) | **~23 min** | runs 31683358304, 32743024812 |

Local, with dependencies already compiled: the workspace crate alone takes
**11m39s** — 187s codegen for the lib, then **511s in the fat-LTO link** of the
binary. That link is the part no cache can shorten; it runs twice per leg
(sidecar, then Tauri shell).

Thin LTO, same machine, measured after the switch: **8m51s for the whole
release build with every dependency recompiled from scratch** (the fat figure
above had warm dependencies and covers only the workspace crate). Binary
67.5 MB raw against fat's 63.5 MB — the +4 MB raw is ~+0.2 MB compressed,
matching the table in `rust/Cargo.toml`.

`task rust:check` locally is dominated by linking **91 separate integration
test binaries** (`rust/crates/rustling-processing/tests/*.rs`, no shared
`tests/common`), each of which links the whole crate.

## Where the bytes go (v0.1.4: setup.exe 44.7 · MSI 49.5 · deb 40.5 MiB)

- Sidecar binary: 63.5 MB raw, 500 crates. Subtrees: `office2pdf`/typst 307
  crates, PKI (`cryptographic-message-syntax`) 109, `svg2pdf` 96, `lettre` 75,
  `pdfium-render` 54. Embeds DejaVuSans + Bold (1.4 MB raw).
- Bundled natives: 26 MB raw — tesseract bin 11, qpdf lib 11, `eng.traineddata` 4.
- Frontend dist (desktop-pruned): 19 MB raw — `pdfium.wasm` 4.5, `index.js` 2.1,
  NotoSans ×4 faces 2.5, `pdf.worker` 1.26, `pdf-engine` 1.0, 12 locales 2.5.
- Three PDF stacks ship in the frontend: pdf.js, embedpdf/PDFium-wasm, pdf-lib.

## Findings

### F1 — Actions cache is over the 10 GB limit (10.94 GB)

GitHub evicts under pressure, which is why identical builds alternate between
~30 and ~50 minutes. The overflow is the Docker layer cache written by
`release.yml` with `cache-to: type=gha,mode=max` — buildkit blobs totalled
~3.3 GB at audit time and are regenerated every release. The four Rust caches
(1.1 + 1.8 + 2.3 + 1.8 GB) are legitimate.

### F2 — Every release builds the desktop matrix twice

`desktop-release-dryrun` and `release.yml` both run the full
`desktop-build.yml` matrix. The dry-run's value is catching bundler-level
breakage (unsigned bundles, missing `plugins.updater`), which only changes when
`src-tauri/`, the desktop workflows, or the desktop-tools scripts change.

### F3 — `lto = "fat"` buys 0.2 MB for 6m40s per binary

Measured in `rust/Cargo.toml`'s own comment: thin LTO captures 3.2 of fat's 3.4
MB saving at a third of the time. Applied in both workspaces, that is roughly
two binaries × two legs × ~4–6 min per release.

### F4 — 91 test binaries

Merging into ~8–10 group binaries (`tests/main.rs` + `mod`) cuts local and CI
link time by half or more without changing a test.

### F5 — Duplicate crate versions

`zip` ×3 (0.6 ← office2pdf, 2.4 ← umya-spreadsheet, 8.6 ← ours), `quick-xml`
×3 (0.37 ← umya, 0.38 ← office2pdf/citationberg, 0.41 ← ours), `kurbo` ×3
(0.11 ← usvg, 0.12 ← hayro/typst, 0.13 ← ours), `rand` ×3 (0.8 ← lipsum, 0.9 ←
governor), `hashbrown` ×3 (0.14 ← dashmap, 0.16 ← governor), `getrandom` ×3
(0.2 ← ring/umya, 0.3 ← ahash/governor), plus `png`, `sha2`, `moxcms` ×2. Bumping
`umya-spreadsheet` and the `office2pdf` fork clears most of it (~30–40 fewer
crates, ~0.3 MB *est.*).

### F6 — Frontend dependency hygiene

Verified with a scan over every source file including dynamic `import()`:
`d3`, `recharts`, `@tanstack/react-query`, `react-easy-crop` have zero
references. `@mui/material` + `@emotion/react` + `@emotion/styled` also have
zero references but are peer dependencies of `@mui/icons-material` (414 icon
imports), so npm reinstalls them regardless; dropping them means migrating icons
to `@iconify/react`, which is already a dependency. `tailwindcss`,
`@tailwindcss/postcss`, `autoprefixer`, `license-report`, `globals` are build
tooling filed under `dependencies`.

### F7 — Size levers, priced (unchanged from the v0.0.9 audit)

| Lever | Saves | Effort |
|---|---|---|
| Drop NotoSans Italic + BoldItalic fallback faces — **not safe**: `fallbackFonts.ts` documents that the engine falls back to an upright face when no italic exists, so italic text in a document without embedded fonts would render upright; a fidelity call for the maintainer | ~0.5 MB | one list edit |
| NSIS `SetCompressorDictSize` 64 MiB (template already forked) | ~3.1 MB setup.exe *est.* | one line |
| Typst embedded font subset | 1.0–2.9 MB | typst-assets fork; fidelity call |
| typst-library dead subsystems (`syntect`, `hypher`, `hayagriva`, `citationberg`, `wasmi`) | ~2.2 MB *est.* | typst-library fork |
| Tesseract Windows no-curl build | ~1.5 MB | high |
| Drop `pdfium.wasm`, render through the sidecar | 1.83 MB | architectural |
| DejaVu in sidecar vs NotoSans in frontend | ~0.4 MB *est.* | check shareability |

## Do not

- Strip `libqpdf` — loads, answers `--version`, segfaults on the first document.
- Change `panic = "unwind"` — 7.8 MB raw, but one bad PDF would take the service down.
- Re-run `cargo bloat` looking for more — Office (15.4 MB) and OCR (4.5 MB) are content, not bloat.

## Plan

**Batch 1 (safe, shipped as v0.1.6):** F1 Docker cache `mode=min`; F2
dry-run rule in RELEASING.md; F3 thin LTO in both workspaces; F7 NSIS
dictionary; F6 remove the four unreferenced packages and refile the tooling.

**Batch 2 (needs a decision or a fork):** F4 test-binary merge; F5 dependency
bumps; F6 icon migration to drop the MUI/Emotion trio; F7 NotoSans italics
(fidelity call), typst forks.

**Measured after batch 1 (v0.1.6, run 32884313597, warm thin cache):**

| | v0.1.4 (fat, warm) | v0.1.6 (thin, warm) | Δ |
|---|---|---|---|
| Windows `stage-sidecar` | 1009s | 680s | **−33%** |
| Windows `tauri build` | 571s | 555s | −3% |
| Windows leg total | 1829s | 1542s | −16% |
| Linux `stage-sidecar` | 398s | 288s | −28% |
| Linux leg total | 752s | 663s | −12% |
| setup.exe | 44.6 MiB | 43.6 MiB | −1.0 MiB |
| MSI | 49.6 MiB | 50.1 MiB | +0.5 MiB |
| deb | 40.5 MiB | 40.9 MiB | +0.4 MiB |

Read it straight: thin LTO delivered on the sidecar (−33%) but the Tauri
shell's `tauri build` step barely moved — that step is mostly vite build,
bundling and signing, not the shell's link. The NSIS dictionary's ~−3 MB was
partly eaten by thin's +~0.2 MB per binary on setup.exe, and the MSI and deb,
which have no dictionary lever, simply got the thin cost. The dry-run that
preceded this release ran cold (2612s Windows) because the profile change
invalidated every cached dependency — a one-time cost. With the dry-run now
skipped for non-packaging releases, a release is ~26 min of desktop matrix
instead of ~55. Stale mode=max Docker blobs were deleted by hand after this
run; cache stood at 9.78 GB before that.

The ~20-minute figure predicted above was optimistic by the `tauri build`
step; the next lever for that step is the frontend build itself, not LTO.
