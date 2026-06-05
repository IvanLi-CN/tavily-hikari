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
- The daily `request_logs_gc` scheduler records partial progress in `scheduled_jobs` and waits
  before continuing catch-up instead of keeping one long-running cleanup job open.
- Scheduled jobs now distinguish `trigger_source` from `job_type`, use an atomic claim path to avoid
  duplicate active work, and expose manual trigger entrypoints for maintenance/admin jobs.
- Request-log GC catch-up now uses smaller scheduler windows and slower retry cadence so a large
  body-cleanup backlog does not repeatedly consume the SQLite writer slot.
- DB maintenance now records size/freelist telemetry and can compact the SQLite file through a
  dedicated job, with automatic threshold-based triggering and manual admin triggering.
- DB-backed scheduled and manual jobs now pass through a process-wide execution gate before their
  SQLite write windows. The gate covers retention GC, compaction, quota sync, rollups, session GC,
  backoff GC, auth-token log GC, and LinuxDo sync/refresh jobs while preserving the existing
  scheduled-job claim/finish semantics.
- Request-log GC catch-up releases the DB job execution gate between cleanup windows so the
  scheduler delay does not block other DB-backed jobs.
- Added `request_logs_gc_once` as a one-shot operational binary. It supports JSON output and
  `--run-until-complete` for deterministic low-resource validation against production-derived
  database samples.
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
- The implementation does not perform production data mutation and does not alter pool size.
