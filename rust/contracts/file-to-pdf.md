# `POST /api/v1/convert/file/pdf`

Current contract for office-to-PDF conversion.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one office/text document, required
- Success returns the original base name suffixed with `_convertedToPDF.pdf` as
  `application/pdf`.

Response headers describing the conversion:

| Header | When | Meaning |
| --- | --- | --- |
| `X-Rustling-Conversion-Engine` | always | `libreoffice` or `builtin` |
| `X-Rustling-Conversion-Degraded` | when `true` | source content is known to be missing from the PDF |
| `X-Rustling-Conversion-Warnings` | when non-zero | count of non-fatal warnings |
| `X-Rustling-Conversion-Warning-Detail` | when non-zero | the warnings, `; `-joined, reduced to printable ASCII, truncated at 1024 characters |

The body is a PDF, so these headers are the only place a partial conversion can
be reported. **A `200` alone is not evidence that the document survived.** A
caller that must not silently lose content has to read
`X-Rustling-Conversion-Degraded`. The same information is written to the server
log at `warn` level. The SPA does not read these headers today.

## Engines

Two engines back this endpoint.

1. **LibreOffice** — better fidelity, covers every accepted extension. Used
   whenever a `soffice` binary can be started.
2. **Built-in** (`office2pdf`, pure Rust, Typst-backed) — DOCX, XLSX, and PPTX
   only, no external tool required. Used when LibreOffice is not installed.

`RUSTLING_PROCESSING_OFFICE_ENGINE` selects between them:

| Value | Behavior |
| --- | --- |
| unset / `auto` (default) | LibreOffice if it starts, otherwise the built-in engine |
| `libreoffice` / `soffice` | LibreOffice only; `501` when it is missing |
| `builtin` / `office2pdf` | built-in only, even if LibreOffice is installed |

Any other value is a configuration error and returns `500`.

Both engines receive the same sanitized input (see *Supported inputs*). A
LibreOffice run that *starts* and then fails is never retried on the built-in
engine: repeating the work would hide the real diagnostic.

### LibreOffice path

```
soffice -env:UserInstallation=file://<profile> --headless --nologo \
        --convert-to pdf --outdir <workdir> <input>
```

A fresh temporary `UserInstallation` profile is used per request so concurrent
conversions do not collide and the host profile is untouched. The produced PDF
is located at `<workdir>/<base>.pdf`, falling back to any `.pdf` in the working
directory (some LibreOffice builds emit a different name). An empty output is
treated as a failure.

The `soffice` binary is resolved from `RUSTLING_PROCESSING_SOFFICE_COMMAND` when
set, otherwise from platform defaults (`soffice`/`/usr/bin/soffice`, or the
`soffice.com`/`soffice.exe`/`soffice` chain on Windows).

### Built-in path

The built-in engine is a library, and a hostile or merely malformed document can
make it loop forever, overflow the stack, or allocate without bound — none of
which an in-process call can survive. It therefore runs **out of process**: the
service re-invokes its own executable as `--office2pdf-worker <input> <output>
<report.json>` and supervises it. The worker executable can be overridden with
`RUSTLING_PROCESSING_OFFICE_WORKER_COMMAND`.

Bounds, all applied per request:

| Bound | Default | Environment override | On breach |
| --- | --- | --- | --- |
| Upload size | 50 MiB | `RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_INPUT_BYTES` | `400` |
| Worksheet rows (XLSX, summed across sheets) | 20 000 | `RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_SHEET_ROWS` | `400` |
| Wall clock | 120 s | `RUSTLING_PROCESSING_OFFICE_BUILTIN_TIMEOUT_SECONDS` | worker killed, `400` |
| Worker resident set | 2048 MB | `RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_MEMORY_MB` | worker killed, `400` |
| Concurrent conversions | 1 | `RUSTLING_PROCESSING_OFFICE_BUILTIN_MAX_CONCURRENCY` | request waits for a slot |

The size and row bounds are checked in the parent before a worker is spawned;
row counting stops as soon as the limit is passed, so the reported figure is a
lower bound ("at least N rows"). The time and memory bounds are enforced by
polling the child every 10 ms and killing it (`SIGKILL` / `TerminateProcess`).

**Parts are located the way the engines locate them — through relationships,
never by filename convention.** OOXML part paths come from `.rels` targets, so a
standards-legal workbook may put its only worksheet at `xl/data/s1.xml` and a
legal deck may name its slides `ppt/slides/d1.xml`. The row bound therefore
resolves `<sheet r:id>` through `xl/_rels/workbook.xml.rels`, and the slide
reconciliation resolves `<p:sldId r:id>` through
`ppt/_rels/presentation.xml.rels`, in both cases joining the target against the
directory of the declaring part and normalising `.`/`..`. External relationship
targets are skipped, and a target that climbs above the package root is refused.
A part the directory part never references is not counted, because the engine
will not read it either. `<sheet>` and `<sldId>` elements are matched at any
depth, because the engines' readers are event-based and do not care how deeply
an element is nested.

**Discovery fails closed.** There is no naming-convention fallback, and it is
worth saying why, because the obvious design is wrong: a convention scan of a
package that keeps its parts elsewhere finds nothing, and "nothing" reads as "no
rows" and "no slides" — which is the evasion the relationship walk exists to
prevent, reinstated through the back door. Anything that stops the relationship
walk from answering (a directory part or `.rels` that is absent, larger than the
16 MiB read cap, or unparseable) therefore falls to the package's own
content-type list, `[Content_Types].xml`, which types every part regardless of
where it lives. If that cannot be read either, the answer is *undeterminable*
and each surface takes the only safe direction available to it:

- the **row count** widens to every XML part in the package. It can then
  over-count, which at worst rejects a package the engine could not have read;
  it can never under-count.
- the **slide expectation** becomes unknown, and the response says so
  (`X-Rustling-Conversion-Warning-Detail`: "the deck does not say which slides
  it contains, so the page count could not be verified"). It does not claim a
  loss there is no evidence for, and it does not claim a clean conversion
  either. `X-Rustling-Conversion-Degraded` stays absent, so its meaning —
  "content is known to be missing" — remains exact.

A part that reaches the read cap is reported truncated and is never handed to
the parser. Parsing half a document yields a syntax error indistinguishable from
"this package declares nothing", which is precisely how an oversized `.rels` was
used to make the row bound vanish. The cap matches the office sanitizer's own
16 MiB per-XML-part limit, so a `.rels` that trips it cannot have arrived
through the sanitizer in the first place.

The undeterminable branch is reachable, and an earlier version of this contract
was wrong to imply otherwise. It claimed the office sanitizer rejects such a
package first; it does not. `is_targeted_xml_part` covers `.rels` files and the
ODF parts, so `xl/workbook.xml`, `ppt/presentation.xml`, and
`[Content_Types].xml` are neither parsed nor size-capped by the sanitizer, and
packages that make all of them unreadable pass it cleanly. The behaviour was
right regardless — an oversized `presentation.xml` is rescued by the
content-type source, an opaque deck reports the unverified-slides warning, and
an opaque workbook counts rows across the whole package and rejects — but the
justification was false, which is worse than no justification. "This cannot
happen" was not a safety argument even when it looked true.

Failures caused by the document — an unparseable package, a timeout, a memory
breach, a stack overflow that aborts the worker — are reported as `400` with a
message naming what tripped. They are properties of the upload, not server
faults.

### Known built-in-engine failure modes and how they are handled

These were measured, not assumed. Each is contained by killing or replacing the
worker process, never by fixing the engine.

| Failure mode | Containment |
| --- | --- |
| Non-terminating loop in the DOCX paragraph reader (100 % CPU, unkillable in-process) | wall-clock timeout kills the worker. **This is the only real mitigation**: a watchdog cannot cancel a stuck thread, which is why the conversion is a subprocess at all. |
| Stack overflow from mutual recursion in the DOCX table reader (aborts the process; the engine's own `MAX_TABLE_DEPTH` guard fires too late to help) | the abort kills the worker, not the service; reported as `400`. |
| XLSX memory growth of roughly 77 MB per 1000 rows (100k rows ≈ 7.7 GB) | the row bound rejects the workbook up front; the memory bound catches whatever slips past. LibreOffice handles 500k rows in ~345 MB, so large spreadsheets are a reason to install it. |
| Whitespace in a document body expanding by two orders of magnitude (25 MB → 4.4 GB) | not caught by any input rule — the package is structurally ordinary. The memory and time bounds stop it. Larger variants are rejected earlier by the office sanitizer's 200 MiB expansion cap. |
| `comemo` memoisation and font-metric caches that are never evicted, so memory grows with attacker-controlled input across requests | the worker exits after each conversion, so every cache dies with it. `comemo::evict` is never called because nothing survives long enough to need it. |
| Whole PPTX slides silently dropped while the engine reports zero warnings | the parent counts the slide parts `ppt/presentation.xml` references — excluding those marked `show="0"` — before conversion, and PDF pages after; a shortfall sets `X-Rustling-Conversion-Degraded` and adds an explicit warning. |

Residual risks, stated plainly:

- A document that stays just under every bound can still occupy the single
  conversion slot for the full timeout. The worst case is a queue, not an outage.
- **Loss instrumentation exists for PPTX only.** DOCX and XLSX have no
  independent check at all: there is no per-page or per-sheet invariant to
  reconcile against, so content dropped from a Word document or a workbook is
  invisible unless the engine itself reports a warning — and its warnings have
  been observed to be absent when content was lost.
- Silent data loss *inside* a rendered slide or paragraph (wrong glyphs, lost
  formatting, a dropped shape) is not detected either; only whole missing PPTX
  pages are. The absence of `X-Rustling-Conversion-Degraded` does not prove
  fidelity.
- The row count matches `<row` textually rather than parsing the worksheet. It
  can overcount a file that embeds that string elsewhere; it is a bound, not a
  statistic.
- The hidden-slide check reads only the root element's start tag of each slide
  part; it does not parse the slide. A start tag that cannot be read is treated
  as visible, which can only over-count and never hide a dropped slide.

  Within that tag it **mirrors `quick-xml` 0.38, the parser the engine uses**,
  because any disagreement is a wrong verdict in one direction or the other:
  a slide wrongly called visible produces a degraded verdict on an honest deck,
  and one wrongly called hidden lowers the expected page count until
  `pages >= expected` holds, which can hide a slide dropped elsewhere. So:
  the element's local name must be `sld`; the tag ends at the first `>` outside
  a quoted value (`ElementParser`); a processing instruction ends at `?>`, a
  comment at `>` after `--`, a CDATA section at `>` after `]]`, and a doctype at
  the first `>` outside a nested `<...>` counted by `<`/`>` balance
  (`PiParser`, `BangType`) — including its internal subset, and, like
  `quick-xml`, not quote-aware there; a malformed attribute is skipped and
  parsing continues rather than abandoning the tag (`Attributes` in XML mode,
  which the engine drives with `.flatten()`); `show` is matched by qualified or
  local name; its value is XML-unescaped, with lowercase `x` only for
  hexadecimal, a leading `+` or `-` refused, and `&#0;` rejected; and only the
  literal `0` and `false` count as hidden.

  This is a deliberate coupling to a specific parser's behaviour, quirks
  included, pinned alongside the pinned engine revision. The end-to-end tests
  assert against the engine's real page count rather than against this crate's
  expectation, so a `quick-xml` bump that changes any of the above shows up as a
  test failure instead of as a silently wrong verdict.
- The memory bound is a 10 ms poll of the worker's resident set, not a hard
  allocation ceiling. It has been observed to fire reliably at the scales tested,
  but a single allocation large enough to complete between two polls can overshoot
  the limit before the kill lands. A hard ceiling would need `setrlimit`, which
  the workspace's `unsafe_code = "forbid"` rules out.

## Supported inputs

Common LibreOffice office/text/presentation/spreadsheet extensions are accepted
(`doc`, `docx`, `odt`, `rtf`, `txt`, `xls`, `xlsx`, `ods`, `csv`, `ppt`, `pptx`,
`odp`, `html`, `htm`, …). HTML is decoded lossily as UTF-8 and passed through the
same strict Rust sanitizer used by `html-to-pdf`: scripts/active tags, external or
absolute image sources, traversing paths, URL-valued CSS, and unsafe data URLs are
removed before LibreOffice receives the file. Unknown extensions return `400 Bad Request`.

Without LibreOffice, only `docx`, `xlsx`, and `pptx` convert; every other
accepted extension returns `501 Not Implemented` naming the missing engine.

OOXML and ODF ZIP packages are rewritten before conversion, for **both** engines.
External OOXML relationships and external `href` attributes in ODF `content.xml`,
`styles.xml`, `meta.xml`, and `settings.xml` are removed. The sanitizer streams
non-XML entries, does not expand the package onto disk, and rejects traversal
paths, symbolic links, case-insensitive duplicate names, unsupported compression,
DTD-bearing or malformed target XML, more than 100,000 entries, more than 200 MiB
expanded data, or a targeted XML part larger than 16 MiB. Macro-enabled package
extensions are accepted, but this pass neutralizes external references without
deleting VBA payloads.

## Limitations

- Every external office-package target is stripped; there is no unsafe
  sanitization bypass.
- Conversion invokes `soffice` directly; it does not use a persistent
  `unoconvert` server.
- The built-in engine's fidelity is materially below LibreOffice's. It is the
  fallback that makes the feature work everywhere, not a replacement.

## Availability

The endpoint is **always advertised as available**. It is deliberately absent
from the `LibreOffice` dependency group in `ENDPOINT_GROUPS`, because the
built-in engine converts the common OOXML formats with no external tool — the
previous behavior (`DEPENDENCY`, "this tool is not available from your server")
was untrue on a machine without LibreOffice. The PDF → office direction has no
built-in engine and remains gated on the `LibreOffice` group.

`501 Not Implemented` is still returned per request when the uploaded format
needs LibreOffice and LibreOffice is not in use. A LibreOffice process that
starts but fails, or produces no PDF, returns a server error.

## Supply chain

The built-in engine is `office2pdf` 0.6.5, pinned in `rust/Cargo.toml` via
`[patch.crates-io]` to a commit on `hairbui76/office2pdf` rather than the
published crate. Published 0.6.5 derives the filename of an extracted embedded
font from strings copied verbatim out of the uploaded document (DOCX `w:name`,
PPTX `typeface`), and `Path::join` lets an absolute or `..`-bearing name choose
where that file is written; the pinned revision reduces those names to a
sanitised single path component. The pin is a commit SHA, never a branch.

`umya-spreadsheet` is likewise patched to the panic-safety branch `office2pdf`'s
own workspace uses, because a `[patch]` section only takes effect in the
top-level workspace.

## Verification

Unit tests cover extension validation, HTML sanitization, OOXML relationship removal,
ODF `href` removal, DTD rejection, traversal rejection, package payload preservation,
profile-URI building, the built-in engine's format gate, its row bound, and its
tolerance of an unopenable package. `tests/office_builtin_engine.rs` runs the real
worker binary end to end: it converts a real DOCX, XLSX, and PPTX and asserts each
produces a readable, non-empty PDF; it asserts a malformed package fails without
taking the caller down; it asserts the row bound trips before a worker is spawned
and that it still trips when the workbook relocates its worksheet part outside
`xl/worksheets/`; it asserts a deck with a `show="0"` slide is *not* reported as
degraded; it asserts a dropped slide *is* reported as degraded even when the
slide parts are not named `slideN.xml`; and it asserts a document naming its
embedded font with an absolute path cannot place a file there. Unit tests cover
relationship-target resolution (relative, package-absolute, `..`-climbing,
escaping, external), unreferenced parts, the content-type fallback, the
fail-closed paths (oversized `.rels`, extra nesting level, wholly opaque
package, undeterminable deck), and start-tag scanning against `quick-xml`'s
behaviour (spaced `show`, decoy `show` in another attribute, `>` inside a quoted
value, malformed and unquoted attributes, doctype internal subset, signed and
uppercase-`X` character references, non-`sld` root). Two end-to-end tests assert
the scanner against the engine's **real page count** in both directions: no
degraded verdict when the engine rendered everything it meant to, and a degraded
verdict whenever a slide was genuinely dropped. HTTP tests
assert unknown/unsafe input → `400` and real
text/HTML conversion when LibreOffice is present on the host (otherwise `501`).
`runtime_config` tests assert `file-to-pdf` stays enabled with the `LibreOffice`
group missing while `pdf-to-word` reports `DEPENDENCY`.
