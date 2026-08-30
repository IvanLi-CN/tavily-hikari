# Period-scoped upstream usage reconciliation

## Current scheduling contract

Candidate windows are maintained in an indexed durable work projection. The engine hydrates a bounded page,
starts primary settlement before research polling, and permits at most two serial main settlement requests per
run. Research remains an independent, globally single-concurrent sweep. When due Research is known during
preparation, it reserves two seconds for the later Research sweep and two seconds
for main durable finalization before it starts main remote work; the reserve may preclude a second slow main
request. Without due Research, main settlement retains its normal remote envelope. Research exhaustion is
diagnostic follow-up, not primary local pressure. Local-pressure backoff (`30/60/120/300s`) is separate from the
per-key upstream-429 cooldown (`5/10/20/30m`); a 429 only cools the affected `period_reconciliation` key,
and non-429 failures do not reset that key's cooldown. A current claim that reaches a
low-foreground recovery window clears only its local-pressure state before trying the engine again;
if SQLite is still pressured, that attempt durably defers again. This preserves foreground yielding
without letting a stale local backoff consume the whole recovery tail.

Terminal completion is typed. Active non-zero delta uses `settled`, any zero delta uses
`no_adjustment`, and compare-mode non-zero delta uses `observed` without writing billing truth.
Each terminal result finishes only the current usage generation; transport failure, semantic failure, upstream `429`, and local pressure preserve
their own durable retry state. This prevents a valid zero-delta observation from becoming a new
minute-by-minute reconciliation job and prevents a non-429 result from falsely clearing a remote
rate-limit circuit.

Transport failures need a small durable diagnosis, not a copied error. Map endpoint/connect,
timeout, body-read, credential/database, and unknown failures into a fixed category, retain the
work generation, and retry it on `30/60/120/300s`. Only a later terminal result clears the
recovered state; do not log or expose the upstream body, URL, token, or database error text.

The main candidate result must cross its durable fence before a Research sweep starts. Research
selection uses the due covering index with an 80-row keyset page, a four-per-key cap, and a
20-row sweep cap. A Research read-budget defer schedules one 30-second continuation without
rewriting the main result or advancing the cursor.

The two-second Research sweep reserves its final 500ms for local durable writes. A slow probe can
use only the first 1.5 seconds; when it times out, persist a normalized `retry`/`timeout` outcome
and its next poll before returning the 30-second continuation. Accept the cursor and sweep marker
in one claim-fenced transaction only when the whole selected page has durable outcomes. A missing
eligible upstream key is a separate fifteen-minute input retry and aggregate diagnostic, not a
semantic failure or a billing decision.

For a period that maps to more than one eligible upstream key, persist each successful key observation
by work generation before requesting another key. Cap each run at two remote requests. If keys remain,
write `remote_attempt_budget` and schedule one durable 30-second continuation; do not write a semantic
failure or terminal result. Sum usage and enter the existing compare/active terminal path only after
all current-generation key observations are present. Delete the local observations atomically with
terminal completion, and fence both observation writes and reads by claim generation.

For a period that maps to more than one eligible upstream key, persist each successful key observation
by work generation before requesting another key. Cap each run at two remote requests. If keys remain,
write `remote_attempt_budget` and schedule one durable 30-second continuation; do not write a semantic
failure or terminal result. Sum usage and enter the existing compare/active terminal path only after
all current-generation key observations are present. Delete the local observations atomically with
terminal completion, and fence both observation writes and reads by claim generation.

## Activation controller

Treat the existing precise-reconciliation switch as the sole operator action. Persist and replicate
a controller state rather than recalculating a mode from transient readiness checks: `false` selects
`compare`; `true` selects `active` immediately and records the next complete business period as the
actual-billing boundary. Work from earlier periods remains shadow-only and completed observations
are never replayed. If durable correctness fails, record `active_paused`, clear the legacy switch,
and require a new true write to resume. This preserves a simple operator action while keeping the
first actual adjustment behind a stable period boundary.

Historical usage needs a separate lifecycle from live settlement. Mark an empty new database as
projection-complete during its versioned migration. For an upgraded database with historical rows,
read one stable `(token_id,key_id,period_code)` keyset page outside the writer, aggregate in memory,
then atomically merge work and CAS-advance the cursor in a short claim-fenced transaction. Adapt the
page between 25 and 100 rows from transaction time, and check the cooperative run budget only at safe
boundaries. Never use cancellation as the normal way to end an open write transaction.

Every reconciliation preparation source `SELECT` gets its own fresh 250ms connection-local SQLite
progress-handler session: recent/backlog candidate lanes, candidate/billed-credit hydrate, Research
candidates, and the historical projection page. A deadline interrupt leaves the stable cursor and
work truth unchanged, stops later preparation and remote work, and records one claim-fenced
30-second defer; the handler must be removed before the connection can return to the pool. Keep the
source SQL unchanged until scoped read-kind timing proves it remains the bottleneck.

Billed-credit hydrate is a pre-request source-read gate. Once HTTP has completed, settlement reads
the current ledger through a separately bounded finalization connection so a charge recorded during
the request is reflected without turning a post-request consistency read into a source-deadline
defer.

The global remote limit is a lease around the actual outbound HTTP request, not the whole
reconciliation run. Local projection, hydrate, finalization, and Research bookkeeping release the
lease. Manual remote work retains priority, while an automatic reconciliation representative that
has been eligible for 120 seconds receives the next non-manual remote turn.

When the upstream only exposes cumulative usage counters, a proxy cannot do exact per-request
billing by reading upstream state inline. Tavily Hikari solves this by splitting local billing
into two phases: optimistic request-time charging, then one idempotent settlement per complete
business period.

## When to use this pattern

Use this pattern when all of the following are true:

- the upstream exposes cumulative usage totals instead of per-request receipts;
- the proxy must keep user-visible quota accurate enough for day-to-day operations;
- the proxy can identify the effective upstream billing subject after routing;
- the system can tolerate a bounded delayed reconciliation step.

## Core pattern

1. At request time, charge locally using the proxy's normal business-cost rules.
2. Record the exact tuple that matters for later settlement:
   - local billing subject (`token` / `unbound token`);
   - effective upstream key;
   - business period code.
3. Freeze the period code at request ingress so later retries, async jobs, or time drift do not
   move the request into another window.
4. After the full business period closes, query the upstream cumulative usage only for the tuples
   that were actually used in that period.
5. Compare the upstream total with the sum of already-charged local credits.
6. Apply a signed adjustment (`+` extra charge, `-` refund) through a dedicated reconciliation
   ledger keyed by a unique settlement key.

For compare-only operator views, do not hide the new value until every window settles. Expose a
hybrid number instead:

- `hybrid daily value = current local daily credits + confirmed shadow delta sum`
- mark it `projected` until every same-day observed window reaches a terminal settlement state;
- upgrade it to `confirmed` once all same-day windows are `shadow_settled` or `shadow_degraded`.

## Why period windows matter

If the upstream counter is cumulative, the proxy must pick a window boundary that is:

- stable for users and operators;
- easy to reason about operationally;
- late enough that most async work has already reached terminal state.

Tavily Hikari uses server-local business periods instead of UTC month-only settlement:

- `S1 = 00:00-11:00`
- `S2 = 11:00-22:00`
- `S3 = 22:00-24:00`

This keeps same-day quota corrections timely without needing multiple automatic rechecks.

## Required invariants

- One settlement key per `(billing subject, period code)`.
- One upstream aggregation input per `(upstream key, period code)` actually observed in traffic.
- Period attribution must survive restarts and HA failover.
- Research / async jobs must either reach terminal state before settlement or enter a single
  degraded path with a recorded reason.
- Reconciliation adjustments must affect the original business window, not the current wall clock
  window.

## Idempotency rules

The settlement worker must be safe to retry at any point:

- repeated queue scans must not duplicate adjustments;
- repeated upstream `/usage` reads must not create a second settlement row;
- a hot upstream key that returns `429` or hits the proxy's local usage-query throttle should apply
  one durable reconciliation-only key cooldown, while recording retry state only for the current
  window; never fan that transient result out to all same-key windows.
- takeover by another HA node must reuse the same settlement key;
- process restarts must resume from durable recorded usage tuples and settlement state.
- candidate selection should prefer recently closed windows over ancient backlog so same-day UI
  convergence does not stall behind old hot keys.
- async Research must have a server-side terminal sweep. Client result retrieval is an observation
  path, not a completion dependency; persist poll schedule/outcome fields so restart and HA takeover
  continue bounded polling.

The simplest durable contract is:

- `upstream_reconciliation_usage`: observed tuples to settle later;
- `upstream_reconciliation_research`: async work that can delay closure;
- `upstream_reconciliation_settlements`: per-window terminal state;
- `billing_reconciliation_adjustments`: signed accounting events.

## Degraded mode

Do not keep rechecking forever. Pick one maximum wait budget, then settle once with an explicit
degraded reason. This keeps the system operable and makes operator state visible.

Tavily Hikari uses:

- settle 10 minutes after a quiet window with no research;
- settle 10 minutes after all research reaches terminal state;
- fall back to one degraded settlement after 24 hours if terminal state never arrives.
- prefer a two-lane candidate budget during backlog: `recent=12` for today+yesterday windows in
  descending `period_end`, `backlog=8` for older windows in ascending `period_end`, with unused
  budget refilled across lanes.

## Quota correction detail

Refunding a prior-period adjustment must not accidentally gift capacity to the current hour or the
next business day. Corrections should restore only the scopes that still belong to the attributed
window.

In Tavily Hikari:

- same-day settlements can restore hour/day/month availability for the original day;
- `S3` next-day settlement restores the original day and month accounting without reopening the
  current hour bucket.

## Operational visibility

Operators need to know whether exact reconciliation is active or merely configured. Expose:

- configured vs effective anonymization mode;
- activation gates;
- active legacy sessions still preventing precise mode;
- queued settlements;
- pending async work;
- degraded settlements;
- `rate_limited` buckets split into upstream `429`, local usage-query throttling, and other retry
  causes;
- current-period per-key activity, including bound-user count and pending Project ID count, with
  sensitive ids shortened to stable local hints;
- same-day account standard-settlement coverage, period/Research terminal coverage, and per-key
  pending Research plus reconciliation-only cooldown state;
- recent signed adjustments.

This is why Tavily Hikari ships a dedicated `System Status` admin page instead of hiding the state
inside logs.

## Key-scoped upstream cooldown

An upstream `429` belongs to the `period_reconciliation` Key that returned it. Persist one cooldown
for that Key using the existing `5/10/20/30` minute ladder, honoring a later `Retry-After`, and
leave other Keys eligible. A run that finds every otherwise eligible Key cooling down does no HTTP
work and returns a claim-fenced deferred outcome at the earliest cooldown expiry; it does not turn
the condition into a run-wide circuit or a semantic failure.

The legacy global-backoff meta remains readable for rolling compatibility, but it is not a live
engine gate, representative wake source, or administrator blocker. Keep per-Key cooldown events at
DEBUG and reserve state-transition logs for entering, escalating, and recovering the affected Key.

Run reconciliation in a remote-I/O concurrency class with a total wall-clock budget. Before each
new upstream request, verify that enough budget remains for the request deadline; a timeout around
the entire worker alone still permits the last request to consume the full remainder. Persist the
pressure transition and the single delayed representative job atomically so restarts neither lose
backoff nor enqueue minute-by-minute no-op work.

Candidate pressure must be measured without a full queue aggregate: use an indexed recent/backlog
page, an exact `hasEligible` existence check, and the oldest candidate age. The observation contract
uses nullable, explicitly bounded estimates so an unobserved queue is not reported as zero and a
multi-item queue is not collapsed into a boolean. Keep historical degraded visibility as a separate
indexed existence probe instead of deriving it from a current-day metric. After three rounds with no
remote attempt and exhausted local budget, persist a short local backoff without changing any Key
cooldown. Only an actual upstream 429 attempt advances the affected Key's `5/10/20/30` minute
cooldown and honors a later Retry-After; a real remote attempt or successful settlement clears only
the recovered Key state. Keep normal per-Key 429 logs at DEBUG and reserve state-transition logs for
enter, escalation, and recovery.

Main settlement must start before terminal-research polling. Hydrate the bounded candidate page,
key/cooldown state, and Research eligibility in the two-second local preparation budget. If Research is due,
reserve its two-second post-finalization sweep before starting main HTTP, along with the two-second main
durable-finalization boundary. This is a progress guarantee for due Research, not permission to poll before
main settlement; with no due Research, do not reduce main remote capacity.

## Claim-fenced deferred finalization

Treat finalization as durable work with its own reserve, not as an afterthought after remote I/O.
Before starting the next remote request, reserve two seconds for the terminal observation and
continuation boundary. If that reserve is exhausted, return only a typed deferred outcome with a
reason and retry time.

The scheduler must persist one deferred outcome through a single claim-fenced control transaction:
finish the claimed job, advance independent local-backoff metadata, record local-pressure observation
and retry time, and retain or create one delayed auto representative. Finalization-reserve exhaustion,
admission defers, and transient SQLite pressure before a durable boundary are typed defers; other
non-stale, non-transient failures remain terminal job errors so the scheduler does not hide invariants
or storage faults. If the transaction cannot acquire the writer, leave the claim running for stale
recovery. Do not add an in-memory retry loop that can fan out jobs after restart.

Research health is separate from settlement retries. For a current-period starvation investigation,
observe a fixed ten-minute window: terminal rate must become positive and pending Research must not
grow. An `upstream429` bucket measures settlement retry pressure only; it is neither backlog size
nor proof of terminal progress.
