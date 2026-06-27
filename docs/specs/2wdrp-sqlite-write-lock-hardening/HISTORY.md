# History

## 2026-05-07

- Created after production showed transient SQLite `database is locked` errors across token billing,
  MCP session locks, scheduled job starts, and LinuxDo OAuth upserts.
- Chose bounded retry hardening as the first repair because the evidence showed short writer
  collisions, while API rebalance selection and research result key pinning were behaving as
  designed.

## 2026-05-24

- Extended the same lock-hardening line to forward-proxy startup: subscription refresh now fetches
  multiple feeds concurrently, runtime snapshot persistence retries transient busy/locked writes,
  and startup logs now break out sqlite, refresh, xray, and store-sync phases.

## 2026-05-31

- Moved repeated LinuxDo system tag refresh out of the startup readiness path after production
  timing showed SQLite initialization dominated by per-user binding sync on an already consistent
  database. Startup still performs a cheap mismatch repair check; periodic refresh now runs in the
  background scheduler.

## 2026-06-04

- Extended the hardening scope from transient write retries to operator-controlled scheduled jobs
  and DB size convergence. Manual and automatic maintenance now share job bookkeeping semantics, and
  SQLite file shrinkage is treated as an explicit compaction concern rather than an implicit side
  effect of deleting old rows.

## 2026-06-06

- Added a process-wide execution gate for DB-backed scheduled/manual jobs after production evidence
  showed stale running maintenance rows and overlapping writers across request-log GC, quota sync,
  and compaction. Request-log GC catch-up only holds the gate during active cleanup windows, not
  while sleeping between catch-up passes.

## 2026-06-09

- Extended the same hardening line to `quota_sync`: upstream `/usage` now has a hard timeout,
  `quota_sync` / `quota_sync/hot` runs are bounded and self-heal stale `running` rows at claim
  time, and failed runs explicitly finish as `error` without mutating quota snapshot/sample data.
- Added an offline `db_compaction_once` operator binary so maintenance compaction no longer depends
  on the in-process admin trigger path when the DB execution gate is busy.
- Reduced the default SQLite pool concurrency from `5` to `3` after production evidence showed that
  more writer-capable connections amplified contention instead of absorbing it.

## 2026-06-10

- Replaced the owner-facing “shared execution gate decides whether manual maintenance is rejected”
  model with a persisted maintenance queue on `scheduled_jobs`.
- Added `queued` lifecycle semantics (`queued_at`, nullable `started_at`, coalesced representative
  rows, startup abandon-all cleanup) plus a single in-process maintenance worker for DB-backed
  maintenance jobs.
- Scheduler loops now enqueue maintenance work instead of claiming-and-running inline; manual
  trigger APIs now return the representative queued/running job instead of surfacing
  `db_job_execution_busy`.

## 2026-06-11

- Tightened the representative-row contract so a later manual trigger can promote an already
  running scheduler row to `trigger_source=manual`, keeping `/api/jobs/trigger` and `/api/jobs`
  aligned on the same representative instance.
- Added queue-state hints (`status`, `coalesced`, `promoted`) to manual trigger responses so the
  admin UI can say whether work was newly queued or attached to an existing active job.
- Split `forward_proxy_geo_refresh` into a remote discovery phase plus a short DB persistence phase,
  and let the maintenance worker keep one bounded remote-I/O slot instead of blocking the queue on
  GEO I/O or fan-out starting multiple remote maintenance jobs at once.

## 2026-06-13

- Finished the first dual-DB stabilization pass after the observability sidecar split exposed
  follow-up drift in admin/test paths.
- Server test helpers now attach the sibling observability DB and use observability-aware schema
  probes, so migration/admin coverage exercises the same attached-database layout as production.
- Admin request-log page reads now self-heal missing catalog rollups before serving totals/facets,
  and auth-token log queries now qualify `auth_token_logs.*` columns explicitly after the
  `billing_ledger` join split removed synchronous billing truth from the legacy history table.

## 2026-06-15

- Added a large-legacy compatibility path for observability sidecar startup: when inline
  `request_logs` migration would exceed the startup budget, `observability` stays attached to the
  core DB so startup and offline GC can proceed without a heavy copy.
- Tightened attached-schema self-heal helpers to probe `observability` explicitly, preventing
  duplicate-column migrations when both `main.request_logs` and `observability.request_logs` are
  present during sidecar rollout or repair.

## 2026-06-17

- Promoted the large-legacy observability cutover from a deferred follow-up into an explicit
  offline operator flow via `observability_sidecar_migrate`.
- Locked the migration semantics to “copy only `request_logs`, then rebuild or self-heal derived
  observability tables in the sidecar layout” instead of attempting a full-table legacy transplant.
- Added resumable `request_logs` batch copy semantics keyed by preserved `id`, so partial sidecar
  copies can resume safely without duplicating rows or breaking preserved `request_log_id`
  references.
- Standardized the validation path around a shared-testbox single-node Compose harness plus a
  short-maintenance cutover/rollback runbook, with the pre-cutover core DB exported to testbox as
  the rollback anchor.
- Replaced the earlier WAL-mode offline-lock heuristic with an explicit sibling
  `observability-migrate.lock` contract so online service holders and the offline migration command
  coordinate through a process-visible file lock instead of inferring shutdown from `BEGIN EXCLUSIVE`.
- Recorded the first passing shared-testbox evidence for that flow: isolated compose project build,
  explicit sidecar migration, fresh token-backed `/api/tavily/search`, `/mcp`, and migrated
  request-log reads all succeeded against the migrated snapshot.

## 2026-06-18

- A short-maintenance cutover attempt exposed a contract bug in the first explicit migration: the
  tool copied `request_logs` and reset derived-table rebuild markers, then service startup
  synchronously awaited the large derived rollup rebuild before opening the HTTP listener.
- The explicit migration contract now treats a successful reopen of the normal startup path as part
  of completion, so `completed=true` only means “offline rebuild succeeded and the service can
  reopen cleanly after the migration lock is released.”
- Tightened the contract so `observability_sidecar_migrate` must complete all sidecar derived-table
  rebuild work and write completion meta before reporting success or deleting hidden legacy
  observability tables.
- Tightened the finalization order again after review: the migration now drops hidden legacy
  observability tables before writing completion meta and the explicit cutover marker, avoiding a
  false completed state if final table deletion is interrupted.
- Added startup protection for explicit cutovers: a sidecar with the explicit cutover marker,
  historical rows, and missing derived completion meta now fails fast with an operator instruction
  to rerun the offline tool instead of turning startup into an implicit migration worker. The guard
  is scoped to the explicit marker so legacy startup and small automatic sidecar self-heal remain
  compatible.
- Tightened completion checks so only exact `1` meta values count as complete; stale `0` values
  remain rebuild-needed and are recovered by rerunning the offline tool.
- Restored `valuable_failure_429_count` in the offline `api_key_usage_buckets` rebuild path so
  per-key upstream-429 failures survive explicit sidecar migration.

## 2026-06-20

- Validated the first lossless startup/read-path hardening pass against a live production SQLite
  snapshot copied online with SQLite `.backup`, then replayed on the shared testbox.
- The first upgraded boot still paid one-time repair/migration work on the historical snapshot:
  `billing_ledger` repair ran once, and the new alert-supporting indexes were created on
  `auth_token_logs`.
- The second boot on that same repaired snapshot logged
  `billing ledger startup precheck skipped: ... reason=no_gap` and reduced
  `sqlite startup total` from about `24s` to about `2.2s`, confirming the routine restart tax is
  gone after the first successful repair.
- The same production-derived replay returned `/api/public/metrics` in about `1.44s`,
  `/api/alerts/events` in about `0.14s`, and emitted the first public SSE `metrics` event
  immediately after connect.
- Offline `billing_ledger_audit` on that snapshot still reported `118` day-only mismatches between
  quota samples and ledger-derived daily windows. That audit drift remains a separate quota
  correctness follow-up and is not repaired by the startup ledger bootstrap path itself.

## 2026-06-22

- Production evidence re-opened the topic at the request hot path: `/health` and `/api/version`
  stayed responsive, but the latest sampled hour still showed `34` transient `database is locked`
  failures and `296` slow statements.
- The highest-value live signatures were `record_pending_billing_attempt failed for /mcp`,
  `db operation error: operation=request stats persist`, and a public metrics month-tail query
  shaped like `WITH scoped_logs AS (...) FROM observability.request_logs`.
- The fix line stayed narrow: request-path pending billing now retries inside one bounded
  transaction, request-stats flush now retries with batch-aware requeue proof and structured
  contention logs, and public success metrics stop using the retained-log wide-scan fallback.

## 2026-06-27

- Tightened serving-role `/health` so the startup grace window no longer masks missing
  forward-proxy runtime or shared xray readiness on `single`, `full_master`, or
  `provisional_master`.
- Preserved the accepted `f36b4` HA carve-out: `standby` / `recovery` still report `200 ok` on
  `/health` while business runtime warmup is intentionally skipped.
- Moved `server_pressure_buckets` historical rebuild out of the startup critical path into one
  post-listener background rebuild, then tightened that background path so later HA promotions also
  trigger it, HA demotions cancel it cleanly, and each rebuild commits from one transactional
  request-log snapshot.
- Cold startup subscription refresh now fans out across the whole configured URL set in one wave,
  so 5-8 slow feeds no longer turn strict readiness into multiple 60-second timeout batches.
- Tightened the image `HEALTHCHECK` timing to a builder-compatible 20-second minimum gate plus
  `start-period=20s/interval=5s/timeout=5s/retries=18`, so container healthy state still tracks
  the stricter serving contract without depending on Docker 25-only `--start-interval`.
