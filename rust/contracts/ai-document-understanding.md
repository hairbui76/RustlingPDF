# Stateless AI document-understanding contract

RustlingPDF exposes three dedicated document-understanding operations through
the optional AI engine:

- `POST /api/v1/ai/tools/document-summary`
- `POST /api/v1/ai/tools/document-extraction`
- `POST /api/v1/ai/tools/document-translation`

The processing service accepts the PDF, extracts bounded page text locally, and
sends only that text plus the requested operation settings to the separately
configured AI engine. PDF bytes are never sent to the model provider. Each
operation is a single request: neither service stores the PDF, extracted text,
prompt, or result after the response completes. The source PDF is never
modified.

The routes are disabled unless `AIENGINE_ENABLED` (or
`RUSTLING_AI_ENGINE_ENABLED`) is true. They use the existing Anthropic,
OpenAI-compatible, or local Ollama provider selected for
`rustling-ai-engine`; no account, RustlingPDF license key, or server-side
document identity is introduced.

## Shared multipart and bounds

Every route requires one non-empty PDF part named `fileInput`. The processing
service enforces the normal server upload limit and a 50 MiB tool-local limit.
It extracts at most the configured `aiEngine.limits.maxPages` pages and
`maxCharacters` UTF-16 code units (defaults: 200 and 200,000), with at most
4,000 units from any one page. A PDF with no extractable text returns `422`;
the user can run OCR first for an image-only scan.

The processor calls these authenticated engine routes with typed JSON:

- `POST /api/v1/ai/document/summary`
- `POST /api/v1/ai/document/extraction`
- `POST /api/v1/ai/document/translation`

Each request carries `fileName` and one-based `{pageNumber,text}` page objects.
The engine independently rejects duplicate/zero page numbers, blank page text,
or content above its own active page/character limits. Engine response bodies
are bounded to 2 MiB at the processor boundary.

Successful public responses use:

```json
{
  "schemaVersion": 1,
  "operation": "summary",
  "providerDisclosure": "Extracted document text was sent to the AI provider configured by this server.",
  "source": {
    "fileName": "example.pdf",
    "pagesProcessed": 3,
    "charactersProcessed": 8421,
    "maxPages": 200,
    "maxCharacters": 200000
  },
  "result": {}
}
```

`providerDisclosure` is deliberately explicit even for local Ollama: the
processor cannot reliably infer whether an operator-configured compatible
endpoint is on the same machine. The UI shows the disclosure before execution,
not only after a response.

## Summary

Optional text fields:

- `detail`: `brief`, `standard` (default), or `detailed`;
- `instructions`: additional focus, at most 4,000 UTF-16 code units.

The result contains `summary` and ordered `keyPoints`. Each key point has
`text` and a non-empty `pages` array. Model-supplied page references not present
in the request are removed; a point left without a valid page is removed. The
engine therefore never presents an ungrounded page number as a source.

## Structured extraction

The `fields` part is a JSON array with 1–50 entries:

```json
[
  {
    "key": "invoice_number",
    "description": "Invoice identifier",
    "valueType": "string",
    "required": true
  }
]
```

`key` is a unique 1–64 character identifier containing ASCII letters, digits,
`_`, `-`, or `.`. `description` is 1–500 UTF-16 code units. `valueType` is one
of `string`, `number`, `integer`, `boolean`, `date`, or `list`.

The result contains one value for every caller field, in caller order. A value
has `key`, `value`, `pages`, `confidence` (`high`, `medium`, or `low`), and an
optional `note`. Unknown model keys are discarded, duplicate keys use the first
valid model result, and page references are filtered to supplied pages. Missing
or invalid model values become `null`, `pages: []`, `confidence: low`, with a
diagnostic note. RustlingPDF does not claim model confidence is a calibrated
probability.

## Translation

`targetLanguage` is required and `sourceLanguage` is optional; each is a
1–100-character human-readable language name or BCP 47 tag.

Translation preserves source page boundaries and extracted block order, not
the original visual layout. The engine deterministically divides extracted
page text into ordered blocks and assigns opaque IDs before invoking the model.
It groups those blocks into bounded provider completions using the configured
chunk-size and concurrency limits, filters each completion to the block IDs
that completion received, and then reconstructs the response in source
page/block order. This keeps a large accepted request from depending on one
provider output window:

```json
{
  "targetLanguage": "Vietnamese",
  "pages": [{
    "pageNumber": 1,
    "blocks": [{
      "blockId": "p1-b1",
      "sourceText": "Hello",
      "translatedText": "Xin chào"
    }]
  }]
}
```

Unknown and duplicate block IDs are discarded. A source block omitted by the
model remains present with an empty `translatedText`, making partial output
visible instead of silently reordering or deleting source content.

This endpoint does **not** return a rewritten PDF and does not promise
pixel-perfect, font-preserving, or line-breaking-equivalent output. Producing a
translated PDF would require explicit font substitution, overflow, geometry,
and human-review policy that this programme has not authorized.

## Errors and privacy

- invalid multipart fields or operation settings: `400`;
- no extractable PDF text or an invalid engine request: `422`;
- optional engine disabled/unavailable or local PDFium unavailable: `503`;
- provider failure or invalid provider output: `502`;
- processor-to-engine timeout: `504`;
- internal temporary-file or serialization failure: `500`.

Temporary upload directories are request-owned and deleted on every success or
error path. There is no document store, vector database, retrieval index,
conversation history, analytics payload, or background retry for these
operations.
