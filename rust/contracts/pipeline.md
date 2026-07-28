# Pipeline contract

## Scope

`POST /api/v1/pipeline/handleData` runs a multipart document pipeline without
leaving the Rust process. It is the synchronous API counterpart of Java's
`PipelineProcessor`: each step is dispatched through the same Rust route
handlers and receives ordinary multipart fields.

Adding `?async=true` persists the exact multipart request and admits the whole pipeline through the
shared resource-weighted job queue. The normal pipeline response becomes the job
result and is available through the generic status/result/file endpoints.

## Request

The body is `multipart/form-data` and requires:

| Field | Shape | Meaning |
| --- | --- | --- |
| `fileInput` | one or more files | Initial pipeline inputs. |
| `json` | UTF-8 JSON text | Object containing a non-empty `pipeline` array. |

Each item in `pipeline` has an `operation` path and an optional `parameters`
object. String, boolean, number, and null values become normal form fields;
array values become repeated fields. `name`, `outputDir`, and `outputFileName`
are accepted as legacy configuration fields but do not affect this synchronous
HTTP endpoint, matching the Java controller.

The operation path is deliberately restricted to
`/api/v1/{general,misc,security,convert,filter}/...`, with only ASCII
alphanumeric, `_`, and `-` path segments. It cannot re-enter pipeline, config,
authentication, or AI orchestration routes.

## Execution and output

- SISO operations run once per current input file. Successful ZIP responses are
  safely unpacked before the next step, so a split can feed a later per-file
  operation.
- Confirmed all-`fileInput` multi-input operations (`general/merge-pdfs` and
  `convert/img/pdf`) run once over the current set. Operations that need a
  separately named companion file field (for example `overlayFiles`) are not a
  supported generic multi-input shape yet.
- A failed filter (`204 No Content`) drops that file from the rest of the
  pipeline. Other non-`200` step responses stop the run and preserve their HTTP
  status in the pipeline response.
- One final file is streamed as `application/octet-stream`; multiple or zero
  final files are streamed in `output.zip`. Duplicate entry names receive
  Java-compatible numeric suffixes.
- As in the Java processor, ordinary tool-generated filename suffixes are
  removed between steps so the original logical filename follows the document;
  `auto-rename` keeps its generated name.

Files and response bodies are streamed through a private temporary workspace.
ZIP extraction rejects traversal paths, more than 10,000 entries, and more than
128 GiB total expanded data.

## Watched folders (removed)

The server-side watched-folder daemon was removed with server-side state: the
binary no longer scans `pipeline/watchedFolders` or writes to
`pipeline/finishedFolders`, and the `autoPipeline.*` /
`system.customPaths.pipeline.watchedFoldersDir(s)` / `finishedFoldersDir`
settings are ignored. Folder automation is client-owned: the SPA watches the
user's real folders through the File System Access API and drives this same
HTTP endpoint. `system.customPaths.pipeline.pipelineDir` is still honoured for
locating the read-only pipeline web-UI config templates.

## Deliberate gaps

The generic dispatcher does not yet use the legacy runtime OpenAPI metadata to
pre-filter inputs or validate every operation parameter before execution; the
selected Rust handler remains the authoritative validator. Consequently an
unsupported input is restored after that handler rejects the job rather than
being filtered during directory collection.
