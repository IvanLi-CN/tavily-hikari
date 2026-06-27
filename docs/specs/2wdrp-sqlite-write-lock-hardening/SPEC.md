# SQLite write-lock hardening（#2wdrp）

## Status

- Lifecycle: active
- Created: 2026-05-07
- Last: 2026-06-22

## Background

Production `tavily-hikari` `0.46.2` showed transient SQLite `database is locked` errors after API
rebalance was enabled. The service remained healthy, but logs included request-path failures such as
`token billing lock failed` for `/api/tavily/search` and `/api/tavily/extract`, plus MCP session
lock failures and background job start failures.

The observed behavior points to SQLite writer contention rather than API rebalance selector
misrouting. Billing, MCP session serialization, quota sync, scheduled job logging, LinuxDo OAuth
upserts, and rollups all share the same SQLite database and can briefly compete for the single
writer slot.

After the first maintenance-queue hardening, the remaining production noise shifted toward request
hot-path writes: billing-subject lock rows, request-log derived rollups, and HA runtime snapshots.
The topic now also covers shrinking those synchronous hot-path write windows without weakening
billing correctness.

Forward-proxy startup also participates in this same lock budget: it refreshes subscription-backed
endpoints, syncs xray state, and persists runtime snapshots before the HTTP server reports itself
ready. That startup path can amplify a short writer collision into a slow first healthy when the
runtime snapshot write collides with other writers.

Dockrev `job_01KSEPDVXF8NAQCEQGJKV33T4F` showed a related startup-order failure after the previous
hardening: the service was healthy before restart, but startup still waited on remote subscription
refresh before restoring the locally persisted subscription runtime. A restart should recover known
proxy nodes from `forward_proxy_runtime` first; remote subscription refresh is only the calibration
source when a usable persisted runtime already exists.

## Goals

- Keep request-path billing and MCP session locks from failing on transient SQLite busy/locked
  errors when the existing bounded wait budget can absorb the contention.
- Keep billable `/mcp` request completion and pending-billing settlement on the same bounded
  transient-write retry contract, so a short production writer lock no longer produces
  `record_pending_billing_attempt failed for /mcp: database is locked` while the upstream response
  itself was otherwise successful.
- Preserve quota ledger correctness, pending billing replay, session affinity, research key pinning,
  and API rebalance behavior.
- Remove avoidable request hot-path SQLite writes that do not need to be synchronous truth, while
  preserving fail-closed billing semantics and owner-facing observability.
- Keep public success metrics on rollup-first bounded reads so month-tail fallback no longer depends
  on a retained-log wide scan against `observability.request_logs`.
- Make background job bookkeeping tolerate transient lock pressure without amplifying request-path
  failures.
- Keep forward-proxy startup from turning short SQLite writer contention into a long readiness
  delay.
- Keep serving-role `/health` from reporting green before the forward-proxy runtime and shared xray
  are actually ready for core business traffic.
- Keep non-core derived observability rebuild work such as `server_pressure_buckets` out of the
  listener-before-ready critical path.
- Cover the lock-contention behavior with local tests that use only local/mock state.

## Non-goals

- No production data repair, destructive maintenance, or hand-written production ledger SQL.
- No increase to the SQLite connection pool as the primary fix.
- No billing reservation redesign unless this hardening proves insufficient.
- No change to public HTTP, MCP, DB schema, or frontend contracts.

## Requirements

- `quota_subject_locks` acquire/refresh/release writes must retry transient SQLite write errors with
  bounded backoff and must remain inside the existing lock timeout/lease budget.
- Token billing and MCP session lock callers must retain the current fail-closed semantics if the
  bounded retry window is exhausted.
- `record_pending_billing_attempt*`, `insert_token_log_pending_billing`, and
  `apply_pending_billing_log` must share one bounded transient SQLite write retry contract.
  Successful recovery within budget must keep the request/settlement green; budget exhaustion must
  still fail closed.
- `scheduled_jobs` must remain the persisted fact source for maintenance work, but it now needs a
  first-class queued lifecycle: `queued` rows persist before execution, `queued_at` records queue
  admission time, and `started_at` only records the actual execution start time.
- `scheduled_jobs` enqueue/start/finish writes must retry transient SQLite write errors before
  surfacing a background job logging failure.
- LinuxDo OAuth account upsert/refresh calls must retry transient SQLite write errors at the proxy
  boundary so a short writer collision does not immediately fail user login/profile sync.
- `forward_proxy` startup runtime snapshot persistence must retry transient SQLite write errors with
  bounded backoff so a short writer collision does not delay readiness longer than necessary.
- Startup subscription refresh may fetch multiple subscription URLs concurrently, as long as the
  refresh still fails closed when every subscription fetch fails.
- When startup has no safely restorable subscription runtime, the configured subscription URL set
  must still complete in one startup wave instead of fixed 4-URL timeout batches, so strict cold
  startup wall time stays bounded by a single subscription-fetch timeout.
- Startup must restore persisted subscription endpoints before remote subscription refresh when
  their configured subscription ownership is unambiguous. When at least one restored subscription
  endpoint exists, startup must not block on remote refresh before xray sync/runtime persistence;
  without safely restorable endpoints, startup continues to wait for subscription readiness.
- Serving-role `GET /health` must ignore the forward-proxy startup grace window. If the current
  business-serving role still depends on local relay and shared xray/runtime is not actually ready,
  `/health` must return non-`200` immediately instead of reporting a grace-period green state.
- `active_standby` `standby` / `recovery` keep the accepted HA minimal-health carve-out: their
  `/health` contract remains `200 ok` even when xray/runtime is intentionally not prewarmed.
- `rebuild_server_pressure_buckets()` must not block listener bind, first strict `/health`, or
  first healthy image status. Once the process is already serving business traffic, it may run once
  as a best-effort background rebuild whose failure is isolated to logs/observability and does not
  turn serving `/health` red.
- If a later HA transition promotes the process back into a business-serving role, the same
  best-effort `server_pressure_buckets` rebuild must be scheduled for that serving tenure too.
- If HA demotes the process out of a business-serving role while that rebuild is still pending, the
  rebuild must cancel or roll back instead of committing new pressure buckets from a standby or
  recovery process.
- The default image `HEALTHCHECK` must align with the stricter serving contract by using
  `start-period=20s`, `interval=5s`, `timeout=5s`, and `retries=18`, while the healthcheck command
  itself keeps the first green transition at or after the intended 20-second hold window without
  depending on Docker 25-only flags.
- Scheduled job records must preserve the logical job type and record the trigger source separately
  as `scheduler`, `manual`, or `auto`. Manual runs must not be encoded by appending suffixes to
  `job_type`.
- Manual scheduled-job triggers must use the same execution path as scheduler runs, coalesce onto an
  existing `queued`/`running` representative row of the same logical job, and return that
  representative `job_id` instead of rejecting on a shared execution gate timeout.
- Manual trigger responses may add queue-state hints such as representative `status`,
  `coalesced=true`, and `promoted=true` as long as the existing `jobId` / `jobType` /
  `triggerSource` contract remains backward compatible.
- `request_logs_gc` must not hold the SQLite write slot for an unbounded full-retention cleanup
  pass. It must delete old `request_logs` and `request_log_catalog_rollups` in bounded batches,
  yield between batches, report partial progress, and continue catch-up after a throttled delay when
  more rows remain.
- `quota_sync` and `quota_sync/hot` must run under a bounded runtime budget. Upstream `/usage`
  fetches must use a hard timeout, timeout/error paths must finish the `scheduled_jobs` row as
  `error`, and failed runs must not write quota snapshot/sample data.
- Claiming `quota_sync` / `quota_sync/hot` must abandon stale `running` rows older than the
  configured timeout window inside the same claim transaction, so a stuck job does not block future
  sync attempts forever.
- DB-backed scheduled and manual maintenance jobs that can write SQLite must flow through one
  persisted maintenance queue and one single-process worker. The worker may orchestrate remote I/O
  phases outside the DB execution window, but only one maintenance job may hold the SQLite-writing
  execution gate at a time, and the worker must not fan out multiple remote-I/O maintenance jobs at
  once just because those phases are outside SQLite.
- Request-log GC catch-up must finish one bounded slice, persist its progress message, and requeue a
  fresh `queued` row when more backlog remains instead of keeping one long-lived `running` row while
  waiting for the next catch-up opportunity.
- Service startup must abandon any leftover `queued` or `running` maintenance rows from the previous
  process lifetime rather than implicitly resuming them after restart.
- SQLite file size must converge after retention cleanup. The service must expose DB size/freelist
  telemetry, automatically trigger compaction when reclaimable space crosses the configured
  threshold, and provide a manual compaction trigger. Health checks must remain available while DB
  maintenance is active.
- A one-shot request-log GC CLI must reuse the same bounded cleanup path so production database
  samples can be validated deterministically without waiting for the daily scheduler.
- A one-shot DB compaction CLI must reuse the same threshold logic, support forced execution for a
  maintenance window, and remain available even when the in-process admin trigger is blocked by the
  DB execution gate.
- Request-log GC must avoid high-resource catch-up strategies such as rebuilding the whole
  `request_logs` table or generating a large WAL. Large backlogs are expected to catch up over
  repeated bounded windows.
- Retry logs may include operation, attempt, backoff, and final error context.
- Retry/exhaustion logs for this topic must expose a stable structured contract with
  `operation`, `request_path`, `request_kind`, `attempt|attempts`, `backoff_ms`, `elapsed_ms`,
  `retry_budget_ms`, `pending_batch_counts`, `oldest_pending_created_at`,
  `newest_pending_created_at`, and `billing_subject_kind`. The subject kind must remain one of
  `token|account|unknown`; raw billing subjects, request bodies, and token secrets must not appear.
- Request-path billing-subject serialization may rely on an in-process guard for the current
  single-process active node, instead of persisting a SQLite lock row for every request.
- Pending/charged billing truth may move to a dedicated `billing_ledger` table as long as pending
  replay, settlement idempotency, and admin/history compatibility remain intact.
- Request-derived dashboard/API-key/catalog rollups may be buffered in-memory and flushed in bounded
  windows, provided owner-facing reads flush or self-heal before returning stale results.
- The same bounded in-memory buffering model may also cover other request-derived observability
  counters such as auth-token activity and account request-rate buckets, provided billing truth
  stays synchronous and owner-facing reads flush before returning.
- Observability-heavy tables may live in a separate attached SQLite file when they are not required
  for synchronous billing truth. In that layout, `request_logs`, request-derived rollups, and other
  rebuildable observability tables are allowed to be eventually consistent and are not required to
  participate in HA outbox trigger replication; rebuild/export paths remain the recovery mechanism
  for those derived views.
- Startup must not force a large legacy single-DB `request_logs` table through an inline sidecar
  migration when that copy would exceed the startup budget. In that case, observability must remain
  attached to the core DB for startup and offline `request_logs_gc_once`, while smaller legacy DBs
  still migrate into the sibling sidecar automatically.
- Large legacy single-DB samples must now support an explicit offline cutover command:
  `observability_sidecar_migrate --db-path <path> [--batch-size 5000] [--dry-run] [--json]`.
  The command must require operators to stop the service first, must force the sibling sidecar
  attach path instead of reusing the large-legacy startup fallback, and must copy only
  `request_logs` into the sidecar while rebuilding or self-healing the derived observability tables
  in the new layout.
- The explicit sidecar migration contract must be resumable and idempotent. Re-running the command
  after a partial copy must preserve existing `observability.request_logs` rows by `id`, continue
  copying any missing `main.request_logs` rows in bounded batches, keep soft `request_log_id`
  references valid, rebuild all derived observability tables in the sibling sidecar layout, delete
  the legacy `main.request_logs` and legacy `main` rollup/bucket tables, then mark the corresponding
  meta keys complete and write the explicit cutover marker
  `observability_sidecar_explicit_cutover_v1_done`. The command must not report completion until a
  normal restart can attach the sibling sidecar without running a full derived-table rebuild, and
  the reopened startup path has been verified after the offline lock is released.
  A DB where the legacy tables are already gone but this explicit marker or the derived completion
  meta is missing is an interrupted cutover, not an `already_migrated` success; rerunning the tool
  must rebuild and mark the sidecar offline.
  Completion meta must be interpreted as complete only when the value is exactly `1`; present
  `0` values mean rebuild-needed and must not satisfy either the offline `already_migrated` check
  or the startup guard.
  Offline rebuild SQL for `api_key_usage_buckets` must preserve the `valuable_failure_429_count`
  metric, not just the aggregate success/error buckets.
- Startup must not run large sidecar derived-table rebuilds after an explicit cutover. If a cutover
  DB has the explicit cutover marker, `main.request_logs` removed, and sidecar `request_logs`
  present but the derived rebuild meta is incomplete, startup must fail fast with an
  operator-facing instruction to rerun `observability_sidecar_migrate`. Startup self-heal for
  legacy single-DB and small automatic sidecar migration remains outside this explicit-cutover
  fail-fast guard.
- Sidecar-aware schema self-heal paths must probe the attached `observability` schema explicitly.
  When both `main.request_logs` and `observability.request_logs` exist during migration or repair,
  column-existence checks must not accidentally read the wrong schema and issue duplicate `ALTER TABLE` statements.
- Shared-testbox validation for this path must remain isolated under `/srv/codex/**`, use a unique
  Compose project and run directory, avoid global Docker cleanup, and only remove this owner’s
  inactive workspaces or runs after confirming they are not referenced by any live
  `com.docker.compose.project`.
- The production cutover procedure is a short maintenance-window, local-host migration. Operators
  must stop the service, export a pre-cutover cold backup of the core DB to a rollback anchor, run
  the offline migration on the target host itself, restart and validate, and if anything fails
  restore the pre-cutover core DB and delete the sibling sidecar before bringing the service back.

## Acceptance

- Under a competing SQLite writer, acquiring a quota subject lock eventually succeeds after the
  writer releases within the existing wait budget.
- Under a competing SQLite writer that outlives SQLite's builtin busy timeout but releases before
  the bounded application retry budget, one billable `/mcp` tools/call request still returns `200`,
  records the billing row, and charges quota after the writer lock clears.
- Under a competing SQLite writer, scheduled job start retries rather than immediately returning
  `database is locked`.
- Under a competing SQLite writer, forward-proxy startup runtime snapshot persistence retries
  transient lock errors rather than failing the startup path immediately.
- With safely restorable persisted subscription runtime and a slow subscription endpoint, restart
  restores the local runtime and completes without waiting for the slow remote refresh.
- Without persisted subscription runtime, startup remains strict and waits for subscription
  readiness instead of reporting healthy from an empty proxy graph.
- Without persisted subscription runtime and with a small multi-URL subscription set, startup still
  waits for subscription readiness but does not spill into multiple 60-second timeout waves before
  strict health can turn green.
- When a serving role needs shared xray relay, `/health` returns non-`200` immediately until the
  runtime is actually ready even if the internal startup grace timer has not expired yet.
- In `active_standby`, `standby` / `recovery` still return `200 ok` on `/health` while runtime/xray
  prewarm is intentionally skipped.
- `server_pressure_buckets` rebuild no longer appears on the listener-before-ready critical path.
  After the process is already serving, one background rebuild repopulates historical buckets
  without making owner-facing pressure analysis return `500`.
- Manual or internal HA promotions that return the node to `provisional_master` / `full_master`
  still trigger the same post-ready `server_pressure_buckets` rebuild path as initial serving
  startup.
- If the node is demoted to `standby` / `recovery` before that rebuild finishes, the detached task
  exits without committing new `server_pressure_buckets` rows from the non-serving role.
- Under mock/local upstream startup, the image `HEALTHCHECK` becomes healthy only after the stricter
  serving `/health` is truly green and remains stable within the
  builder-compatible 20-second minimum-gate plus `start-period=20s/interval=5s/timeout=5s/retries=18`
  timing contract.
- With a large backlog of old request logs, one scheduler pass records bounded progress instead of
  running indefinitely; later catch-up passes eventually remove all rows older than the retention
  threshold.
- Public success metrics continue to wait for inflight request-stat flushes, but the month-tail
  fallback no longer emits the retained-log wide scan shape
  `WITH scoped_logs AS (...) FROM observability.request_logs` on the public metrics path.
- Overlapping DB-backed maintenance jobs in one process run through one persisted queue and one
  maintenance worker, so a second job is accepted/coalesced as `queued` instead of competing for the
  SQLite writer slot.
- With two queued remote-I/O maintenance jobs such as `quota_sync`, only one enters the active
  remote phase at a time; the next job remains `queued` until that remote slot clears, while
  non-remote maintenance work may still advance.
- Manual trigger API calls return a representative job id, job rows expose `trigger_source` plus
  `queued_at`, and duplicate active manual triggers coalesce onto the existing queued/running row
  instead of returning `db_job_execution_busy` or duplicate-running conflicts.
- After restart, any leftover `queued` or `running` maintenance rows are marked `abandoned` with a
  completion timestamp before new queue work is accepted.
- With an upstream `/usage` endpoint that hangs past the quota-sync timeout budget, manual and
  scheduler-triggered quota sync runs finish as `error`, leave no long-lived `running` row behind,
  and do not write `api_key_quota_sync_samples` or `api_keys.quota_*`.
- With a stale `quota_sync` or `quota_sync/hot` `running` row older than the configured timeout
  window, the next same-key claim abandons the stale row and starts a fresh run.
- After request-log retention cleanup creates enough freelist pages, DB compaction runs under the
  maintenance gate and reduces the main SQLite file size or reports why compaction was skipped.
- `db_compaction_once --json` reports threshold-based skip vs execution, and `--force` bypasses the
  threshold for a controlled maintenance window.
- `request_logs_gc_once --run-until-complete --json` removes old request logs and catalog rollups
  from a production-derived validation sample and reports `completed=true` when no old rows remain,
  while keeping WAL growth and CPU time bounded.
- A large legacy SQLite DB whose `request_logs` inline sidecar migration would exceed the startup
  budget still starts successfully, keeps `observability.request_logs` attached to the core file,
  and does not create a sibling sidecar file during that startup path.
- `observability_sidecar_migrate --dry-run` reports the core path, sibling sidecar path, sibling
  `observability-migrate.lock` path, whether the service lock is exclusively available, the
  best-effort SQLite write probe result, current attach target, legacy-table presence, fallback
  status, file sizes, and available disk space without creating or attaching a new sidecar file.
  That startup attach probe must match normal startup semantics for existing DBs; only missing or
  mistyped `--db-path` values are allowed to fail before any file creation.
  The write-probe field is best-effort only and must not make `--dry-run` fail on a read-only
  snapshot.
  A missing or mistyped `--db-path` must fail before creating either the core DB file or the
  sibling sidecar file or the sibling `observability-migrate.lock`.
- After `observability_sidecar_migrate` completes on a large legacy sample, the sibling
  `*-observability.db` exists, `main.request_logs` is gone, `observability.request_logs` preserves
  the original `id` coverage, child `request_log_id` / `source_request_log_id` references remain
  valid, and legacy `api_key_usage_buckets`, `dashboard_request_rollup_buckets`, and
  `request_log_catalog_rollups` are removed from `main`. Sidecar `api_key_usage_buckets`,
  `dashboard_request_rollup_buckets`, and `request_log_catalog_rollups` must already be rebuilt,
  their meta keys must be marked complete, the catalog retention meta must match the current
  retention setting, and a fresh normal startup reopen must succeed before the command reports
  `completed=true`.
- The explicit migration path must refuse to run while another process still holds the sibling
  `observability-migrate.lock`; success no longer relies on WAL-mode `BEGIN EXCLUSIVE` semantics to
  infer that the live service has stopped.
- Re-running `observability_sidecar_migrate` after a partial or finished copy must not duplicate
  sidecar `request_logs` rows and must report whether it resumed an earlier copy or found the DB
  already migrated.
- `request_logs_gc_once` can run against that same large legacy single-DB layout without forcing a
  startup-time sidecar split first.
- The current branch must be able to reproduce that explicit migration path on shared testbox from a
  production-shaped cold snapshot, then start normally and pass `/health`, `/api/version`,
  `/api/tavily/search`, `/mcp`, and request-log read-path smoke checks against the migrated data.
- The cutover runbook must be executable as written against the current deployment topology, with a
  local `docker compose` service, host-level `sqlite3`, a writable data mount, and the rollback
  anchor uploaded before the local migration mutates the core DB.
- Existing billing tests continue to prove locked billing subject stability, pending billing
  replay, and account/token quota attribution.
- Existing MCP/API routing behavior remains unchanged, including research result GET key pinning.

## References

- `docs/specs/s2vd2-upstream-credits-billing/SPEC.md`
- `docs/specs/cp8s9-upstream-agnostic-api-rebalance/SPEC.md`
- `docs/specs/34pgu-mcp-session-privacy-affinity-hardening/SPEC.md`
- `docs/specs/3tyrc-admin-dashboard-quota-charge-cards/SPEC.md`
- `docs/solutions/operations/sqlite-write-lock-contention.md`
