# Info and runtime metrics

## Routes

- `GET /api/v1/info/status` and `GET /api/v1/info/health` return
  `{ "status": "UP", "version": "<application version>" }`.
- `GET /api/v1/info/load[?endpoint=<path>]` and `/load/unique` return total or
  unique-session counts for GET requests.
- `GET /api/v1/info/load/all` and `/load/all/unique` return ordered endpoint
  count entries for GET requests.
- `GET /api/v1/info/requests[?endpoint=<path>]`, `/requests/unique`,
  `/requests/all`, and `/requests/all/unique` are the POST equivalents.
- `GET /api/v1/info/uptime` returns a `0d 0h 0m 0s` duration.
- `GET /api/v1/info/wau` returns `weeklyActiveUsers`,
  `totalUniqueBrowsers`, `daysOnline`, and ISO-8601 `trackingSince`.

The canonical application version comes from `rust/VERSION`. The build script
stages it into the compiled service and the status route reports that value.

## Collection

The process-local collector counts eligible requests before dispatch, excludes
static and `/api/v1/info/*` paths, and groups by method, path, and session.
Missing `JSESSIONID` cookies use the `no-session` bucket. Non-empty
`X-Browser-Id` values are retained for seven days for weekly-active statistics.

`metrics.enabled` or `METRICS_ENABLED` defaults to true. When disabled, metric
query routes other than status and health return
`403 This endpoint is disabled.`.

Counters are process-local, leave the service only when a caller requests the
info routes, and reset on restart. They are not external analytics.

Unit and HTTP tests cover version reporting, filters, uniqueness, browser
identifiers, request counts, weekly-active output, and disabled metrics.
