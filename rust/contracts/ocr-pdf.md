# `POST /api/v1/misc/ocr-pdf`

Current contract for OCR processing.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- `languages`: repeated field, at least one required (e.g. `eng`, `deu`)
- `ocrRenderType`: `hocr` (default) or `sandwich`
- `ocrType`: `skip-text` | `force-ocr` | `Normal` (anything non-empty other than
  `force-ocr` maps to `--skip-text`)
- `sidecar`, `deskew`, `clean`, `cleanFinal`, `removeImagesAfter`: optional booleans
- Success returns `<base>_OCR.pdf` (`application/pdf`), or — when `sidecar` is set —
  `<base>_OCR.zip` (`application/octet-stream`) containing the OCR'd PDF and its
  extracted text.

## Behavior

OCRmyPDF is the preferred engine:

```
ocrmypdf --verbose 2 --output-type pdf --pdf-renderer <hocr|sandwich> \
         [--sidecar <txt>] [--deskew] [--clean] [--clean-final] \
         [--force-ocr | --skip-text] --invalidate-digital-signatures \
         --language <l1+l2+…> <input> <output>
```

The `ocrmypdf` binary is resolved from `RUSTLING_PROCESSING_OCRMYPDF_COMMAND` when
set, otherwise platform defaults. The known restricted-kernel multiprocessing
failure is retried once with `--jobs 1`. When `removeImagesAfter` is set the OCR'd
PDF is post-processed in Rust: every image `XObject` resource, the `Do` operators
that paint them, and every inline `BI`/`ID`/`EI` image are removed with `lopdf`,
and the orphaned image streams are then pruned so their bytes leave the file. The
OCR text layer, fonts, and vector content are untouched. This replaces the Java
behavior's Ghostscript `-dFILTERIMAGE` pass; no external tool is involved, so the
flag can no longer fail for lack of one.

When OCRmyPDF is disabled or cannot be found, Rust uses Java's Tesseract fallback.
PDFium loads the source once, detects text for `skip-text`, retains every original
page as a one-page PDF, and renders selected pages to bounded PNGs at
`system.maxDPI` (default 500). Each selected page runs:

```
tesseract <page.png> <zero-based-output-base> -l <l1+l2+…> pdf
```

The generated and retained page PDFs are merged in source order. Exit zero without
the expected generated PDF retains the source page. `force-ocr` and all values
other than `skip-text` OCR every page. Matching Java, fallback mode ignores the
OCRmyPDF-only cleanup flags and creates an empty text member when `sidecar=true`.
`RUSTLING_PROCESSING_TESSERACT_COMMAND` can select an explicit executable.

Both command paths use shared process pools with the same Java configuration
surface. `processExecutor.sessionLimit.ocrMyPdfSessionLimit` defaults to 2 and
`tesseractSessionLimit` defaults to 1; both matching timeout values under
`processExecutor.timeoutMinutes` default to 30 minutes. Timeout terminates the
command and its discovered descendants before releasing the pool slot. The
equivalent Spring relaxed-binding environment names are also honored.

Before starting OCRmyPDF, Rust discovers the immediate `*.traineddata` entries in
the configured tessdata directory (`system.tessdataDir`, then `TESSDATA_PREFIX`,
then the packaged default), excluding `osd` case-insensitively. Requested
languages are matched case-sensitively and unavailable values are discarded while
request order and duplicates are preserved. Empty `languages`, an invalid
`ocrRenderType`, or no remaining installed language each return `400` in the same
validation order as Java.

## Optional PaddleOCR engine

`ocr.engine` selects the engine and defaults to `auto`, which is the
OCRmyPDF/Tesseract behavior described above and is unchanged. Setting it to
`paddle` selects the in-process `PaddleOCR-Rust` port instead. Any other value
is a configuration error.

Paddle is compiled behind the `paddle-ocr` Cargo feature and is not part of any
shipped artifact. The desktop sidecar and both Docker binaries are built as
`-p rustling-processing` without it, so they answer a `paddle` selection with
"this RustlingPDF build does not include the paddle-ocr feature". Only a build
that enables the feature can run this engine.

Paddle is opt-in and entirely operator-provisioned. Five local paths are
required together; a partially configured engine is rejected and never falls
back to another engine:

- `ocr.paddle.onnxRuntimePath`
- `ocr.paddle.detectorModelPath`
- `ocr.paddle.recognizerModelPath`
- `ocr.paddle.dictionaryPath`
- `ocr.paddle.textLayerFontPath`

No model, dictionary, ONNX Runtime library, or text-layer font is bundled in
any installer or downloaded at runtime. `rust/contracts/runtime-config.md` owns
the key and environment-variable spellings.

RustlingPDF supports exactly one artifact pair. The detector and recognizer are
checked against pinned SHA-256 digests, and the dictionary must match both a
pinned digest and an entry count of 18,708. The dictionary is read under a
4 MiB bound and the text-layer font under a 32 MiB bound. A mismatch fails the
request; it does not silently recognize with different artifacts.

One engine is loaded lazily on the first Paddle request and reused for the
process. Inference is serialized behind a mutex because the port's engine type
is not safely shared across threads.

### Page construction

Paddle returns text and quadrilaterals, so RustlingPDF builds the searchable
layer itself. PDFium loads the source once and selects pages with the same rule
as the Tesseract fallback: `skip-text` retains pages that already carry text,
and every other `ocrType` value OCRs every page. Selected pages are rendered to
bounded images at `system.maxDPI` and rebuilt as a new page whose background is
that raster and whose text objects use the operator's font at PDF text render
mode 3 (invisible), positioned from the recognized quadrilaterals. Pixel
coordinates use a top-left origin and PDF points a bottom-left origin, so the
vertical axis is flipped during placement. Retained pages keep their original
content unchanged, and all pages are merged in source order.

An OCR-selected page therefore becomes a raster page. This matches the existing
Tesseract fallback shape and is the reason `removeImagesAfter` is ignored on
this path: the raster is the page.

### Request fields on the Paddle path

The request contract is unchanged and still fully validated. Empty `languages`
and an invalid `ocrRenderType` still return `400` in the same order.

Two of those fields are then advisory rather than effective:

- `languages` is structurally required but selects nothing. Paddle uses the one
  pinned model and dictionary pair and never reads tessdata, so an empty
  tessdata directory does not block a Paddle request and the installed-language
  check is not applied.
- `ocrRenderType` is validated but has no Paddle-specific effect; there is no
  hOCR or sandwich renderer on this path.

`deskew`, `clean`, `cleanFinal`, and `removeImagesAfter` have no Paddle
implementation and no effect, matching how the Tesseract fallback ignores the
OCRmyPDF-only cleanup switches.

`sidecar=true` still returns `<base>_OCR.zip`. Its text member contains the
recognized lines of the pages Paddle actually processed, in page and line
order. Under `skip-text` the pages that were retained contribute nothing, so
the sidecar is a record of what was recognized, not a full-document text
extract.

### Failure behavior

A Paddle failure is reported as a server error and never falls back to
OCRmyPDF or Tesseract. An invalid or incomplete configuration is reported
before any rendering work begins. A build compiled without the `paddle-ocr`
feature reports the missing feature rather than recognizing nothing.

## Availability

Startup discovery probes OCRmyPDF and Tesseract independently, and the endpoint is
advertised when either tool remains enabled. If neither executable is available,
the endpoint returns `501 Not Implemented`.

Any explicitly selected `ocr.engine` — a valid `paddle` as well as an invalid
value — advertises the endpoint on its own, independently of OCRmyPDF and
Tesseract. Paddle does not use either program, and an invalid selection has to
surface as its own configuration error; reporting `501 Not Implemented` on a
host without OCRmyPDF would blame a missing program for the operator's typo. A process that starts but fails returns
a server error. `removeImagesAfter` has no external dependency; a malformed PDF
whose content streams cannot be decoded or re-encoded returns a server error.

## Verification

Unit tests cover tessdata discovery, empty-language and invalid-render-type
rejections, exact untrimmed multipart strings, case-sensitive availability
filtering, and preservation of selected language order and duplicates. A fake
Tesseract runner exercises bounded rendering, exact arguments, generated-page
selection, and ordered PDF reassembly without a host dependency. Process-executor
tests verify pool serialization and timeout cleanup of a spawned descendant. HTTP
tests assert all validation `400`s and follow the combined OCRmyPDF/Tesseract
availability contract.

Paddle adds unit tests for engine selection and the five required paths, for an
invalid engine value staying observable as a configuration error while keeping
the endpoint advertised, and for the service reporting an unusable
configuration identically with and without the `paddle-ocr` feature. Selection
tests prove that `auto` keeps the tessdata gate, that explicit Paddle does not
require tessdata, that request-shape validation is retained, that page
selection uses the Tesseract fallback rule, and that an invalid configuration
never falls back. A PDFium integration test builds a raster page with one
recognized line using the repository DejaVu font, reopens it, and asserts both
that the text extracts and that its render mode is invisible.

Model-backed recognition is not covered. It needs the operator's ONNX Runtime,
models, and dictionary, which are deliberately absent from the repository and
from CI.
