# Local CLI contract

## Scope

The `rustlingpdf` binary runs catalog-backed RustlingPDF operations against
local files. It assembles the existing processing runtime and pipeline router
inside the CLI process: it does not start a TCP listener, require an account,
upload to a RustlingPDF service, or create durable server-side state.

The operation bindings are generated during the Rust build from
`rustling-ai-engine/src/operation_catalog.json`. That keeps operation paths and
parameter JSON Schemas aligned with the HTTP pipeline and AI tool boundary.
Regenerating the existing catalog therefore changes all three consumers in one
reviewable source update.

## Build and discovery

From the repository root:

```shell
cargo build --manifest-path rust/Cargo.toml --locked -p rustling-cli
cargo install --path rust/crates/rustling-cli --locked

rustlingpdf operations
rustlingpdf operations --json
rustlingpdf describe general-rotate-pdf
rustlingpdf describe /api/v1/general/rotate-pdf --json
```

Every generated operation has:

- a canonical HTTP path such as `/api/v1/general/rotate-pdf`; and
- a CLI ID formed from the path below `/api/v1/`, with `/` changed to `-`,
  such as `general-rotate-pdf`.

Both spellings are accepted by `run` and in pipeline specifications.
`operations --json` emits the ID, canonical path, title, and full parameter
schema for automation and completion tooling.

## Execute one operation

```shell
rustlingpdf run general-rotate-pdf \
  --input report.pdf \
  --output report-rotated.pdf \
  --param angle=90
```

`--input` is repeatable for the catalog operations whose HTTP handlers accept
multiple `fileInput` parts. `--param key=value` is repeatable. Values that are
valid JSON become their JSON type (`true`, `90`, `null`, arrays, or objects);
other values remain strings. Repeating a key produces an array. A base object
may instead be supplied inline or from a file:

```shell
rustlingpdf run convert-pdf-img \
  -i report.pdf -o images.zip \
  --params-json '{"imageFormat":"png","dpi":180}'

rustlingpdf run convert-pdf-img \
  -i report.pdf -o images.zip \
  --params-json @image-options.json
```

Explicit `--param` values replace the same key from `--params-json`; repeating
that flag key then builds an array. The assembled object is validated against
the generated catalog schema before any file processing starts.

## Execute a pipeline

`pipeline` accepts the same non-empty `{"pipeline":[...]}` shape as
`POST /api/v1/pipeline/handleData`:

```json
{
  "pipeline": [
    {
      "operation": "general-rotate-pdf",
      "parameters": { "angle": 90 }
    },
    {
      "operation": "/api/v1/misc/compress-pdf",
      "parameters": { "optimizeLevel": 2 }
    }
  ]
}
```

```shell
rustlingpdf pipeline \
  --spec pipeline.json \
  --input report.pdf \
  --output report-ready.pdf
```

The CLI resolves IDs to canonical paths and validates every step before it
invokes the in-process router. Execution then retains the HTTP pipeline's SISO,
multi-input, ZIP fan-out, filter, filename, upload-limit, and optional native
dependency behavior. Supporting `fileParameters` are rejected by this first
CLI contract because the local command has no unambiguous asset-key input
syntax yet.

## Output and overwrite policy

An explicit `--output` is required. Document bytes are written to that file;
status and errors are written to stderr. An existing output is preserved unless
`--force` is passed. File output is first written and synced in the destination
directory, then atomically persisted.

`--output -` is the only binary-stdout mode. `operations --json` and
`describe --json` are metadata-stdout modes. This separation keeps pipes safe:

```shell
rustlingpdf run general-rotate-pdf \
  -i report.pdf -o - -p angle=90 > report-rotated.pdf
```

## Exit codes

| Code | Meaning |
|---:|---|
| `0` | Success |
| `2` | CLI usage, catalog schema, or pipeline-definition error |
| `3` | Local input/output I/O error |
| `4` | The processing handler rejected the request, or configuration disabled it |
| `5` | A required optional runtime dependency is unavailable |
| `6` | Internal runtime, response-stream, or compiled-catalog failure |

The CLI checks availability on the exact runtime instance before dispatch so a
missing LibreOffice, WeasyPrint, Calibre, OCR toolchain, FFmpeg, or other
operation-required optional dependency is distinguishable from an invalid
document or administrator-disabled operation.

## Deliberate boundaries

- This surface contains the operations selected by the existing generated
  catalog. Interactive, introspection, secondary-upload, certificate-signing,
  form-design, accessibility-remediation, and AI-orchestration endpoints that
  the catalog intentionally excludes are not silently exposed as untyped
  commands.
- There is no remote-server mode, account/token support, watched-folder daemon,
  or persistent queue in this CLI.
- PDF/A conversion or remediation is not part of this programme.
