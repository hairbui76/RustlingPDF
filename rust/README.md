# RustlingPDF Rust Workspace

The Rust workspace contains the complete processing runtime, optional AI
runtime, local automation CLI, and operation-catalog generator.

## Crates

- **`rustling-processing`** — the Axum HTTP service and in-process PDF
  processing pipeline. It provides document operations, configuration and
  UI-data endpoints, asynchronous jobs, temporary result handling, and static
  SPA serving.
- **`rustling-ai-engine`** — optional, stateless document understanding and
  orchestration. It supports classification, page-cited summaries, structured
  extraction, ordered translation, review/edit planning, document generation,
  and math auditing.
- **`rustling-cli`** — the `rustlingpdf` executable. It validates parameters
  against generated catalog schemas and runs local files through the processing
  pipeline without starting an HTTP server.
- **`rustling-operation-catalog`** — generates typed operation metadata from
  the committed OpenAPI snapshot.

The processing service has no accounts, database, or durable document store.
The optional AI engine is disabled by default.

## Quick start

From the repository root:

```bash
task rust:install
task backend:dev
```

The service listens on `127.0.0.1:8080` by default. Set `PORT` on Task commands,
or set `RUSTLING_PORT` when running the binary directly. Port `0` requests an
OS-assigned port.

Run the CLI without a server:

```bash
task rust:cli -- operations
task rust:cli -- run general-rotate-pdf \
  --input report.pdf \
  --output report-rotated.pdf \
  --param angle=90
```

The Tauri shell packages `rustling-processing` as a sidecar. It starts the
sidecar on an ephemeral loopback port and connects the open-source frontend to
that local runtime.

See [RUNNING_WITH_RUST.md](RUNNING_WITH_RUST.md) for configuration, Docker,
native dependencies, AI setup, and deployment details. Behavior contracts live
under [contracts](contracts).

## Validation

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`task rust:check` installs and binds the pinned PDFium runtime before running the
workspace gate.
