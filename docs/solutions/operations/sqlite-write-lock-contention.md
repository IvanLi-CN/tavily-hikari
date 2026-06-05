---
title: SQLite write-lock contention
module: tavily-hikari
problem_type: production_lock_contention
component: sqlite-writes
tags:
  - sqlite
  - production
  - billing
  - mcp
status: active
related_specs:
  - docs/specs/2wdrp-sqlite-write-lock-hardening/SPEC.md
  - docs/specs/s2vd2-upstream-credits-billing/SPEC.md
  - docs/specs/34pgu-mcp-session-privacy-affinity-hardening/SPEC.md
---

# SQLite write-lock contention

## Context

Tavily Hikari uses one SQLite database for billing ledgers, session affinity, scheduled job logs,
OAuth account state, quota sync samples, and admin/read models. WAL mode allows readers and one
writer to coexist, but only one writer can hold the write slot at a time.

## Symptoms

- Logs contain `database is locked` while `/health` remains OK.
- Request-path messages mention `token billing lock failed` or `mcp session ... lock failed`.
- Background messages mention `token-usage-rollup: start job error`, `quota-sync-hot: start job error`, or LinuxDo OAuth upsert failures.
- Startup logs may show `forward-proxy startup: ...` phases taking a long time when runtime
  snapshot persistence or subscription refresh collides with another writer.
- Deploy health may remain `starting` when restart waits for remote subscription refresh before
  restoring previously working subscription-backed proxy nodes from the local runtime table.
- WAL can be large without itself proving corruption; it is a signal to inspect writer pressure and
  long readers before performing maintenance.

## Root Cause

Short-lived SQLite writer collisions can happen when request-path billing/session locks and
background writes all touch the same DB. Treating every transient busy/locked response as fatal makes
brief contention visible as HTTP 500s or failed background bookkeeping.

## Resolution

- Keep billing and MCP serialization fail-closed, but retry transient SQLite busy/locked writes
  inside the existing bounded lock wait or lease budget.
- Retry background job bookkeeping writes before surfacing scheduler errors.
- Retry OAuth upsert/refresh wrapper calls so login/profile sync can survive short writer collisions.
- Retry forward-proxy runtime snapshot persistence at the startup/maintenance boundary so a short
  writer collision does not stretch readiness.
- Overlap startup subscription fetches where possible, but keep the refresh fail-closed if every
  feed fails.
- Restore safely attributable persisted subscription-backed proxy nodes from `forward_proxy_runtime`
  before attempting remote subscription refresh. If that restored graph exists, use it for startup
  readiness and leave remote subscription calibration to the maintenance scheduler.
- Keep retention cleanup bounded. Large `request_logs` backlogs should be deleted in small batches
  with a runtime/batch budget and a catch-up delay, rather than one daily job holding or repeatedly
  contesting the writer until the whole backlog is gone.
- Keep scheduled-job trigger provenance separate from logical job type. Use a dedicated
  `trigger_source` column for scheduler/manual/auto runs so filters, duplicate detection, and
  history remain stable as operators gain manual trigger buttons.
- Treat SQLite file shrinkage as a separate maintenance concern. Row deletes and body nulling create
  free pages; size convergence requires freelist telemetry plus a controlled compaction job after
  retention cleanup has made space reclaimable.
- Serialize DB-backed scheduled/manual maintenance jobs through an in-process execution gate when
  they can write SQLite. Same-job duplicate claiming is not enough when different logical jobs, such
  as retention GC, quota sync, rollups, and compaction, can all compete for the single writer slot.
- Do not hold that job execution gate while a catch-up scheduler is sleeping between cleanup
  windows. Hold it for the active DB write window, then release it before throttled rechecks.
- Provide a one-shot operational CLI for retention cleanup so production-derived database samples
  can be tested deterministically. Do not rely only on the daily scheduler when validating cleanup
  behavior.
- Avoid high-resource retention catch-up tactics such as rebuilding large log tables or producing a
  large WAL. If the backlog is very large, run repeated bounded cleanup windows and verify progress
  with row counts and resource telemetry.
- Keep startup backfills cheap and no-op aware. Large production SQLite files make repeated per-user
  repair loops expensive even when every row is already correct; use an indexed precheck in the
  readiness path and move periodic refresh work to a background scheduler.
- Prefer bounded retries and narrower write windows before increasing SQLite pool size.

## Guardrails / Reuse Notes

- Do not fix this class of problem by simply raising `sqlx` pool size; more concurrent writers can
  increase lock pressure.
- Do not hand-edit production ledgers. Use repository repair binaries or controlled migrations when
  historical data needs correction.
- Keep request-path quota semantics stable: locked billing subject, pending replay, quota precheck,
  and settlement must remain one coherent subject.
- For WAL growth, inspect active readers and checkpoint behavior before running live maintenance.
- Deleting rows does not shrink the SQLite file by itself. Treat VACUUM or database replacement as
  a separate maintenance-window decision after retention cleanup has completed.
- Automatic compaction should be threshold-gated and cooldown-limited. Triggering it on every GC
  pass can turn a cleanup backlog into a new writer-pressure loop.
- Stale `scheduled_jobs.running` rows from a previous process lifetime are an operational restart
  concern. The process-level gate prevents new same-process writer overlap, but a controlled restart
  may still be needed to let startup stale-job cleanup mark old rows abandoned before operators
  retry manual jobs.
- If a retention table has aggregate-maintenance triggers, validate large-copy cleanup with the
  triggers in mind. For `request_logs`, GC deletes expired rollup buckets separately and suppresses
  the per-row rollup delete trigger inside each batch transaction to avoid spending minutes per
  batch on redundant aggregate updates.

## References

- `src/store/mod.rs`
- `src/bin/request_logs_gc_once.rs`
- `src/store/key_store_users_and_oauth.rs`
- `src/store/key_store_request_logs_and_dashboard.rs`
- `src/tavily_proxy/proxy_auth_and_oauth.rs`
