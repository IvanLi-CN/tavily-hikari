# Implementation

## Current Coverage

- GC records the earliest unsealed or divergent day for source-backed reauditing. Pending day
  recovery prevents deletion until minute/daily rollups and the seal have been finalized.
- Productive bounded passes continue after one second; no-progress, pressure and error passes
  retain the five-minute defer. Scan-only progress counts only after its cursor is persisted.
- SQLite maintenance keeps a deferred admission ticket through the five-minute scheduler backoff,
  so unrelated coordinator activity cannot erase the aged turn before a pressure retry.
- Dashboard integrity admission keeps its five-minute backoff under foreground pressure, pool pressure,
  and recent SQLite contention; an occupied exclusive bulk-admission slot is rechecked after five seconds.
- Request-log GC keeps the five-minute defer for admission pressure, contention, no progress, and errors
  so cleanup resumes without repeatedly competing with foreground capacity.
- A productive request-log GC slice registers its next maintenance turn before releasing the current
  permit, keeping the one-second continuation ahead of later rolling integrity work.
- An unqueued initial hot page or newly closed hot segment gets its bounded page ahead of GC-blocking
  recovery. Once the hot page has a durable checkpoint, its continuation can yield to the blocking day
  without losing progress. Checkpointed hot continuation remains distinct from rolling hot pages, which
  a new hot segment may still preempt.
- GC-blocking day re-audits advance in bounded two-hour source ranges; range-wide committed,
  cancelled, and in-flight mutation fences prevent a later five-minute source bucket from being
  missed while ordinary history work retains five-minute ranges.
- Aged dashboard-integrity and request-log-GC scheduler turns use the scoped bounded-recovery admission path; it bypasses only the foreground-rate heuristic after the existing ticket age, while retaining the single bulk permit, pool-capacity checks, recent-contention defer, and coordinator fairness.
- Integrity completion checks only the target range for pending request-statistics work; pending and flushing dashboard buckets, repair barriers, and in-flight source mutations outside that range no longer block a valid historical checkpoint.
- The joint recovery entrypoint acquires exclusive service ownership, fixes a local-day target and source fence, persists two-hour checkpoints, seals the day from source counts, and resumes through explicit `complete`, `deferred`, `budget-exhausted`, or `failed` outcomes without stopping an active service.
- Structured recovery diagnostics record lane, target range/day, source fence, checkpoint defer or acceptance reason, GC progress, deleted rows, blocked day, and time since effective progress without exposing request bodies or credentials.
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

## Recovery Delivery Requirements

The recovery change is delivered in one PR spanning maintenance admission, dashboard integrity, local-day recovery, and request-log GC. Completion includes release verification and production recovery evidence; a merged PR or a passing synthetic load test alone does not establish recovery.

### Outcomes

- Restore the existing dashboard's closed historical ranges through source-backed verification.
  Keep genuine unverified ranges distinct from verified zero values and preserve the established
  statistics definitions.
- Complete source-backed re-audits and seals for the earliest GC-blocking local days, then remove
  only source rows and derived retention data that have expired under the configured policies.
- Sustain bounded recovery progress alongside supported live traffic whenever actual database
  capacity permits. Continuous new traffic must not silently turn finite historical debt into an
  indefinite wait.
- Preserve foreground billing, quota, request classification, and HTTP/MCP semantics. Recovery
  retains one local maintenance-bulk writer and bounded acquisition, read, and transaction work.
- Preserve accepted checkpoints, recovery debt, and claim fencing across contention, cancellation,
  process restart, and retries. A restart must not drop pending recovery work or publish a partial
  aggregate.
- Provide a controlled, resumable recovery entrypoint capable of integrity checking, local-day
  sealing, and retention cleanup. The existing GC-only entrypoint cannot by itself complete a
  missing seal or pending day re-audit.

### Evidence and Diagnostic Requirements

- Existing production observations demonstrate populated chart bucket values alongside
  `unverifiedBucketStarts`, repeated admission defers, and repeated GC results with
  `incomplete_blocked_integrity` / `reaudit_pending`. They do not establish deleted chart history.
- Admission logs distinguish foreground-rate pressure, pool pressure, another bulk owner, and
  recent SQLite contention. Inner integrity slices currently collapse different conditions into
  `state=deferred`; diagnostic evidence must distinguish scan continuation, read-budget exhaustion,
  relevant unflushed statistics, source-fence changes, and transient write contention.
- The current slice-completion path checks global request-statistics freshness. The fix must
  reproduce whether unrelated live statistics block a completed historical range before choosing
  a narrower freshness contract; relevant pending or in-flight changes must remain protected.
- Record lane, target range or day, fixed source boundary, accepted checkpoint before/after,
  sealed-day progress, expired-row deletions, defer reason, and time since effective progress.
  Keep secrets, request bodies, and personal data out of diagnostic evidence.
- `lastVerifiedAt` describes a successful slice, not full recovery. A moving-window bucket count
  and an unchanged earliest visible gap are insufficient to prove a historical scan is stationary.
  Evaluate a fixed historical debt set and each lane's accepted checkpoint instead.

### Acceptance Scenarios

| Scenario                                                                                     | Required result                                                                                                                                                        |
| -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sustained traffic above the ordinary maintenance-rate threshold with spare database capacity | Fixed historical debt and GC-blocking days make effective progress while foreground safety remains within the agreed budget.                                           |
| Continuous statistics for unrelated recent ranges                                            | A completed historical range can finalize without losing relevant pending or in-flight changes.                                                                        |
| Late insert, update, cancelled mutation, or concurrent flush in the target range             | Source fencing prevents stale verification, duplicate aggregation, partial publication, and premature deletion.                                                        |
| Dense history, slow reads, competing maintenance, or temporary writer lock                   | Work yields within its budgets, resumes accepted checkpoints, and reports the actual defer reason.                                                                     |
| Missing or inconsistent day seal with expired source rows                                    | Full-day source recovery completes before GC, and source/minute/daily/seal counts agree.                                                                               |
| Restart during scan, finalization, or GC continuation                                        | Durable work resumes with valid fences and no lost source rows or double-counted summaries.                                                                            |
| Controlled joint recovery entrypoint                                                         | Online ownership conflicts are rejected; interruption is resumable; final integrity and retention results are explicit.                                                |
| Production verification after release                                                        | Fixed closed chart ranges recover, the blocking day advances or clears, expired-row debt decreases, and foreground errors and latency remain within the agreed budget. |

Synthetic verification uses local/mock upstreams and generated fixtures with distinct old and live ranges. It must include sustained-traffic recovery rather than proving catch-up only after dropping traffic below the ordinary admission threshold. Production verification observes real traffic and uses the supported repair path; it does not inject a load test into the production upstream.

### Approved Recovery Policy

The owner accepted bounded online recovery for aged dashboard-integrity and request-log-GC work. When real database capacity permits, these recovery slices may receive a limited exception to the ordinary foreground-rate heuristic. The exception retains one maintenance-bulk writer, actual resource-pressure checks, bounded acquisition and transactions, and billing correctness.

The decision is recorded in [ADR 0008](../../adr/0008-bounded-request-statistics-recovery.md), a scoped amendment to ordinary maintenance admission. The implementation now provides the exception; sustained-traffic acceptance and production closeout remain separate evidence gates. Unrelated maintenance classes retain their existing safeguards.

### Approved Acceptance Budget

The owner accepted the following measurable boundaries. The normative contract lives in [the retention Spec](./SPEC.md#sustained-traffic-recovery-acceptance); these are required results, not evidence that the implementation already passes or an estimate for the production database.

| Measurement                                                | Approved acceptance boundary                                                                                                                                                                                 |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Effective progress with actual database capacity available | Each runnable recovery lane records at least one accepted checkpoint or completion within every five-minute observation window.                                                                              |
| Sustained-traffic fixture                                  | A generated 100,000-row fixture, including 5,000 expired source rows, completes its fixed target historical verification, blocking-day seal, and expired-row cleanup within 30 minutes at continuous 10 RPS. |
| Foreground P95                                             | The recovery-on result exceeds the same-build, same-load recovery-off baseline by at most 250ms.                                                                                                             |
| Foreground errors                                          | Mock-upstream requests have no failures; lock-pressure scenarios must introduce no recovery-caused request errors relative to their matched baseline.                                                        |
| Production closeout                                        | Observe fixed historical debt, chart restoration, seal/GC progress, and real foreground behavior separately; the fixture's 30-minute limit is not a whole-production-database promise.                       |

The fixture must preserve all 95,000 retained fixture source rows and billing truth, verify source/minute/daily/seal counts, and separate new live traffic from the fixed historical target. Actual capacity exhaustion must be reported explicitly rather than being silently counted as progress or a passing recovery interval. The existing load test's high-traffic assertions expect no expired-row deletion, so that expectation must change for the approved recovery policy.

### Recovery Entry Point Contract

- Online automatic recovery is the normal path; manual online triggers share its coordinator, durable work, and claim fences rather than creating a parallel writer.
- The controlled joint recovery command must establish exclusive ownership of its target databases and reject an active service or another recovery owner. It must not automatically stop a service or change deployment configuration.
- The command records its fixed target, accepted checkpoints, verification/seal state, deletion counts, and defer/error reasons. Cancellation or budget exhaustion preserves accepted progress for a subsequent invocation.
- Completion means that the fixed target's integrity checks and required day seals have finished and eligible expired rows have been cleaned. A deferred slice, successful process exit, or an exhausted retry budget cannot substitute for this result.
- Delivering this command does not authorize its execution against production. Production repair or a maintenance outage requires the owner's explicit operational authorization.

### Delivery Boundaries

The PR preserves retention settings, statistics definitions, billing and quota truth, and existing HTTP/MCP and CLI compatibility. It does not use a database migration, a larger pool, a general relaxation of maintenance admission, or marking unverified ranges as verified to satisfy acceptance. A UI redesign is outside this recovery change.

Release closeout must freeze an explicit production historical target, then verify chart restoration, source-backed day sealing, cleanup of that target's expired debt, and foreground behavior. Report continuing live traffic separately. Evidence that workers are running or that one slice succeeded is insufficient to declare the production recovery complete.

The policy and acceptance budgets are settled. The implementation is covered by this change; the 100,000-row sustained-traffic fixture and production closeout remain separate evidence gates.

## Validation

- This round passed `cargo fmt --all -- --check`, `cargo clippy --locked -j 2 --all-targets -- -D warnings`, the dashboard-integrity module (31 tests), request-log-GC filters (25 tests), SQLite runtime filters (70 tests), and the performance workflow contract suite (19 tests).
- The Agent VM contract was exercised for the required heavy-validation route, but its guest has no `cargo` or `rustc` in `PATH`; no toolchain was installed, and the VM was released after recording that exact blocker.
- The 100,000-row sustained 10 RPS fixture, full backend suite, frontend build, and production closeout were not run in this round; they remain CI or explicitly authorized operational evidence rather than inferred completion.
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

- Status: 实现完成（PR 收敛中；合成负载与生产收口待执行）
- Created: 2026-06-02
- Last: 2026-10-07
