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

On `2026-06-22`, one production sample provided a clean example of this shape: `/health` and
`/api/version` stayed fast, but the latest sampled hour still contained `34`
`database is locked` errors and `296` slow statements, concentrated around
`record_pending_billing_attempt failed for /mcp`, `request stats persist`, and a retained-log
month-tail public metrics scan.

## Resolution

- Add a runtime DB logging contract before changing lock semantics again. For this service, default
  runtime logging to JSON lines on stderr via `tracing`, and keep a documented `text` fallback for
  grep-oriented local workflows. Runtime DB phases must still emit stable fields for
  `component=db`, `event=operation_slow|operation_error`, `operation`, `elapsed_ms`, optional
  `context`, and optional `err`.
- Extend that same default runtime logging contract to the HA replication path. Baseline/events
  export/import and standby sync should emit stable `component=ha event=...` perf records with
  `elapsed_ms`, `channel`, `row_count`, `payload_bytes`, optional `compressed_bytes`, and runtime
  memory headroom fields. Do not require a second metrics pipeline just to answer “which HA leg is
  holding memory right now?”
- Enable SQL-level slow statement logging directly on runtime `sqlx` SQLite connect options. The
  default threshold is `250ms` for SQL statements and `1s` for explicit DB operation phases such as
  startup pool open, schema init, request-stats flush, scheduler enqueue, OAuth upsert, and
  pending billing settlement.
- Keep stable startup/runtime event names so operators can tell whether the time went into pool
  open, observability attach probing, `BEGIN IMMEDIATE`, schema bootstrap, or a later
  request/worker write path. In JSON mode this comes from structured `component/event/...` fields;
  in fallback text mode the same fields remain grep-friendly.
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
- Keep serving `/health` stricter than internal startup grace. If core serving traffic still
  depends on forward-proxy runtime or shared xray readiness, return non-`200` immediately instead
  of masking that gap with a grace-period green state; preserve any explicit HA standby/recovery
  minimal-health carve-out separately.
- If a derived observability rebuild is not core truth, do not keep it on the startup critical
  path. Trigger it once after the listener is already serving, trigger it again when a later HA
  promotion turns the node back into a business-serving role, keep failure isolated to logs or
  stale/empty analysis views, and cancel or roll back the rebuild if the node is demoted before it
  commits.
- If strict cold startup still depends on subscription-backed forward-proxy runtime, do not let a
  small configured subscription set spill into fixed-size timeout batches before readiness. Fan the
  whole startup set out in one wave, and keep the fail-closed rule tied to “every subscription
  fetch failed” rather than to a per-batch wall-clock artifact.
- Align deploy health timing with that stricter contract. In this service the accepted image
  baseline is `start-period=20s`, `interval=5s`, `timeout=5s`, and `retries=18`, with the
  healthcheck command itself enforcing the 20-second minimum-success gate. That keeps old Docker
  builders working while still stopping a fast successful probe from flipping the container healthy
  before the intended startup hold window has elapsed.
- Keep retention cleanup bounded. Large `request_logs` backlogs should be deleted in small batches
  with a runtime/batch budget and a catch-up delay, rather than one daily job holding or repeatedly
  contesting the writer until the whole backlog is gone.
- When catch-up is needed, prefer one bounded cleanup pass per `scheduled_jobs` row. Finish the
  job after the pass, record the bounded-pass totals in the message, and let the scheduler claim a
  fresh job after the recheck delay. Do not keep one `running` job row open while sleeping between
  catch-up windows.
- Keep scheduled-job trigger provenance separate from logical job type. Use a dedicated
  `trigger_source` column for scheduler/manual/auto runs so filters, duplicate detection, and
  history remain stable as operators gain manual trigger buttons.
- When DB-backed maintenance work starts contending with request-path writes, do not make
  owner-facing manual operations fight for the same execution gate. Persist maintenance jobs as
  `queued`, coalesce duplicate logical work onto one representative row, and let one maintenance
  worker consume that queue.
- If a later manual trigger attaches to an already active representative row, surface that fact
  explicitly. Promoting the representative `trigger_source` and returning `status/coalesced` hints
  keeps the admin UI from falsely implying that a new job was rejected or silently ignored.
- When a later trigger can reuse an already active representative row without changing its
  priority/source, do that coalescing through a read-only fast path. Requiring `BEGIN IMMEDIATE`
  before checking the active row turns harmless duplicate manual triggers into transient HTTP 500s
  whenever a bounded GC slice is holding SQLite's writer slot.
- Keep `queued_at` separate from `started_at`. A queued job has been accepted but has not entered a
  DB execution window yet; collapsing those timestamps makes queue delay invisible and breaks admin
  diagnosis.
- Treat SQLite file shrinkage as a separate maintenance concern. Row deletes and body nulling create
  free pages; size convergence requires freelist telemetry plus a controlled compaction job after
  retention cleanup has made space reclaimable.
- Serialize DB-backed scheduled/manual maintenance jobs through one persisted queue plus one
  in-process worker. Same-job duplicate claiming alone is not enough when different logical jobs,
  such as retention GC, quota sync, rollups, and compaction, can all compete for the single writer
  slot.
- Keep hot-path billing subject serialization in-process for the single-process deployment model.
  Using a SQLite lock table for every request turns one writer-slot collision into a write-amplified
  failure mode.
- Split billing truth from request history. `billing_ledger` should carry the synchronous pending /
  charged state, while `auth_token_logs` remains the legacy history surface that is mirrored for
  compatibility.
- Batch request-derived rollups. `request_logs` should write synchronously, but dashboard/API-key
  usage, auth-token activity counters, account request-rate buckets, and catalog rollups should be
  coalesced and flushed in bounded windows instead of being updated per request.
- If those observability-heavy tables move into an attached sidecar SQLite file, treat them as
  rebuildable/eventually consistent views rather than HA-trigger-replicated truth. SQLite attached
  database triggers cannot safely write back into `main`, so the HA outbox should stay focused on
  core control-plane and billing truth tables.
- Do not reuse one HA whitelist for both baseline export and change-event triggers. In this codebase
  that caused `billing_ledger` and runtime quota tables to bloat `ha_outbox` even in effectively
  single-node production. Keep HA channels explicit: `control` small-state, `billing` dedicated
  ledger truth, and `runtime` minimal correctness state.
- For control-channel cursor validation, match the SQLite index to the actual predicate shape.
  This service validates `SELECT MIN(seq) ... WHERE created_at >= ? AND resource IN (...)`; the
  plain `(created_at, seq)` index is not enough once retained backlog grows. Keep a
  `(resource, created_at, seq)` index and a query-plan regression test so the planner does not
  fall back to scanning by time and filtering resources row-by-row.
- Do not stop at splitting HA channels if the replication path still materializes a whole baseline
  or whole event batch in memory. In this service the next failure mode after channel split was a
  billing baseline path that still built one giant NDJSON string on the active node and one giant
  decompressed blob on the standby node. The reusable rule is: state-replication paths must stream
  rows/events end-to-end and apply incrementally within a bounded transaction.
- The same role gate must cover background writers, not just external request handlers. In this
  service, standby still looked “fenced” from the outside while `quota_sync`, usage rollups, GC,
  and maintenance schedulers were quietly enqueueing and writing into SQLite. That is enough to
  break a long-running HA apply transaction even when the request path is fully blocked.
- When diagnosing memory growth alongside lock pressure, report cgroup memory and process RSS/HWM
  together. On 101 the observed shape was `memory_current_bytes` near `3GB` while
  `process_rss_bytes` stayed around `560MB`; that is evidence to first reduce dashboard/HA/SQLite
  read-write amplification before calling it a heap leak.
- If HA sync persists node-state or watermark metadata through a coalescing writer, flush that
  metadata at safe boundaries between channel apply sessions. Waiting until the end of the whole
  sync loop can make the next channel's `BEGIN IMMEDIATE` collide with delayed bookkeeping writes
  and surface as nested-transaction or `database is locked` failures.
- If an authority/health refresh loop keeps re-emitting the exact same HA node-state payload, do
  not let that become a periodic SQLite rewrite. In this service, a standby node whose EdgeOne
  authority stayed unchanged could still enqueue the same `ha_node_state` row every five seconds;
  deduplicating identical coalesced snapshots removed both the slow-statement noise and an
  otherwise needless writer competitor.
- Keep `HA_MODE=single` truly silent for HA replication writes. Leaving replication triggers enabled
  on a single live node creates unbounded local-only backlog with no standby consumer.
- Treat large retained HA event cleanup like request-log cleanup: bounded online GC for freshness,
  explicit offline one-shot cleanup for backlog removal, and optional later compaction only when
  reclaimable bytes justify it.
- For upgraded HA databases, treat trigger repair as a separate first-class maintenance step.
  If legacy `trg_ha_outbox_*` triggers from the old single-channel era remain in `sqlite_master`,
  online GC will never catch up because the live node keeps appending fresh non-control noise into
  `ha_outbox`. Repair the trigger set first, then start backlog cleanup.
- When offline maintenance proof depends on a production-derived SQLite copy, define the input as a
  full DB set instead of a single file. In this service that means the core DB plus the
  observability sibling sidecar; copying only `tavily_proxy.db` would miss the attached
  request-log/read-model layout that production actually serves.
- Once observability tables move into a sidecar, test helpers and admin/read paths must become
  sidecar-aware too. Unqualified schema probes or direct core-only SQLite opens can silently stop
  covering `request_logs` even though production still reads that table through the attached
  `observability` database.
- If a legacy DB is large enough that inline sidecar migration would blow the startup budget, do
  not force that copy in the readiness path. Keep `observability` attached to the core DB for that
  startup/maintenance session, and let offline GC or later explicit migration handle the backlog.
- That large-legacy compatibility path should not also collapse the SQLite pool to a single
  connection. Doing both at once makes owner-facing summary flushes and early scheduler enqueues
  contend for one slot, so `/health` may go green while `/api/summary` still returns transient
  500s.
- When both `main.request_logs` and `observability.request_logs` can coexist temporarily, schema
  probes must target the attached schema explicitly. Generic `pragma_table_info('request_logs')`
  lookups can resolve against the wrong DB and trigger duplicate-column repairs.
- Owner-facing log pages that rely on coalesced catalog rollups should flush or rebuild those
  rollups before serving totals/facets. Otherwise the sidecar split removes write pressure from the
  hot path, but leaves `/api/logs` vulnerable to showing empty totals while raw `request_logs` rows
  are still present.
- After moving synchronous billing truth into `billing_ledger`, any admin history query that joins
  `auth_token_logs` to billing state must qualify legacy-table columns explicitly and avoid
  unnecessary joins in count/facet queries. Mixed ledger/history reads otherwise regress into
  `ambiguous column name` failures under ordinary owner-facing token-log filters.
- For maintenance jobs that mix remote I/O with SQLite writes, split those phases. Remote fetches
  such as forward-proxy GEO refresh or quota `/usage` probes should not hold the SQLite-writing
  execution gate, and they should not pin the queue worker when the remaining DB phase can be
  resumed separately. At the same time, do not “solve” that by fan-out spawning every remote job:
  keep a bounded remote-I/O slot so the queue cannot turn a backlog into an upstream stampede.
- Keep `quota_sync` bounded. `/usage` fetches should have a hard timeout, the whole sync run should
  finish on a short wall-clock budget, and stale `quota_sync` / `quota_sync/hot` `running` rows
  should be abandoned during the next claim instead of waiting for a restart.
- Do not hold that job execution gate while a catch-up scheduler is sleeping between cleanup
  windows. Hold it for the active DB write window, then release it before throttled rechecks.
- Provide a one-shot operational CLI for retention cleanup so production-derived database samples
  can be tested deterministically. Do not rely only on the daily scheduler when validating cleanup
  behavior.
- Provide the same offline path for compaction. Manual HTTP trigger endpoints are still useful, but
  operators need a `db_compaction_once`-style bypass for maintenance windows when the online job
  gate is busy.
- For HA maintenance windows, the safe order is `ha_trigger_repair_once` (or
  `ha_outbox_cleanup_once --repair-triggers`) first, `ha_outbox_cleanup_once` second, and
  `db_compaction_once` optional third. Reversing that order can leave the live node still writing
  invalid backlog, or vacuum retained dead rows too early and waste I/O without reclaiming the real
  problem.
- If the live system stores the main DB and observability data in sibling SQLite files, export the
  offline validation input with SQLite `.backup` per file and carry forward SHA-256 plus
  `PRAGMA integrity_check` evidence for each member of the set.
- For hot WAL-mode production databases, prefer a single-step backup for offline export instead of
  small incremental page loops. SQLite's incremental backup API can restart when the live source is
  modified underneath it; on a busy writer this can turn a snapshot export into an apparent
  livelock. Use page-level progress reporting for observability, but default the real export path
  to one single-step copy.
- If the snapshot export path stages large temporary backup files on the source host, treat cleanup
  of those staging directories as part of the same maintenance runbook. A successful upload to the
  shared testbox is not the end of the flow if tens of GiB remain under a temporary source path.
- Treat disk hygiene as an explicit post-maintenance step. Remove orphaned one-off snapshot
  directories, stale gzip/sqlite artifacts, and dangling images once the new release is verified, or
  the next “database is too large” incident can be self-inflicted by leftover maintenance inputs
  rather than live product data.
- Avoid high-resource retention catch-up tactics such as rebuilding large log tables or producing a
  large WAL. If the backlog is very large, run repeated bounded cleanup windows and verify progress
  with row counts and resource telemetry.
- Keep startup backfills cheap and no-op aware. Large production SQLite files make repeated per-user
  repair loops expensive even when every row is already correct; use an indexed precheck in the
  readiness path and move periodic refresh work to a background scheduler.
- For `billing_ledger` startup truth repair specifically, persist a high-watermark marker and let
  the readiness path prove “no gap / no drift” before invoking a whole-ledger reconcile. The first
  upgraded boot may still need one repair, but steady-state restarts should only pay the precheck.
- If an owner-facing read depends on coalesced rollups, prefer a freshness-gated flush over an
  unconditional flush. This keeps near-real-time semantics without turning every public/admin read
  into a write barrier under SQLite's single-writer budget.
- Do not let an SSE freshness poll become that write barrier by accident. In this service,
  `/api/events` was polling every 2 seconds; when its freshness path called the same
  `summary_windows` / rollup-flush helpers as the dashboard rebuild, it re-heated SQLite write
  contention even before a human opened a heavier admin page. Keep the poll path on cheap
  no-flush reads plus pending-coalescer signatures, and reserve the actual flush for the shared
  snapshot rebuild that will be emitted to clients.
- When request-path billing needs both a history row and a ledger row, keep them in one SQLite
  transaction before adding retries. Retrying two independent writes can duplicate the history row
  and only masks the actual contention bug.
- For request-path and flush contention, expose one stable structured retry/exhaustion contract:
  `operation`, `request_path`, `request_kind`, `attempt|attempts`, `backoff_ms`, `elapsed_ms`,
  `retry_budget_ms`, `pending_batch_counts`, `oldest_pending_created_at`,
  `newest_pending_created_at`, and `billing_subject_kind`. Use `token|account|unknown` only; never
  log raw billing subjects, token secrets, or request bodies.
- If public metrics only need success counts, do not reuse a generic retained-log summary scan for a
  month-tail fallback. Subtract the retained tail from the last daily rollup bucket with a bounded
  success-count query instead of reintroducing a wide `WITH scoped_logs AS (...)` scan.
- Do not let lock-contention tests depend on real wall-clock sleep just to cross a retry window or a
  one-second timestamp boundary. Prefer deterministic state shaping or a controlled time seam so the
  test still exercises the production retry logic without paying real-time cost.
- Prefer bounded retries and narrower write windows before increasing SQLite pool size.

## Guardrails / Reuse Notes

- Do not enable full SQL debug logging in production by default. Slow-statement logging is enough
  for this contention class and avoids dumping every statement or bind-heavy traffic path.
- Treat runtime DB operation logs and `sqlx::query` slow warnings as complementary:
  `sqlx::query` answers “which statement was slow,” while `db operation ...` answers “which
  service-layer operation or phase was slow/failing.”
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
- Do not treat a large main SQLite file as proof that the main DB alone is the whole persistence
  surface. In this project the observability sibling sidecar is part of the production-shaped input,
  while `ha_outbox` and other retained rows can make the core file look “unreasonably large” until
  retention cleanup and optional compaction are completed in the right order.
- A successful cleanup proof does not guarantee that offline compaction can run on every shared
  validation host. `VACUUM` still needs enough free filesystem headroom for the rewritten database.
  When cleanup shows multi-GiB reclaimable space but the shared validation host has only a few GiB
  left, record that as an environment-capacity blocker and reserve the actual compaction for a real
  maintenance window with adequate free space.
- Stale `scheduled_jobs.running` rows from a previous process lifetime are still an operational
  restart concern. Claim-time stale abandonment should cover fresh quota-sync wedges, but the
  broader maintenance queue should abandon every leftover `queued`/`running` row on startup instead
  of implicitly resuming unknown partial work from an old process.
- If a retention table has aggregate-maintenance triggers, validate large-copy cleanup with the
  triggers in mind. For `request_logs`, GC deletes expired rollup buckets separately and suppresses
  the per-row rollup delete trigger inside each batch transaction to avoid spending minutes per
  batch on redundant aggregate updates.
- If an owner-facing read surface depends on coalesced rollups, flush the batcher before reading
  or rebuild from source rows when a legacy/manual path bypasses the coalescer.

## References

- `src/store/mod.rs`
- `src/bin/request_logs_gc_once.rs`
- `src/bin/ha_outbox_cleanup_once.rs`
- `scripts/export-live-db-snapshot-to-testbox.sh`
- `src/store/key_store_bootstrap.rs`
- `src/store/key_store_users_and_oauth.rs`
- `src/store/key_store_request_logs_and_dashboard.rs`
- `src/tavily_proxy/proxy_auth_and_oauth.rs`
- `src/tavily_proxy/proxy_ha.rs`
- `src/server/schedulers.rs`

## 2026-06-22 validation set

- `cargo test mcp_tools_call_tavily_search_retries_pending_billing_when_sqlite_writer_lock_releases -- --nocapture`
- `cargo test ensure_user_token_binding_with_preferred_retries_when_begin_is_locked -- --nocapture`
- `cargo test public_success_breakdown_waits_for_inflight_flush_before_serving_metrics -- --nocapture`

## Rollout grep

- `journalctl -u tavily-hikari -n 2000 | rg 'sqlite_transient_write_retry|sqlite_transient_write_exhausted'`
- `journalctl -u tavily-hikari -n 2000 | rg 'operation=request stats persist|operation=insert_token_log_pending_billing|operation=apply_pending_billing_log'`
- `journalctl -u tavily-hikari -n 2000 | rg 'record_pending_billing_attempt failed for /mcp|WITH scoped_logs AS'`

## Symptom Mapping

For the `2026-06-19 01:00 +08:00` onward production sample, the runtime DB log contract should map
the observed symptoms like this:

- `forward-proxy startup: sqlite initialized in 38906ms`
  -> keep the startup lifecycle event and expect
  `component=db event=operation_slow operation="sqlite startup" ...`
- `quota-sync-hot: enqueue job error: ... database is locked`
  -> expect `component=db event=operation_error operation="scheduled job enqueue" ...`
- `request stats persist warning: ... database is locked`
  -> expect `component=db event=operation_error operation="request stats persist" ...`
- `upsert linuxdo oauth account error: ... database is locked`
  -> expect `component=db event=operation_error operation="oauth account upsert" ...`
- `oauth account upsert: transient sqlite write error (...)`
  -> keep bounded retry logs and expect the final phase-level `oauth account upsert` slow/error log
- `apply_pending_billing_log: transient sqlite write error (...)`
  -> keep bounded retry logs and expect the final phase-level
  `component=db event=operation_slow|operation_error operation="apply_pending_billing_log" ...`
- request-path `/api/tavily/search` / MCP billing failures
  -> correlate request warning lines with `apply_pending_billing_log`, quota/billing lock logs, and
  `sqlx::query` warn lines for slow statements on the same wall-clock window
