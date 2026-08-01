# Get PDF Information Compatibility Contract

Route: `POST /api/v1/security/get-info-on-pdf`

## Request

The route accepts `multipart/form-data` with one PDF in `fileInput`. The upload
is bounded at 100 MiB. As in the Java controller, missing, empty, oversized, or
malformed input returns HTTP `200` with an `error.json` attachment containing an
`error` message and timestamp instead of an HTTP error status.

## Response

A valid request returns `application/json` as `response.json`. Rust preserves
the report's existing top-level sections:

- `Metadata`, including custom document-information keys;
- `BasicInfo`, `DocumentInfo`, `Compliancy`, `Encryption`, and `Permissions`;
- recursive `FormFields`;
- `Other`, including embedded files, annotation attachments, JavaScript,
  layers, bookmarks, XMP metadata, and the structure tree;
- `PerPageInfo`, including geometry, rotation, annotations, images, links,
  fonts, XObjects, and multimedia; and
- `SummaryData` when summary values exist.

PDF/A, PDF/UA, and WTPDF verification is attempted through the shared
`verify-pdf` implementation. Matching the Java endpoint, validator failures do
not fail the information request; structural and security information is still
returned.

## Resource bounds

- XMP stream decompression is limited to 16 MiB.
- Recursive form and structure traversal is limited to depth 256 and 100,000
  visited items, with cycle detection.
- Page image, font, and XObject reporting inspects direct page resources, which
  matches the Java controller rather than recursively expanding nested forms.

## Compatibility limits

- XMP is returned as decoded source XML without a normalize-and-reserialize
  pass.
- Embedded-file creation and modification dates retain their PDF date strings.
  Top-level document dates are normalized to the Java-compatible local date
  format.
- Full standards conformance details remain dependent on the optional veraPDF
  runtime described by `verify-pdf.md`, and the dependence is stronger than it
  looks. The report calls `verify_pdf(...).ok()`, and that call fails as a whole
  the moment a document declares *any* validation profile it cannot check, so
  without veraPDF the entire compliance block is skipped rather than partially
  filled. The practical effect: `IsPDF/ACompliant`, `IsPDF/UACompliant` and
  `PDF/AConformanceLevel` are absent or `false` for a declaring document on
  every configuration that does not ship veraPDF — which is all of them. A
  document that declares nothing still gets the `not-pdfa` key, so the presence
  of that key and the absence of any per-standard key are what distinguish
  "declares nothing" from "declares something we could not check".
- `IsPDF/ACompliant` means *declared and validated as conformant*, not merely
  *declares PDF/A*: the scan runs behind a `compliant` filter. There is no field
  that reports the declaration alone.
