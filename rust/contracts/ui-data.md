# UI data contract

These read-only endpoints provide backend metadata consumed by the client.

## Routes

| Route | Rust response |
| --- | --- |
| `GET /api/v1/ui-data/footer-info` | Analytics choice and legal links from `settings.yml` / `custom_settings.yml`. |
| `GET /api/v1/ui-data/home` | `showSurveyFromDocker`, controlled by `SHOW_SURVEY` (unset is `true`). |
| `GET /api/v1/ui-data/licenses` | `{ "dependencies": [...] }` generated from the locked Rust dependency graph at build time. |
| `GET /api/v1/ui-data/pipeline` | Recursive JSON templates from `pipeline/defaultWebUIConfigs`, or the Java placeholder when absent. |
| `GET /api/v1/ui-data/ocr-pdf` | Sorted Tesseract `.traineddata` language names, excluding `osd`. |
| `GET /api/v1/ui-data/sign` | Shared signature-image metadata plus installed packaged/custom font metadata. |

The pipeline template directory follows this settings precedence:
`system.customPaths.pipeline.pipelineDir`, then
`system.customPaths.pipeline.webUIConfigsDir`, then its installation default.
Tessdata follows `system.tessdataDir`, `SYSTEM_TESSDATADIR`,
`TESSDATA_PREFIX`, then Java's Linux default path.

## Behavior notes

OCR language packs are provisioned by the operator and are never downloaded
at runtime. Saved signature assets are owned by the browser; the `sign` route
may list operator-provisioned assets under
`customFiles/signatures/ALL_USERS`.

## Verification

`tests/ui_data_endpoints.rs` exercises the route tree above.
