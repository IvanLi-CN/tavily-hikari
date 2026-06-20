# Implementation

## Current Coverage

- Added a runtime logging contract for the online service surface based on `tracing` +
  `tracing-subscriber`. Runtime logging now defaults to JSON lines on stderr, exposes a documented
  `RUNTIME_LOG_FORMAT=text` fallback for grep-oriented workflows, and keeps `RUST_LOG` filtering
  intact. All runtime `SqliteConnectOptions` still enable `sqlx` slow statement logging at `250ms`.
- Added shared DB operation log helpers in `src/store/mod.rs`. They emit stable structured events
  with `component=db`, `event=operation_slow|operation_error`, `operation=...`,
  `elapsed_ms=...`, optional `context=...`, and optional `err=...`, so startup, request-path, and
  background DB phases can be filtered with one contract in both JSON and fallback text mode.
- Runtime DB operation logs intentionally do not include SQL bind values. The SQL-level
  `sqlx::query` slow-statement warnings keep the statement text/summary and timing, while the
  explicit phase-level helper keeps only operation/context metadata to avoid leaking secrets.
- Startup SQLite open / observability-attach probe / schema initialization now pass through that
  shared helper with a `1s` slow-operation threshold. Startup/shutdown/HA/forward-proxy paths also
  emit named runtime events (`component`, `event`, and path-specific fields), so operators can tell
  which startup DB phase stalled or failed without depending on ad hoc stderr text.
- Request-path DB work now has unified phase logs around LinuxDo OAuth upsert/profile refresh and
  pending billing settlement. That covers the same classes already seen in production after
  `2026-06-19 01:00 +08:00`: `oauth account upsert`, `apply_pending_billing_log`, and the
  downstream request-path billing failures that bubble up as `/api/tavily/search` or MCP proxy
  errors.
- Background scheduler/worker DB work now has unified phase logs around `scheduled job enqueue`
  and `request stats persist`, so `quota-sync-hot enqueue`, `scheduled job finish`, and request
  stats flush contention all have a stable DB operation prefix in addition to the existing
  owner-facing warning lines.
- Added a shared transient SQLite write retry helper for bounded backoff.
- `quota_subject_locks` acquire/refresh/release now retry transient SQLite busy/locked errors within
  the existing lock timeout or lease budget.
- Scheduled job start/finish writes now retry transient SQLite busy/locked errors before surfacing
  failure to background schedulers.
- OAuth account upsert/profile refresh wrapper calls now retry transient SQLite busy/locked errors
  before returning failures to LinuxDo login or daily sync flows.
- Forward-proxy startup now refreshes subscription-backed endpoints concurrently, syncs xray state
  from the restored snapshot, and retries runtime snapshot persistence when SQLite briefly denies
  the write slot.
- Forward-proxy startup now restores persisted subscription endpoints from `forward_proxy_runtime`
  before attempting remote subscription refresh when the current settings contain one unambiguous
  subscription source. If restored endpoints exist, startup skips the blocking remote refresh and
  proceeds to xray sync/runtime persistence; the existing maintenance scheduler performs
  subscription calibration after the service is running.
- LinuxDo system tag binding backfill now uses a single indexed startup precheck and only repairs
  mismatched rows before readiness. A background scheduler periodically refreshes the bindings and
  quota snapshots after the service is already listening.
- Request-log retention GC now runs in bounded batches for both `request_logs` and
  `request_log_catalog_rollups`, yields briefly between batches, and reports whether more catch-up
  work remains.
- Request-log GC unlinks old child-table references before deleting old `request_logs`, ensures
  supporting reference indexes, uses a lightweight CLI open path that skips full startup
  migrations, and disables SQLite secure-delete for the delete connection so retention cleanup does
  not spend extra CPU overwriting expired payload pages.
- Request-log GC temporarily removes and restores the catalog-rollup delete trigger inside each
  batch transaction. The old rollup buckets are deleted separately in bounded batches, avoiding a
  per-row trigger update for expired request payloads while keeping normal request log writes and
  updates covered by the trigger set.
- The daily `request_logs_gc` scheduler now runs one bounded cleanup pass per
  `scheduled_jobs` row. If backlog remains, the scheduler sleeps for the catch-up interval and
  claims a fresh job for the next pass instead of keeping one long-running `running` row open.
- Scheduled jobs now distinguish `trigger_source` from `job_type`, use an atomic claim path to avoid
  duplicate active work, and expose manual trigger entrypoints for maintenance/admin jobs.
- `quota_sync` now uses a hard `/usage` timeout, a bounded job runtime budget, and claim-time stale
  running row reclamation for `quota_sync` / `quota_sync/hot`, so hung syncs self-heal instead of
  blocking future runs until a restart.
- Request-log GC catch-up now uses smaller scheduler windows with a faster recheck cadence so a
  large body-cleanup backlog can make daily progress without one pass holding the SQLite writer
  slot for long.
- DB maintenance now records size/freelist telemetry and can compact the SQLite file through a
  dedicated job, with automatic threshold-based triggering and manual admin triggering.
- Added `db_compaction_once` as an offline operational binary. It reuses the same threshold gate as
  the scheduler, supports `--force`, and avoids depending on the in-process admin trigger when the
  DB execution gate is busy.
- DB-backed scheduled and manual jobs now pass through a process-wide execution gate before their
  SQLite write windows. The gate covers retention GC, compaction, quota sync, rollups, session GC,
  backoff GC, auth-token log GC, and LinuxDo sync/refresh jobs while preserving the existing
  scheduled-job claim/finish semantics.
- Request-log GC catch-up releases the DB job execution gate between cleanup windows so the
  scheduler delay does not block other DB-backed jobs.
- `scheduled_jobs` now persists queue admission explicitly: `queued_at` is stored for every
  maintenance row, `started_at` is nullable until the worker actually starts execution, and job list
  APIs order by `COALESCE(started_at, queued_at)` so queued work remains visible.
- Added queue-side primitives on top of `scheduled_jobs`: enqueue/coalesce, dequeue, mark-running,
  lookup-by-id, and abandon-all-active semantics.
- Scheduler loops now enqueue DB-backed maintenance work instead of trying to claim-and-run inline.
  One in-process maintenance worker consumes queued jobs, preserves manual-first priority, and
  reuses the existing per-job execution logic.
- Remote-I/O maintenance families (`quota_sync*`, LinuxDo user sync, GEO refresh) now share one
  worker-scoped remote slot. That keeps the queue from fan-out marking a burst of `/usage` or GEO
  jobs as `running` all at once, while still allowing DB-only jobs such as `request_logs_gc` to
  advance during a pending remote phase.
- Coalesced active jobs now promote `trigger_source` even while the representative row is already
  `running`, so a later manual trigger is visible in both the returned trigger response and the
  persisted job row instead of being silently hidden behind the original scheduler source.
- Same-priority duplicate manual triggers now take a read-only coalesce fast path before attempting
  a write transaction. That keeps `POST /api/jobs/trigger` from returning transient SQLite
  `database is locked` errors when a bounded GC slice is already running and the request only needs
  to attach to the existing representative row.
- Request-log GC now requeues itself through the persisted queue when a bounded pass reports
  `completed=false`, so backlog catch-up no longer depends on one scheduler loop keeping a running
  row alive.
- Manual `POST /api/jobs/trigger` now accepts/coalesces queue work and returns the representative
  `job_id` instead of exposing `db_job_execution_busy`. The response also exposes representative
  queue hints (`status`, `coalesced`, `promoted`) so the admin UI can distinguish “newly queued”
  from “already running/queued”. Manual key quota sync still waits for a result, but it now does so
  by enqueueing `quota_sync` and polling the representative job row to a terminal state.
- `forward_proxy_geo_refresh` now follows the same split-phase model as quota sync and LinuxDo
  sync: remote trace/GEO discovery happens outside the DB execution gate, candidate persistence and
  `scheduled_jobs` completion happen inside a short DB window, and the worker may continue with
  other queued non-remote jobs while the single remote-I/O slot is in flight.
- Online billing-subject serialization no longer uses `quota_subject_locks` as the request-path
  mutex. The hot path now uses an in-process subject guard, keeping fail-closed billing semantics
  while removing acquire/refresh/release writes for every billable request.
- Added `billing_ledger` as the synchronous billing truth source. Pending/charged state,
  `billing_subject`, `business_credits`, request linkage, and settlement metadata are backfilled
  from `auth_token_logs` at startup and then maintained in `billing_ledger` on every new pending
  billing record and settlement.
- Pending-billing readers and rollups that previously scanned `auth_token_logs.billing_state` now
  read from `billing_ledger`, while `auth_token_logs.billing_state` is still mirrored for backward
  compatibility with existing admin/history surfaces.
- HA baseline capture now includes `billing_ledger`, so recovery/export paths preserve the new
  billing truth table.
- Added an in-process HA state coalescer. `persist_ha_node_state` and
  `persist_ha_sync_watermark` now merge writes inside a `1s / 100 keys` window, and owner-facing
  reads that require immediate consistency explicitly flush before returning.
- Added a request-stats coalescer for request-derived rollups. Hot-path `request_logs` inserts now
  synchronously write only the `request_logs` row itself, then enqueue:
  - dashboard request rollup deltas,
  - API-key usage bucket deltas,
  - request-log catalog rollup deltas,
  - auth-token `total_requests/last_used_at` deltas,
  - account request-rate (`account_usage_rollup_buckets` five-minute) deltas.
- Request-derived rollups now flush in one background batcher (`1s / 100 pending keys`) instead of
  issuing synchronous rollup writes per request. Owner-facing summary, key-metrics, and request-log
  catalog reads flush that coalescer before reading.
- Request observability tables now attach through a per-core sibling sidecar SQLite file
  (`<core-stem>-observability.db`) in the new layout. `request_logs`, `api_key_usage_buckets`,
  `dashboard_request_rollup_buckets`, and `request_log_catalog_rollups` are created in that
  sidecar for the steady-state layout. Smaller legacy single-DB SQLite files still migrate
  `request_logs` into the sidecar during startup, but large legacy DBs now stay on a temporary
  single-DB compatibility path when the inline copy would exceed the startup budget. In that mode,
  `observability` is attached back to the core file for startup and offline `request_logs_gc_once`
  until operators run the explicit offline sidecar cutover. That compatibility path must still keep
  the normal SQLite pool capacity; collapsing the pool to one connection makes `/api/summary`
  flushes and early scheduler enqueue paths fight for the same slot and can leave owner-facing
  reads returning transient 500s after `/health` is already green.
- Added `observability_sidecar_migrate` as an offline operator binary for that explicit cutover.
  The command:
  - always derives the sibling `*-observability.db` path from the core DB path,
  - probes whether normal startup would still be on the large-legacy compatibility path while
    preserving the normal attach decision for existing DBs,
  - supports `--dry-run` / `--json` reporting without mutating or creating the sidecar,
  - rejects missing or mistyped `--db-path` values before creating either the core DB file or the
    sibling sidecar file,
  - treats the write-probe result as best-effort metadata so read-only snapshots can still be
    inspected in dry-run mode,
  - requires the service to be stopped before a real migration by probing `BEGIN EXCLUSIVE`,
  - forces the sibling sidecar attach target instead of reusing the startup fallback,
  - copies only `main.request_logs` into `observability.request_logs` in `id` order with bounded
    batches and `NOT EXISTS` dedupe so reruns can resume partial copies safely,
  - rebuilds request-log soft-reference tables before removing `main.request_logs`,
  - validates preserved child references (`billing_ledger`, `api_key_maintenance_records`,
    `api_key_transient_backoffs`, and any existing `auth_token_logs.request_log_id`),
  - temporarily hides legacy `main` observability tables while reusing the existing sidecar layout
    rebuild routines, so unqualified rebuild SQL reads and writes the attached `observability`
    schema instead of the legacy `main` tables,
  - rebuilds sidecar `api_key_usage_buckets`, `dashboard_request_rollup_buckets`, and
    `request_log_catalog_rollups` before deleting any hidden legacy table,
  - drops the hidden legacy `main` observability tables before writing completion meta, so a
    crash cannot leave a false completed state with temporary legacy tables still present,
  - marks `api_key_usage_buckets_v1_done`,
    `api_key_usage_buckets_request_value_v2_done`,
    `dashboard_request_rollup_buckets_v1_done`,
    `request_log_catalog_rollup_v1_done`, and
    `request_log_catalog_rollup_v1_retention_days` complete, then writes
    `observability_sidecar_explicit_cutover_v1_done` before reporting success,
  - reports the derived rebuild booleans, completion-meta booleans,
    `startup_rebuild_required`, and `derived_rebuild_elapsed_ms` in JSON output.
- Startup now treats explicit large sidecar cutover as a completed offline operation. If
  `observability_sidecar_explicit_cutover_v1_done` is present, `main.request_logs` is gone, sidecar
  `request_logs` contains rows, and the derived rebuild meta is incomplete, startup fails fast with
  an instruction to rerun `observability_sidecar_migrate` instead of awaiting full derived-table
  rebuilds before the HTTP listener is ready. Legacy single-DB startup and small automatic sidecar
  migration remain allowed to use their bounded startup self-heal paths.
- Server/admin test helpers now mirror that sidecar layout instead of opening only the core DB
  file. SQLite schema assertions for `request_logs` and the other observability tables now probe
  the attached schema explicitly, which keeps migration and admin-route coverage aligned with the
  production attached-database layout even when both `main` and `observability` temporarily expose
  similarly named tables during legacy migration/repair paths.
- Auth-token list/admin-token/user-token reads and admin rate-5m usage series now also flush the
  request-stats coalescer before reading, so owner-facing token activity and request-rate charts
  stay current without putting those derived writes back on the request hot path.
- `request_log_catalog_rollups` no longer relies on per-request SQLite triggers for normal hot-path
  inserts. Owner-facing catalog reads keep a narrow rebuild-on-read fallback for legacy/manual SQL
  mutations so admin surfaces can self-heal if rollups were emptied or bypassed.
- `/api/logs` page reads now also ensure request-log catalog rollups are available before reading
  totals/facets, so an empty or bypassed rollup table does not make the admin logs surface show an
  empty total while visible `request_logs` rows still exist in the observability sidecar.
- The request-log GC path no longer drops/recreates the catalog delete trigger per batch, because
  the catalog rollup table is now maintained by the request-stats coalescer plus explicit rebuilds.
- SQLite attached-database trigger limits mean observability sidecar tables no longer participate
  in HA outbox trigger replication. Those tables are now treated as rebuildable/eventually
  consistent owner-facing views; the HA baseline remains focused on core truth tables such as
  `billing_ledger`, bindings, quota state, and control-plane facts.
- Auth-token log page/detail queries that join `billing_ledger` now qualify `auth_token_logs.*`
  columns explicitly and avoid unnecessary billing joins in count/facet queries. That removes the
  `ambiguous column name` regressions that appeared once synchronous billing truth moved out of
  `auth_token_logs` and the admin token-log surfaces began reading mixed ledger/history data.
- Service startup now abandons leftover `queued` and `running` maintenance rows from the previous
  process lifetime before starting the new worker.
- Added `request_logs_gc_once` as a one-shot operational binary. It supports JSON output and
  `--run-until-complete` for deterministic low-resource validation against production-derived
  database samples.
- Added `request_logs_gc_stats` as a read-only operational binary for daily growth vs
  `cleaned_bodies` analysis directly from SQLite.
- Added local contention tests for quota subject lock acquisition and scheduled job start.
- Added queue lifecycle tests for coalesced enqueue promotion, delayed `started_at` materialization,
  and abandon-all-active restart cleanup semantics.
- Added coverage for manual trigger coalescing on an already running representative row, including
  the HTTP response hints returned by `/api/jobs/trigger`.
- Added regression coverage for duplicate manual trigger coalescing while another connection holds
  the SQLite writer slot, both at the store layer and through the owner-facing HTTP trigger route.
- Added worker orchestration coverage that proves only one remote-I/O maintenance job enters
  `running` at a time and that `request_logs_gc` can still complete while a quota-sync remote phase
  is waiting on `/usage`.
- Added local contention coverage for forward-proxy startup subscription refresh and runtime
  snapshot persistence.
- Added request-log GC coverage for old-row deletion, recent-row preservation, partial catch-up,
  catalog rollup cleanup, and transient SQLite write-lock retry.
- Added explicit sidecar-migration coverage for:
  - large legacy offline migration into the sibling sidecar,
  - dry-run reporting without sidecar creation,
  - idempotent reruns after a finished cutover,
  - resuming partial copies when the sidecar already contains a subset of `request_logs` ids,
  - rejecting startup when a cutover sidecar has historical rows but derived completion meta is
    missing,
  - preserving the large-legacy startup compatibility path and standalone `request_logs_gc_once`
    behavior until the explicit cutover is run.
- Explicit sidecar migration completion now requires a fresh normal startup reopen after the
  offline lock is released. The migration CLI/report surface gained
  `startup_reopen_verified`, and the final `completed=true` contract now means “offline rebuild
  succeeded and a normal startup reopen succeeded,” not just “the offline sidecar tables were
  rebuilt.”
- Added request-stats coverage proving summary/key-metric reads flush pending coalesced deltas
  before returning.
- Added request-stats coverage proving auth-token activity reads and admin rate-5m usage-series
  reads flush pending coalesced deltas before returning.
- Added request-log catalog coverage proving catalog reads still self-heal after direct SQL
  `request_logs` mutations and rollup rebuild scenarios.
- Added server-level regression coverage proving the admin logs page still returns rows/totals from
  the sidecar-backed `request_logs` layout and that token-log detail/page reads remain stable after
  the `billing_ledger` join split.
- Added process-level DB job execution gate coverage that proves overlapping jobs serialize before
  entering their write windows.
- Added startup-order coverage for restored subscription runtime with a slow subscription endpoint,
  plus the strict no-runtime fallback where startup still waits for subscription readiness.

## Validation

- `cargo check -q`
- `cargo test -q db_operation_log_format_includes_operation_context_and_error`
- `cargo test -q sqlite_runtime_log_context_is_stable_and_grep_friendly`
- `cargo test -q scheduled_job_enqueue`
- `cargo test -q linuxdo_oauth_upsert_skips_missing_tags_for_new_accounts_and_recovers_after_reseed`
- `cargo test -q pending_billing_claim_miss_is_retry_later_until_next_replay`
- `cargo fmt --all`
- `cargo test observability_sidecar_migrate_moves_large_legacy_request_logs_offline -- --nocapture`
- `cargo test observability_sidecar_migrate_resumes_copy_from_preseeded_sidecar_gaps -- --nocapture`
- `cargo test observability_sidecar -- --nocapture`
- `cargo test large_legacy_single_db_request_logs_stay_in_core_database_for_startup -- --nocapture`
- `cargo test standalone_request_logs_gc_uses_large_legacy_single_db_layout -- --nocapture`
- Targeted SQLite lock contention tests.
- Existing billing/MCP/quota-sync tests relevant to the touched paths.
- `cargo test --lib scheduled_job_enqueue_coalesces_running_job_and_promotes_manual_source -- --nocapture`
- `cargo test --bin tavily-hikari manual_jobs_trigger_coalesces_running_job_and_returns_representative_row -- --nocapture`
- `cargo test --bin tavily-hikari forward_proxy_geo_refresh_job_records_scheduled_job_and_skips_direct -- --nocapture`
- `cargo test --lib tests::request_log_catalog_rollup_feeds_catalog_and_legacy_page -- --nocapture`
- `cargo test --bin tavily-hikari admin_logs_endpoint_returns_unfiltered_and_filtered_pages -- --nocapture`
- `cargo test --bin tavily-hikari token_log_details_return_linked_bodies_and_page_results_keep_null_payloads -- --nocapture`
- `cd web && bun test ./src/api.test.ts ./src/admin/AdminPages.stories.test.ts`
- `cd web && bun run build`
- `cargo test`
- `cargo clippy -- -D warnings`
- Full `cargo test --locked --all-features`
- `cargo clippy -- -D warnings`
- Shared testbox isolated run:
  - remote workspace `/srv/codex/workspaces/ivan/tavily-hikari__7aa37deb`
  - remote run `/srv/codex/workspaces/ivan/tavily-hikari__7aa37deb/runs/20260617_035715_7dfaaa12_sidecar`
  - compose project `codex_tavily-hikari__7aa37deb_20260617_035715_7dfaaa12_sidecar`
  - migration CLI evidence:
    `observability_sidecar_migrate: dry_run=false completed=true offline_lock=true sqlite_write_probe_ok=true copied_request_logs=2 batches=2 already_migrated=false resumed_copy=false`
  - smoke evidence:
    `{\"backend\":\"0.2.0\",\"tokenId\":\"UED8\",\"logPaths\":[\"/api/tavily/search\",\"/mcp\"],\"sidecarRowCount\":4}`

## Operations Notes

- The `2026-06-19 01:00 +08:00` to `2026-06-19 10:18 +08:00` 101 sample that motivated this pass
  showed all three runtime responsibility surfaces at once:
  - startup: `forward-proxy startup: sqlite initialized in 38906ms`
  - background queueing/worker writes: `quota-sync-hot: enqueue job error`, `scheduled job finish`,
    `request stats persist warning`
  - request-path/user writes: `upsert linuxdo oauth account error`,
    `oauth account upsert: transient sqlite write error`, `apply_pending_billing_log`, and
    `/api/tavily/search` proxy failures bubbling a `database is locked`
- The new runtime DB logging contract is designed to map those same symptoms onto stable fields:
  - `component=db event=operation_slow operation="sqlite startup" ...`
  - `component=db event=operation_error operation="scheduled job enqueue" ...`
  - `component=db event=operation_error operation="request stats persist" ...`
  - `component=db event=operation_error|operation_slow operation="oauth account upsert" ...`
  - `component=db event=operation_error|operation_slow operation="apply_pending_billing_log" ...`
  - `sqlx::query` warn lines for statements slower than `250ms`
- Production baseline was read-only: container healthy, version `0.46.2`, database `8.3G`, WAL
  `235M`, and the most recent one-hour lock sample only showed LinuxDo OAuth upsert contention.
- Later production inspection found a `20G` database where startup spent roughly `78s` inside
  SQLite initialization; the repeated LinuxDo tag binding refresh over all OAuth accounts was a
  primary avoidable startup cost, so periodic refresh now runs outside the readiness path.
- A later request-log body-retention backlog produced a much larger main DB file even after row
  retention was no longer the primary issue. Deleting or nulling payloads alone leaves free pages in
  SQLite, so file-size convergence is handled as a separate compaction job after retention work.
- If production inspection shows long-lived `scheduled_jobs.running` rows from an older process,
  restart the service under the current stale-job cleanup path before relying on manual retriggers.
  The in-process execution gate prevents new same-process overlap; it does not rewrite stale rows
  while the old process is still considered active.
- The SQLite pool now defaults to `max_connections=3` instead of `5`, preserving WAL mode while
  reducing writer contention on the single-file production database.
- The recommended release path for this class of issue is: deploy code, perform one controlled
  restart, verify `/health`, inspect `scheduled_jobs` / `database is locked` logs, continue
  `request_logs_gc_once` if backlog remains, and only invoke `db_compaction_once` when reclaimable
  space crosses the threshold or operators explicitly force a maintenance window.
- The large-legacy sidecar cutover is now a separate operator runbook instead of an automatic
  startup side effect. The validated flow is:
  - on codex-testbox, seed a production-shaped legacy core DB snapshot, run the migration
    container first, then start the current-branch service image against the migrated files and
    verify `/health`, `/api/version`, `/api/tavily/search`, `/mcp`, and request-log reads;
  - on 101, stop `tavily-hikari`, export the pre-cutover core DB as the rollback anchor to
    codex-testbox, run `observability_sidecar_migrate` locally against
    `/srv/app/data/tavily_proxy.db`, restart, and validate the same request-log and MCP surfaces;
  - if validation fails, stop the service, restore the pre-cutover core DB, delete the sibling
    `tavily_proxy-observability.db`, and then restart.
- The current 101 deployment facts that this runbook assumes were rechecked on 2026-06-17:
  - compose working directory `/home/ivan/srv/ai`
  - running service name `tavily-hikari` in compose project `ai`
  - container mount `/srv/app/data -> /var/lib/docker/volumes/ai-tavily-hikari-data/_data`
  - host free space about `56G`
  - `sqlite3` available at `/usr/bin/sqlite3`
- Recommended 101 cutover sequence:
  - stop the service:
    `ssh 192.168.31.11 'cd /home/ivan/srv/ai && docker compose stop tavily-hikari'`
  - create a consistent cold backup on 101:
    `ssh 192.168.31.11 'sqlite3 /var/lib/docker/volumes/ai-tavily-hikari-data/_data/tavily_proxy.db \".backup /tmp/tavily_proxy.pre_sidecar.db\" && gzip -f /tmp/tavily_proxy.pre_sidecar.db'`
  - upload the rollback anchor to codex-testbox:
    `ssh 192.168.31.11 'cat /tmp/tavily_proxy.pre_sidecar.db.gz' | ssh codex-testbox 'cat > /srv/codex/workspaces/ivan/tavily-hikari__7aa37deb/backups/tavily_proxy.pre_sidecar.db.gz'`
  - run the explicit migration on 101:
    `ssh 192.168.31.11 'cd /home/ivan/srv/ai && docker compose run --rm -T --entrypoint observability_sidecar_migrate tavily-hikari --db-path /srv/app/data/tavily_proxy.db --batch-size 5000 --json'`
  - restart and validate:
    `ssh 192.168.31.11 'cd /home/ivan/srv/ai && docker compose up -d tavily-hikari && docker compose ps tavily-hikari'`
  - rollback if validation fails:
    `ssh 192.168.31.11 'cd /home/ivan/srv/ai && docker compose stop tavily-hikari && gzip -dc /tmp/tavily_proxy.pre_sidecar.db.gz > /var/lib/docker/volumes/ai-tavily-hikari-data/_data/tavily_proxy.db && rm -f /var/lib/docker/volumes/ai-tavily-hikari-data/_data/tavily_proxy-observability.db && docker compose up -d tavily-hikari'`
- The stop-service proof for this runbook is the sibling `tavily_proxy-observability-migrate.lock`:
  live server and GC paths hold it shared, while the explicit migration command must acquire it
  exclusively before mutating the legacy core DB. The remaining SQLite write probe stays in the
  JSON/plain report as a diagnostic signal, not as the primary proof that the live service exited.
- The explicit migration entrypoint rejects a missing or mistyped `--db-path` before it probes
  disk space, opens SQLite, or creates the sibling `observability-migrate.lock`, so typoed paths
  do not leave behind empty core, sidecar, or lock files.
