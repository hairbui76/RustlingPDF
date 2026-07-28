# UI data compatibility contract

These read-only endpoints are backend metadata consumed by the unchanged
client. They do not implement or modify the user interface.

## Routes

| Route | Rust response |
| --- | --- |
| `GET /api/v1/ui-data/footer-info` | Analytics choice and legal links from `settings.yml` / `custom_settings.yml`. |
| `GET /api/v1/ui-data/home` | `showSurveyFromDocker`, controlled by `SHOW_SURVEY` (unset is `true`). |
| `GET /api/v1/ui-data/licenses` | `{ "dependencies": [...] }` generated from the locked Rust dependency graph at build time. |
| `GET /api/v1/ui-data/pipeline` | Recursive JSON templates from `pipeline/defaultWebUIConfigs`, or the Java placeholder when absent. |
| `GET /api/v1/ui-data/ocr-pdf` | Sorted Tesseract `.traineddata` language names, excluding `osd`. |
| `GET /api/v1/ui-data/sign` | Shared signature-image metadata plus installed packaged/custom font metadata. |

The pipeline template directory follows Java's settings precedence:
`system.customPaths.pipeline.pipelineDir`, then
`system.customPaths.pipeline.webUIConfigsDir`, then its installation default.
Tessdata follows `system.tessdataDir`, `SYSTEM_TESSDATADIR`,
`TESSDATA_PREFIX`, then Java's Linux default path.

## Removed surfaces

The server is stateless and has no user accounts, so the following historic
surfaces no longer exist (requests fall through to `404`):

- `GET/POST /api/v1/ui-data/tessdata-languages` and `.../tessdata/download` —
  OCR language packs are provisioned by the operator (baked into the image or
  mounted read-only), never downloaded at runtime.
- `GET /api/v1/general/signatures/{filename}` and the whole
  `/api/v1/proprietary/signatures` family — signature assets are owned by the
  client (browser storage). The `sign` UI-data route still lists any
  operator-provisioned files under `customFiles/signatures/ALL_USERS` for
  display purposes.
- Every `/api/v1/proprietary/ui-data/*` portal projection (login, account,
  audit dashboards, teams, admin settings, API keys, documents,
  infrastructure) — these were projections over the removed accounts/audit
  stores.

## Behavior notes

The `sign` route lists only shared assets under
`customFiles/signatures/ALL_USERS`, exactly the subset Java exposes when its
current username is empty.

Every route is served by the single open router; there is no secured variant.

## Verification

`tests/ui_data_endpoints.rs` exercises the tree above, including the proof
that the removed signature-serving route no longer resolves.
