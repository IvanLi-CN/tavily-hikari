# ADR 0005: Fair SQLite Maintenance Admission

## Status

Accepted

## Context

The service uses SQLite as its core persistence boundary. Foreground request work and billing
truth need bounded, synchronous access, while several derived writers share one physical bulk
permit: request-stat flushing, dashboard projections, HA outbox cleanup, request-log cleanup,
server-pressure persistence, alert projection, and reconciliation projection. Each existing worker
already returned a typed defer instead of waiting indefinitely, but independent retry loops could
repeatedly reacquire the permit and leave an older maintenance class waiting behind a newer one.

The problem is admission fairness and maintenance freshness. Increasing the SQLite pool would not
remove SQLite's single-writer limit, and compaction or a database migration would not order the
existing writers. A broad asynchronous waiter queue would also make cancellation and shutdown
ownership harder to bound.

## Decision

Keep one physical maintenance-bulk semaphore and the existing foreground-capacity and recent-
contention policy. Add one instance-local coordinator in front of that semaphore:

- The coordinator has a fixed set of maintenance classes and at most one pending ticket per class.
- Tickets are served oldest-first by their first request time and a monotonic tie-breaker. If the
  oldest ticket is not being retried, any caller whose own ticket has waited at least five seconds
  may take the turn; this keeps a low-frequency worker from holding every other class past the
  freshness bound.
- Ordinary admission still reserves two foreground pool slots. Aging changes maintenance-class
  ordering and can let an eligible turn reach its bounded pool acquire even when all currently-open
  connections are checked out, but ordinary admission never bypasses the foreground-rate gate.
  [ADR 0008](./0008-bounded-request-statistics-recovery.md) defines the scoped exception for aged
  dashboard-integrity and request-log-GC recovery turns. The 100ms acquire
  budget is the safety boundary, so a full pool cannot starve an aged ticket behind a pre-admission
  `pool_pressure` check. Pools at or below the two-slot foreground reserve remain foreground-only.
- Admission is non-blocking. A caller either receives the physical permit plus a coordinator lease
  or receives the existing typed defer reason and retries through its existing bounded loop.
- A class keeps its one pending ticket while a caller is still retrying any typed admission defer;
  a `bulk_busy` result does not create a second ticket. This preserves the original queue-time
  fairness anchor across foreground, pool, and recent-contention pressure.
- Reconciliation preflight owns a drop guard: if claim validation or a controlled retry exits before
  the real bulk admission, the unused ticket is cancelled; only the path entering that admission
  explicitly transfers the ticket to the bulk admission retry loop.
- A pending class expires after six minutes without another retry. This preserves its age across
  the five-minute pressure backoff while bounding abandoned work; durable scheduled-job state
  remains responsible for work that must survive process lifetime.
- The lease is held only for the local SQLite slice. Remote requests and their response handling
  are outside the lease, and foreground request work, billing truth, and control-plane writes do
  not become eventual.
- Runtime workload-window logs expose pending age and per-class admission/completion statistics;
  each admitted slice logs its class and wait age.

Maintenance evidence must distinguish physical bulk-permit ownership from coordinator fairness,
pool acquisition, BEGIN, and native-read deadline. These are bounded internal classifications;
they do not change admission, SQL, or foreground write behavior. Only a committed cursor or range
advance is business progress. Lease release, rollback, stale-claim rejection, and observation-row
updates remain operational events and must not be counted as progress.

Ordinary admission-defer continuations and the coordinator's aged-class-order turn use a
five-second cadence. Request-log GC retries admission-only `bulk_busy`, `pool_pressure`,
`foreground_pressure`, and `recent_contention` defers on that same five-second cadence; unknown
admission reasons retain the conservative five-minute retry. Source or archival blocking and an
actually executed zero-progress pass retain their existing five-minute protection. Dashboard
integrity continues to re-probe transient admission defers on the aged-turn five-second cadence.
This changes only retry timing: foreground-rate, pool-capacity, recent-contention, single-permit,
and coordinator-fairness checks remain enforced on every attempt. A normally completed
request-log GC continuation keeps its five-minute cadence, and HA GC's post-admission channel
continuation keeps its separate durable contention delay.

## Alternatives Rejected

- Expanding the SQLite pool: this increases concurrent connection pressure without increasing the
  single-writer capacity and can reduce foreground reservation.
- Treating compaction as contention control: compaction is an operational maintenance action and
  does not arbitrate ordinary derived writers.
- Moving core persistence to Postgres: this changes the persistence contract and is outside the
  scoped contention problem.
- One unbounded async maintenance queue: it complicates cancellation, process shutdown, and
  bounded memory without improving durable fairness across restarts.

## Consequences

Fairness is enforced only for the instance-local physical bulk permit. Durable scheduled-job
queue semantics still provide process-restart recovery. Derived observations may remain boundedly
eventually consistent, while foreground request and billing correctness are unchanged. The
coordinator adds a small mutex operation to each bulk admission and observable counters that make
maintenance starvation and stale retries diagnosable. A slow retrying class can yield its strict
oldest-first position after five seconds, while every caller remains bounded by its own ticket age.