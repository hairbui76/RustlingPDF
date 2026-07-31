# Form field creation and batch fill

RustlingPDF extends its existing AcroForm inspection and mutation surface with:

- `POST /api/v1/form/create-fields`
- `POST /api/v1/form/batch-fill`

Both operations are stateless. They write outputs in the request scratch
directory, return them immediately, and never retain the uploaded document or
row data.

## Create fields

`create-fields` consumes `multipart/form-data` with:

- `file`: the required source PDF;
- `fields`: a required JSON array of logical field definitions.

Each definition has a required `name`, `type`, and nonempty `widgets` array.
The supported type strings are `text`, `checkbox`, `radio`, `combobox`,
`listbox`, `button`, and `signature`. Optional properties are `label`,
`tooltip`, `required`, `readOnly`, `multiline`, `multiSelect`, `options`,
`defaultValue`, `fontSize`, and `tabOrder`. Each widget contains `pageIndex`,
`x`, `y`, `width`, `height`, and an optional `exportValue`.

Coordinates use the exact inspection/viewer contract: a zero-based page index
and PDF points relative to the page CropBox with an upper-left origin. Width
and height must be positive and the complete rectangle must be inside the
CropBox. Rotation is not applied because the viewer rotates the page
container.

Text supports `multiline`; list boxes support `multiSelect`. Choices require
at least one nonblank option. A radio field requires one option/state per
widget unless every widget supplies its own nonblank `exportValue`. Checkbox
uses its first option or `Yes` as the on-state. Push buttons are created
without an action. Signature fields are empty signature widgets.

Names are trimmed and made unique against existing and preceding new fields
with the mutation surface's `_1`, `_2`, ... suffix convention. Fields are
editable and optional unless requested otherwise. Font size defaults to 12
points. `label` is stored as the mapping/display name; `tooltip`, or the label
when no tooltip is supplied, becomes the alternate name used by assistive
technology. Button labels also become their visible caption.

New widgets receive printable annotation flags and static appearances.
Text/choice values use Helvetica/WinAnsi appearances under the same documented
non-WinAnsi limitation as form fill. Checkbox/radio widgets receive explicit
off/on appearances. Explicit `tabOrder` values order new widgets in each
page's annotation array; existing annotations retain their relative order and
come first. Pages containing ordered new widgets declare annotation-order
tabbing with `/Tabs /A`.

The operation is atomic: any invalid definition, page, rectangle, type,
option, or default rejects the request with HTTP 400 and no output. Success is
an `application/pdf` attachment named `<base>_fields.pdf`.

## Batch fill

`batch-fill` consumes:

- `file`: the required PDF form template;
- `dataFile`: a required CSV or XLSX workbook.

The first row is the header. Blank and duplicate headers are rejected. Every
subsequent nonblank row maps header names to form values and produces one
filled PDF using the existing strict fill semantics. Unknown field columns are
ignored. `_filename` is reserved for an optional output base name; unsafe path
characters are normalized and collisions receive numeric suffixes. Without
it, outputs are named `row-001.pdf`, `row-002.pdf`, and so on.

CSV follows RFC 4180 quoting. XLSX reads the first worksheet and supports
shared strings, inline strings, booleans, formula cached strings/numbers, and
ordinary numeric cells. Display formatting and formula calculation are not
performed; cached cell values are used.

Success is an `application/zip` attachment named `<base>_batch_filled.zip`.
The source PDF and data file are unchanged. Existing global multipart/body
limits are the resource boundary; this route adds no account, tenant, or
server-side persistence quota.

## Verification

Focused tests must prove all seven field types, multiline/multi-select flags,
required/read-only flags, labels/tooltips, defaults, multi-widget radio
states, CropBox coordinate round trips, collision naming, tab order,
reopenable appearances, atomic validation errors, CSV quoting, XLSX shared and
inline strings, safe output names, and per-row form-value round trips.
