# Job statistics and cleanup

The three open runtime-observation routes use the shared `JobManager` and
`JobQueue`. See `job-management.md` for asynchronous job lifecycle behavior.

## Routes

| Method | Path | Response |
| --- | --- | --- |
| `GET` | `/api/v1/jobs/stats` | Camel-case `JobStats`: `totalJobs`, `activeJobs`, `completedJobs`, `failedJobs`, `successfulJobs`, `fileResultJobs`, `oldestActiveJobTime`, `newestActiveJobTime`, `averageProcessingTimeMs`. |
| `GET` | `/api/v1/jobs/queue/stats` | Queue counters: `queuedJobs`, `queueCapacity`, `runningJobs`, `resourceBudget`, `availableResourceUnits`, `totalQueuedJobs`, `rejectedJobs`, `resourceStatus` (`"BOUNDED"`). |
| `POST` | `/api/v1/jobs/cleanup` | `{ "message": "Cleanup complete", "removedJobs": n, "remainingJobs": n }` after expiring completed jobs past their retention window. |

## Access

The product has no authentication or roles. These routes are part of the open
route set and are reachable by any caller.

## Behavior

- `queue/stats` reflects the resource-weighted queue described in
  `job-management.md`.
- Cleanup removes only jobs past the 30-minute post-completion retention
  (the same expiry the background sweeper enforces); it then reports the
  removed count and the remaining total.
- Failures reading manager state return a bare `500`.

## Verification

`job_manager.rs` `mod tests` covers stats derivation and expiry/cleanup
behavior.
