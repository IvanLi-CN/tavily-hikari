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
- This ADR does not change the projection SQL shape. A keyset, batch-lookup, or index rewrite needs
  separate candidate evidence showing that the scoped source read remains the bottleneck.

## Consequences

- Reconciliation yields without using async cancellation as normal transaction cleanup.
- One upstream request remains the global maximum, but idle local preparation no longer consumes
  that scarce request slot.
- Terminal diagnostics distinguish connection-scoped SQLite pages from process/cgroup I/O totals.
- A native progress handler is an FFI boundary and therefore requires cleanup and pooled-connection
  regression coverage.
