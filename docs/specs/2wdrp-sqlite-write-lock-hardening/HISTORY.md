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
