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
