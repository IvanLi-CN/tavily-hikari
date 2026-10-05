# Implementation

## Current Coverage

- GC records the earliest unsealed or divergent day for source-backed reauditing. Pending day
  recovery prevents deletion until minute/daily rollups and the seal have been finalized.
- Productive bounded passes continue after one second; no-progress, pressure and error passes
  retain the five-minute defer. Scan-only progress counts only after its cursor is persisted.
- SQLite maintenance keeps a deferred admission ticket through the five-minute scheduler backoff,
  so unrelated coordinator activity cannot erase the aged turn before a pressure retry.
- Dashboard integrity admission keeps its five-minute backoff only for sustained foreground pressure;
  transient pool pressure and recent contention retry after thirty seconds so GC-blocking day audits
  can continue while the admission guard still protects foreground capacity.
- Request-log GC keeps the five-minute defer for foreground pressure, recent contention, no progress,
  and errors; a transient pool-capacity defer retries after thirty seconds so productive cleanup can
  resume between bounded dashboard-maintenance turns.
- A productive request-log GC slice registers its next maintenance turn before releasing the current
  permit, keeping the one-second continuation ahead of later rolling integrity work.
- A newly created hot segment gets its first bounded page ahead of historical work. Once its fence
  advances, a pending GC-blocking day can take over without waiting for the whole hot cursor backlog
  to drain. Checkpointed hot continuation remains distinct from rolling hot pages, which a new hot
  segment may still preempt.
- GC-blocking day re-audits advance in bounded two-hour source ranges; range-wide committed,
  cancelled, and in-flight mutation fences prevent a later five-minute source bucket from being
  missed while ordinary history work retains five-minute ranges.
- Scan progress compares the retained cursor before and after the entire pass. A terminal page
  that clears the cursor, including repeated bodyless scans during an integrity block, does not
  qualify for one-second continuation. The internal progress flag is omitted from serialized reports.
- Optional blocked-day diagnostics preserve the existing API and CLI fields. Offline legacy
  single-database GC initializes the small recovery queue without moving production source data.
- HTTP user/token primary-affinity selection now evaluates `http_global` cooldown in the existing
  specific-key eligibility query, preserving cooldown boundaries and fallback behavior without a
  separate `ScheduledJobControl` read.

- Backend settings now expose `requestLogRetention` with defaults, range validation, and save-time
  clamp to `maxLogRetentionDays`.
- `request_logs` stores body byte counts, SHA-256 hashes, cleanup reason, and cleanup timestamp;
  policy-zero and expired bodies clear only BLOB columns.
- Request body retention policy classifies business, non-business, and non-success requests, with
  `mcp:batch` treated as business when any contained method is business.
- `request_logs.counts_business_quota` preserves `mcp:batch` billing/operational classification
  after policy-zero or expired body cleanup.
- User debug-sharing consent is persisted on `users` and exposed through the user dashboard and
  `PUT /api/user/debug-info-sharing`; settings and debug-sharing reads are cached in `KeyStore` to
  keep request logging off repeated SQLite meta/debug lookups.
- Existing bounded `request_logs_gc` now cleans expired bodies before deleting rows past the
  configured maximum log retention window, and re-evaluates high-frequency usage so that expensive
  usage-bucket scans stay out of the per-request logging path.
- Admin settings UI includes the high-frequency threshold slider and nonlinear day sliders for
  global, high-frequency, and debug-sharing profiles.
- User console shows the shared debug information toggle, and request detail views summarize
  cleaned body metadata when full bodies are no longer retained.

## Validation

- GC regression coverage includes legacy single-database initialization and source preservation;
  atomic blocking-day registration avoids `BEGIN IMMEDIATE` self-contention between two aliases
  of the same file. Scheduled continuation tests cover progress, no progress and claim-fenced restart.
- Affinity regressions cover global-cooldown rebinding, unrelated-scope isolation, and successful
  cooldown selection after all pooled connections remain occupied beyond 100ms.
- `scripts/gc_recovery_load.py` provides a private 100,000-row mock-upstream probe. Its manual
  GitHub Actions suite runs 10 business requests per second for 30 minutes, then 0.1 requests per
  second for up to 30 minutes to verify bounded deletion and seal recovery. It requires at least
  5,000 expired rows to be deleted from a 5,000-row expired blocking day while preserving 95,000
  rows inside the retention window. The suite also runs maintenance-ticket backoff, hot-page-yield,
  wide-range mutation-fence, and full-day seal-recovery regressions and records integrity cursors,
  blocking work items, priorities, and
  scheduler messages in failure evidence. The high-phase result
  is checkpointed independently so a slow recovery does not hide latency evidence. Only JSON
  acceptance evidence is uploaded; fixture databases and service logs stay on the GitHub-hosted
  runner.

- `cargo clippy -- -D warnings`
- `cargo test request_log_retention -- --nocapture`
- `cargo test request_logs_gc -- --nocapture`
- `cargo test request_log_policy_preserves_batch_non_business_classification_without_body -- --nocapture`
- `cd web && bun run build`
- Storybook visual evidence captured for Admin settings and user console debug-sharing states.

## Status

- Status: 进行中（快车道）
- Created: 2026-06-02
- Last: 2026-10-05
