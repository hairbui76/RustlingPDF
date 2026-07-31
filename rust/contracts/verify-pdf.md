# Verify PDF Compatibility Contract

Route: `POST /api/v1/security/verify-pdf`

## Request

The route accepts `multipart/form-data` with one PDF in `fileInput`. Missing,
empty, or malformed PDF input returns `400`.

## Profile detection

- Rust reads the catalog XMP metadata stream with a 16 MiB decompression bound.
- PDF/A 1–4, PDF/UA 1–2, and WTPDF 1.0 declarations are detected by namespace
  URI, so documents may use prefixes other than `pdfaid`, `pdfuaid`, and
  `pdfwtid`.
- A missing or incomplete PDF/A identification declaration produces the
  Java-compatible `not-pdfa` result. This path is fully native and does not
  require an external validator.
- Every declared profile is validated independently, allowing PDF/A and
  PDF/UA/WTPDF results to coexist in the response.

## Standards validation

Full conformance validation uses the veraPDF command line interface, because
syntax parsing alone cannot prove compliance with PDF/A, PDF/UA, or WTPDF.
Rust invokes veraPDF directly with XML output, no shell interpolation, the
declared profile, logging disabled, and unlimited displayed failures.

The executable is resolved in this order:

1. `RUSTLING_PROCESSING_VERAPDF_COMMAND`, when set;
2. `verapdf`; and
3. `verapdf.bat`.

An unavailable auto-discovered runtime returns `501`. A configured command
that cannot start, fails, or returns an invalid report returns `500`.

This is an input-conditional dependency, not a route-wide one. Startup
discovery still reports `group-enabled?group=veraPDF` as `false` when the tool
is missing, but `endpoints-availability` keeps `verify-pdf` enabled because a
PDF with no declared validation profile completes through the native
`not-pdfa` path. A PDF that declares PDF/A, PDF/UA, or WTPDF then receives the
documented request-time `501` if veraPDF is absent. The route-level
availability shape has no conditional-capability state, so disabling the whole
route would be less truthful than preserving the working native path.

## Response

The route returns `application/json` containing the existing
`PDFVerificationResult[]` wire shape:

- `standard`, `standardName`, `validationProfile`, and
  `validationProfileName`;
- `complianceSummary`, `declaredPdfa`, and `compliant`;
- `totalFailures`, `totalWarnings`, `failures`, and `warnings`; and
- issue-level `ruleId`, `message`, `location`, `specification`, `clause`, and
  `testNumber`.

Failed veraPDF checks are expanded into issue entries with their rule metadata
and object context. veraPDF currently reports conformance failures rather than
warnings for these profiles, so the warnings list is empty.

## Compatibility limits

- The standards rules remain supplied by the veraPDF runtime; replacing that
  independently maintained validator is not part of the HTTP service rewrite.
- External process cancellation and hard timeouts remain part of the shared
  job-runtime migration slice.
