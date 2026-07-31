# Running RustlingPDF

RustlingPDF consists of the `rustling-processing` service, the shared React
frontend, the local `rustlingpdf` CLI, and an optional `rustling-ai-engine`
sidecar. The processing service is account-free and stateless: it has no user
database or durable server-side document storage.

## Prerequisites

| Requirement | Purpose |
|---|---|
| Stable Rust toolchain | Build the Rust workspace |
| [Task](https://taskfile.dev) | Run repository workflows |
| Node.js and npm | Build or run the web frontend |
| PDFium | Native PDF rendering and selected processing paths |

Install dependencies and the pinned PDFium build from the repository root:

```bash
task rust:install
task frontend:install
```

`task rust:install` writes PDFium below the ignored `rust/.pdfium/` directory
and verifies its checksum. A deployment may instead point
`RUSTLING_PDFIUM_LIBRARY_PATH` at a PDFium shared library or its directory.

## Quick start

Run the processing service and web UI together:

```bash
task dev
```

Or run each component separately:

```bash
task backend:dev
task frontend:dev
```

The defaults are:

- processing service: `http://127.0.0.1:8080`;
- Vite frontend: `http://127.0.0.1:5173`;
- optional AI engine: `http://127.0.0.1:5001`.

Check a running service:

```bash
curl http://127.0.0.1:8080/api/v1/info/status
curl http://127.0.0.1:8080/api/v1/config/app-config
```

Run the service directly:

```bash
cd rust
RUSTLING_PDFIUM_LIBRARY_PATH="$PWD/.pdfium/current" \
  cargo run -p rustling-processing --locked
```

The service binds to loopback by default. Set `RUSTLING_HOST=0.0.0.0` only
when it should accept network connections, and put public deployments behind
an authenticated reverse proxy.

## Local CLI

The CLI invokes the processing runtime in-process and does not need a server:

```bash
task rust:cli -- operations
task rust:cli -- describe general-rotate-pdf
task rust:cli -- run general-rotate-pdf \
  --input report.pdf \
  --output report-rotated.pdf \
  --param angle=90
```

Install the command for the current user:

```bash
cargo install --path rust/crates/rustling-cli --locked
```

See [`contracts/cli.md`](contracts/cli.md) for pipeline files, JSON parameters,
binary stdout, overwrite rules, dependency failures, and exit codes.

## Configuration

The processing service loads:

1. `configs/settings.yml`;
2. `configs/custom_settings.yml`, which overrides the first file;
3. supported environment variables.

Both files are resolved below `RUSTLING_BASE_PATH`, which defaults to the
working directory. The bundled template is
`crates/rustling-processing/resources/settings.yml.template`.

Common variables:

| Variable | Purpose |
|---|---|
| `RUSTLING_BASE_PATH` | Root containing `configs/` and optional `customFiles/` |
| `RUSTLING_HOST` | Bind address; defaults to loopback |
| `RUSTLING_PORT` | HTTP port; defaults to `8080`, and `0` asks the OS for a free port |
| `RUSTLING_FRONTEND_DIST` | Built frontend directory served by the Rust binary |
| `RUSTLING_PDFIUM_LIBRARY_PATH` | PDFium library file or directory |
| `RUSTLING_PROCESSING_MAX_UPLOAD_BYTES` | Multipart upload limit |
| `RUSTLING_PROCESSING_ENABLE_URL_TO_PDF` | Explicitly enable guarded URL-to-PDF requests |
| `RUSTLING_JOB_QUEUE_BASE_CAPACITY` | Base asynchronous job capacity |
| `RUSTLING_JOB_QUEUE_RESOURCE_BUDGET` | Resource budget for queued work |
| `RUSTLING_JOB_RESULT_EXPIRY_MINUTES` | Result retention in temporary storage |
| `AIENGINE_ENABLED` | Enable the AI proxy |
| `AIENGINE_URL` | AI engine base URL |
| `AIENGINE_TIMEOUTSECONDS` | Default AI request timeout |

Only `RUSTLING_*` is the product environment namespace. Unknown or retired
settings do not become runtime compatibility aliases.

### Security-sensitive configuration

- URL-to-PDF is disabled by default and applies DNS/IP and redirect checks when
  enabled.
- CORS defaults should be replaced with explicit origins for network
  deployments.
- Temporary workspaces and asynchronous results are bounded and swept.
- Legal-policy links are empty by default and appear only when configured.
- PDF signature trust, timestamping, and revocation behavior are configured
  under the document-security settings; they are unrelated to application
  accounts or product licensing.

## Optional external tools

RustlingPDF probes optional programs at startup. If one is unavailable, only
the endpoints that require it are reported unavailable through
`GET /api/v1/config/endpoints-availability`.

| Program | Features |
|---|---|
| LibreOffice (`soffice`) | Office document conversion |
| qpdf | Repair and selected compression paths |
| OCRmyPDF (`ocrmypdf`) | Preferred OCR workflow |
| Tesseract (`tesseract`) | OCR fallback and language data |
| WeasyPrint (`weasyprint`) | HTML, Markdown, email, and URL rendering |
| Poppler (`pdftohtml`) | PDF-to-HTML and supporting conversions |
| Calibre (`ebook-convert`) | eBook conversion |
| `unrar`, 7-Zip, and `rar` | Comic archive conversion |
| FFmpeg | Opt-in PDF-to-video conversion |
| veraPDF | Optional strict conformance validation |

Executables can be overridden with
`RUSTLING_PROCESSING_<TOOL>_COMMAND`, for example
`RUSTLING_PROCESSING_SOFFICE_COMMAND` or
`RUSTLING_PROCESSING_TESSERACT_COMMAND`.

## Optional AI engine

Start all local components:

```bash
task dev:all
```

Or run the engine alone:

```bash
task engine:dev
```

Then set `AIENGINE_ENABLED=true` and point `AIENGINE_URL` at the engine.
Provider credentials are supplied to the engine process. A local Ollama
endpoint can be used instead of a hosted provider. The AI surface is optional,
disabled by default, and keeps no document database.

## Docker

Build and run the self-hosted image:

```bash
task docker:build
task docker:up
```

Or use Compose directly:

```bash
docker compose -f docker/compose.yml up --build
```

The runtime image serves the SPA and API on port 8080. It includes PDFium and a
common conversion toolchain. Configuration may be mounted read-only below
`/data`; request scratch data remains temporary.

Run the optional AI profile:

```bash
task docker:up:ai
```

Set the provider credential in the container environment and change
`AIENGINE_ENABLED` to `true` for the processing container.

## Desktop

The Tauri application bundles the core frontend and the Rust processing
sidecar. Build prerequisites and native runtimes are staged automatically:

```bash
task desktop:dev
task desktop:test
task desktop:build
```

Desktop bundles include pinned PDFium, qpdf, Tesseract, English OCR data, and
their third-party notices. Platform-specific installer behavior is documented
under `contracts/desktop-*.md`.

## Validation

```bash
task rust:check
task frontend:check
task engine:check
task desktop:test
task check:all
```

When running Cargo directly on Linux in this workspace, bind PDFium explicitly:

```bash
cd rust
RUSTLING_PDFIUM_LIBRARY_PATH="$PWD/.pdfium/current" \
  cargo test --workspace --locked
```

Generated OpenAPI consumers, operation catalogs, frontend types, and dependency
notices must stay synchronized with their source files.
