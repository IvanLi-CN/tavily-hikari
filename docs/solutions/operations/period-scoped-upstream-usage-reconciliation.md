# Period-scoped upstream usage reconciliation

## Current scheduling contract

Candidate windows are maintained in an indexed durable work projection. The engine hydrates a bounded page,
starts primary settlement before research polling, permits at most two serial remote attempts per run, and gives
research at most two seconds of the remaining time. Research exhaustion is diagnostic follow-up, not primary
local pressure. Local-pressure backoff (`30/60/120/300s`) is separate from upstream-429 backoff
(`2/5/10/30m`); non-429 failures do not reset the remote circuit. A current claim that reaches a
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

## Global upstream pressure backoff

When candidates exist but a run settles none and at least half of its attempts are upstream 429s,
preserve the per-key cooldown records but also persist a run-level pressure streak. After three
consecutive pressure runs, skip further candidate work for 2, 5, 10, then 30 minutes; successful
settlement or a lower pressure ratio clears the state. Emit per-key cooldowns at DEBUG and reserve
WARN for entering, escalating, or recovering the global state so diagnosis does not become the
dominant write or log workload.

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
remote attempt and exhausted local budget, persist a short local backoff without changing remote
429 state. Only actual upstream 429 attempts advance the global `2/5/10/30` minute backoff and honor
a later Retry-After; a real remote attempt or successful settlement clears the relevant state. Keep
normal per-key 429 logs at DEBUG and reserve state-transition logs for enter, escalation, and recovery.

Main settlement must start before terminal-research polling. Hydrate the bounded candidate page and
key/cooldown state in one indexed batch, reserve the first eligible key within the two-second local
preparation budget, then use the remaining 20-second job budget for the research sweep.

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
