# Implementation

## Current Coverage

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
- Added `request_logs_gc_once` as a one-shot operational binary. It supports JSON output and
  `--run-until-complete` for deterministic low-resource validation against production-derived
  database samples.
- Added `request_logs_gc_stats` as a read-only operational binary for daily growth vs
  `cleaned_bodies` analysis directly from SQLite.
- Added local contention tests for quota subject lock acquisition and scheduled job start.
- Added local contention coverage for forward-proxy startup subscription refresh and runtime
  snapshot persistence.
- Added request-log GC coverage for old-row deletion, recent-row preservation, partial catch-up,
  catalog rollup cleanup, and transient SQLite write-lock retry.
- Added process-level DB job execution gate coverage that proves overlapping jobs serialize before
  entering their write windows.
- Added startup-order coverage for restored subscription runtime with a slow subscription endpoint,
  plus the strict no-runtime fallback where startup still waits for subscription readiness.

## Validation

- `cargo fmt --all`
- Targeted SQLite lock contention tests.
- Existing billing/MCP/quota-sync tests relevant to the touched paths.
- `cargo test`
- `cargo clippy -- -D warnings`
- Full `cargo test --locked --all-features`
- `cargo clippy -- -D warnings`

## Operations Notes

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
