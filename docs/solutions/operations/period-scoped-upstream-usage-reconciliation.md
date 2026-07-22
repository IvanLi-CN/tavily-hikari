# Period-scoped upstream usage reconciliation

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
  one key-scoped backoff to all due windows for that key, instead of letting the next scheduler pass
  immediately spend the whole candidate budget on the same key again;
- takeover by another HA node must reuse the same settlement key;
- process restarts must resume from durable recorded usage tuples and settlement state.

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
- recent signed adjustments.

This is why Tavily Hikari ships a dedicated `System Status` admin page instead of hiding the state
inside logs.
