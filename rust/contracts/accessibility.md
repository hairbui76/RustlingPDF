# Accessibility Check And Remediation Contract

Routes:

- `POST /api/v1/accessibility/check`
- `POST /api/v1/accessibility/remediate`

This surface is a bounded native inspection and repair tool. It does not
certify PDF/UA conformance, create PDF/A output, or replace a standards
validator such as veraPDF.

The rules are based on the PDF/UA-1 Matterhorn Protocol checkpoints for tagged
content, natural language, graphics, and annotations, plus the W3C PDF
techniques for text alternatives, reading/tab order, document language, and
form-control names.

## Check request

The checker accepts `multipart/form-data` with one PDF in `fileInput`. Missing,
empty, oversized, encrypted, or malformed input returns the normal processing
API error response. Processing is stateless and uses the existing upload and
scratch-directory bounds.

## Check response

The response is JSON with `schemaVersion: 1`:

- `summary` contains `passed`, `failed`, `manualReview`, `total`, and
  `remediable` finding counts.
- `document` contains page count, catalog language, structure-tree presence,
  the `/MarkInfo /Marked` state, figure count, form-field count, and an ordered
  structure preview. Preview rows identify the indirect structure object,
  effective role, optional page, and current alternative text.
- `findings` contains a stable `ruleId`, `status` (`pass`, `fail`, or
  `manual`), `severity`, scope, actionable message, remediation class
  (`automatic`, `userInput`, or `manual`), and applicable page/object/field
  target data.

The native checker reports:

1. whether the catalog has a structure tree and marks the document as tagged;
2. an ordered structure preview that a person must use to confirm semantic
   reading order;
3. whether pages containing visible annotations use structure tab order
   (`/Tabs /S`);
4. whether the catalog has a nonblank, language-tag-shaped `/Lang`;
5. whether every effective `Figure` structure element has nonblank `/Alt` or
   `/ActualText`; and
6. whether every terminal AcroForm field has a nonblank `/TU` accessible name.

The checker resolves indirect objects, inherited field names, and structure
role mappings, with cycle, depth, and item-count bounds. A pass means only that
the named native rule passed. It is never presented as PDF/UA certification.
Logical reading order and the quality of language, alternative text, and form
labels require human review.

## Remediation request

The remediation route accepts:

- the original PDF in `fileInput`; and
- one JSON object in `repairs`.

The object may contain:

- `documentLanguage`: a nonblank language tag using an ASCII primary language
  subtag followed by optional hyphen-separated alphanumeric subtags;
- `markAsTagged: true`, allowed only when a structure tree already exists;
- `structureTabOrderPages`: zero-based page indexes whose `/Tabs` value will be
  set to `/S`;
- `alternativeTexts`: entries containing `objectNumber`, `generation`, and
  nonblank `text` for an existing `Figure` structure object; and
- `formFieldTooltips`: entries containing an exact fully qualified
  `fieldName` and nonblank `text` for an existing terminal field.

Every requested target and value is validated before mutation. Duplicate,
missing, mismatched, unsupported, or out-of-range targets reject the whole
request, and no partial PDF is returned. Unknown structure, content streams,
page content, and field values are preserved.

The response is the remediated PDF. The editor re-runs the checker on that
result to provide explicit before/after proof.

## Deliberate repair limits

- The tool does not synthesize a structure tree for an untagged PDF.
- It does not automatically reorder structure elements or infer semantic
  reading order.
- It does not invent image descriptions or accessible form labels.
- It does not add PDF/UA identification metadata or claim conformance.
- It does not create or convert PDF/A.

Those operations require author judgment or a full tagging workflow; silently
guessing them could make accessibility worse.
