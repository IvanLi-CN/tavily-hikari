# ADR 0002: Reconciliation Uses Scoped SQLite and Remote Admission

## Status

Accepted

## Context

Historical reconciliation projection reads can run long enough to delay the next durable work
boundary. Cancelling the async task that awaits such a read is not a safe query budget: the native
SQLite connection can still have an active statement or transaction when the caller gives up.

The former remote-I/O scheduler slot also spanned local candidate preparation, projection,
hydration, finalization, and research bookkeeping. That made a single upstream slot look busy when
no HTTP request was in flight, and allowed recurring automatic work to postpone an eligible
reconciliation attempt indefinitely.

Process and cgroup write counters are useful pressure signals, but describe an entire process or
cgroup. They cannot attribute write amplification to one SQLite statement.

## Decision

- Each reconciliation preparation source `SELECT` opens a fresh read snapshot through
  `ReconciliationReadSession`. Candidate recent/backlog lanes, candidate and billed-credit hydrate,
  Research candidates, and historical projection pages each use a connection-local SQLite progress
  handler. It checks a fixed 250ms deadline every 1,000 virtual-machine operations and maps its own
  interrupt to a typed `projection_read_budget` deferred outcome. The complete local preparation
  session remains subject to its separate run budget.
- The billed-credit hydrate is a pre-request availability gate. After an observation, settlement
  reads the current ledger through a separately bounded finalization connection so a charge written
  during HTTP is reflected without allowing a post-request source-read deadline to replace durable
  finalization.
- The handler is removed before the read connection is restored to the pool. If removal cannot be
  confirmed, the physical connection is closed instead of being reused.
- A read-budget defer stops preparation at that statement boundary. It starts no projection merge
  transaction, advances no cursor, starts no later preparation read or remote request, and existing
  claim-fenced finish-and-enqueue logic records one delayed continuation after 30 seconds.
- `RemoteAttemptAdmissionController` owns one process-local actual-request slot. A lease starts at
  the outbound HTTP boundary and ends after the response or transport error is read; local SQLite
  preparation and durable finalization never hold it.
- Manual remote work keeps dispatch priority. Once an automatic main reconciliation representative
  or Research drain has been eligible for 120 seconds, it competes for the next non-manual remote
  turn. Main reconciliation is ordered by `available_at`; Research is ordered by its durable
  `queued_at` debt anchor so a defer cannot reset its wait; main wins an exact tie. The turn is
  consumed only when HTTP starts, not by local reads, cooldown selection, cancellation, stale
  claims, or a no-request defer. Research preserves its `scheduled_jobs.queued_at` fairness anchor across foreground,
  lease, read-budget, and control defers, while an accepted poll or Key cooldown starts a new
  interval. After an accepted five-second `remote_lease` continuation, the controller retains that
  aged Research reservation until its resumed run begins an actual HTTP request; ordinary automatic
  remote jobs may still prepare locally but cannot reclaim the released request lease.
- `sqlite_workload_window` records connection-local `CACHE_WRITE` page deltas and cooperative-read
  calls, elapsed time, deadlines, defers, and discarded connections per reconciliation read kind.
  At the same low-frequency window boundary it may sample only configured core/observability DB and
  WAL file metadata. Process and cgroup write bytes remain explicitly labelled aggregate values.
- Scoped evidence now shows that the Research candidate aggregate is the remaining source-read
  bottleneck. Migration v21 therefore adds only a local research scan state and a covering
  `(terminal_at, next_poll_at, key_id, request_id)` index. The selector reads an indexed keyset
  page, hydrates that bounded page, and accepts its cursor only after a claim-fenced run boundary.
  The primary candidate SQL remains unchanged; any rewrite there still requires separate evidence.
- Due Research timing is governed by ADR 0004. Its independent durable drain uses the same
  request-scoped remote admission boundary without extending the lease across local finalization.
  Above the foreground-rate heuristic of five requests per second, a non-aged drain defers for 30
  seconds. Its aged turn bypasses only that heuristic for one bounded poll; lease contention is an
  immediate five-second `remote_lease` defer rather than budget-consuming local wait.
- A candidate that references multiple eligible upstream keys stores each successful key response in
  a local, generation-scoped observation table. Each run requests at most two still-missing keys and
  only computes a cross-key total after every current-generation key is present. Candidates with an
  existing current-generation observation are ranked ahead of fresh candidates sharing the same
  scheduling Key, so partial results resume before new work can hide them. The v22 ledger
  migration creates this derived table and index without scanning usage or emitting HA events;
  terminal completion removes the rows. Exhausting the request cap is a typed
  `remote_attempt_budget` continuation at 30 seconds, never a semantic failure.
- An upstream `429` writes cooldown only for the affected `period_reconciliation` Key. The existing
  `5/10/20/30` minute ladder and `Retry-After` determine its next attempt; non-cooling Keys remain
  eligible, and an all-Key cooldown schedules the claim-fenced representative at the earliest
  expiry. Legacy global-backoff metadata remains readable for rolling compatibility but is not a
  reconciliation gate or representative wake source.
- Research polling stores a separate `poll_resolution` alongside its nonterminal row. A confirmed
  404 becomes `unavailable` with `terminal_at` still unset and `next_poll_at=0`; the drain excludes
  that row while the main reconciliation path retains its existing 24-hour degraded protection.
  This lifecycle marker is replicated as runtime state but is not a terminal billing outcome.
- Research 401/403 responses and empty local secrets arm a six-hour
  `reconciliation_research_credentials` cooldown for only the affected Key. The selector excludes
  both this scope and `period_reconciliation` 429 cooldowns, and a successful poll clears only the
  credentials scope. No credentials or raw upstream error text is persisted in observations.
- A Research drain result is observable only after `ResearchDrainCommitReceipt::Accepted`. Its
  poll resolution, Key state, exact cursor, ten-minute progress window, claimed-job finish, and
  unique next representative are one claim-fenced transaction. Deferred and stale receipts never
  advance the cursor or add outcome counters.
- Short maintenance writes use a runtime-owned transaction task. Caller cancellation can no longer
  return an open transaction to the pool: the owner commits or rolls back before restoration.
  Physical detach remains reserved for panic, shutdown, or an unverifiable connection state.
- A request-stats flush applies its short retry budget before a new transaction starts and before a
  later chunk begins. Once `BEGIN IMMEDIATE` succeeds, the runtime-owned finish reaches commit or
  rollback before the coalescer classifies the drained batch; it must never requeue a batch merely
  because the caller's wall-clock budget elapsed while commit was in flight.

## Consequences

- Reconciliation yields without using async cancellation as normal transaction cleanup.
- One upstream request remains the global maximum, but idle local preparation no longer consumes
  that scarce request slot.
- Terminal diagnostics distinguish connection-scoped SQLite pages from process/cgroup I/O totals.
- A native progress handler is an FFI boundary and therefore requires cleanup and pooled-connection
  regression coverage.
- Partial key observations are node-local rebuildable state. Usage, work generation, settlement, and
  billing adjustments remain the reconciliation truth shared through the existing ledger and HA
  paths; a stale generation or claim cannot consume observations from another run.
- Admin Alerts reads use their own bounded read session: a 100ms acquire and a 250ms native SQLite
  deadline over one snapshot. Exact-key last-good data may be served stale; a cold key returns
  `503 Retry-After: 1` and never falls back to the raw CTE path. The canonical catalog, events
  page `1/20`, and groups page `1/20` keys are owned by an AppState background warm controller;
  performs one bounded `AdminAlertsCacheWarm` read per slice when a single pool connection is
  available, foreground activity is at most five requests per second, and recent contention is
  clear. It does not prewarm or reserve extra connections: a lazy pool can run the warm slice with
  its one idle connection, while a foreground checkout or waiter causes a typed defer. It stages
  all three values behind one projection-generation fence and publishes them together. A deferred
  warm retries at `5s`, `5s`, then `30s`; a generation change re-arms one warm without allowing
  HTTP to trigger a rebuild.
