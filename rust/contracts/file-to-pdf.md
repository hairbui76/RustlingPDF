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
| Whole PPTX slides silently dropped while the engine reports zero warnings | the parent counts `ppt/slides/slideN.xml` parts before conversion and PDF pages after; a shortfall sets `X-Rustling-Conversion-Degraded` and adds an explicit warning. |

Residual risks, stated plainly:

- A document that stays just under every bound can still occupy the single
  conversion slot for the full timeout. The worst case is a queue, not an outage.
- Silent data loss *inside* a rendered slide or paragraph (wrong glyphs, lost
  formatting, a dropped shape) is not detected; only whole missing PPTX pages
  are. The absence of `X-Rustling-Conversion-Degraded` does not prove fidelity.
- The row count matches `<row` textually rather than parsing the worksheet. It
  can overcount a file that embeds that string elsewhere; it is a bound, not a
  statistic.

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
taking the caller down; it asserts the row bound trips before a worker is spawned;
and it asserts a document naming its embedded font with an absolute path cannot
place a file there. HTTP tests assert unknown/unsafe input → `400` and real
text/HTML conversion when LibreOffice is present on the host (otherwise `501`).
`runtime_config` tests assert `file-to-pdf` stays enabled with the `LibreOffice`
group missing while `pdf-to-word` reports `DEPENDENCY`.
