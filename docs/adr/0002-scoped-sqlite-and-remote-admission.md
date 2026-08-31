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
  interrupt to a typed `projection_read_budget` deferred outcome.
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
- Manual remote work keeps dispatch priority. Once an automatic reconciliation representative has
  been eligible for 120 seconds, it owns the next non-manual remote turn until it starts one
  request or exits through a typed no-request terminal/deferred boundary.
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
  request-scoped remote admission boundary without extending the lease across local finalization or
  consuming a main reconciliation fairness turn.
- A candidate that references multiple eligible upstream keys stores each successful key response in
  a local, generation-scoped observation table. Each run requests at most two still-missing keys and
  only computes a cross-key total after every current-generation key is present. The v22 ledger
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
