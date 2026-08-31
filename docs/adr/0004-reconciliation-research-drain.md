# ADR 0004: Research Uses an Independent Durable Drain

## Status

Accepted

## Context

ADR 0003 reserved a tail of each main reconciliation run for terminal Research polling. That
bounded the tail, but did not guarantee progress when main multi-Key work repeatedly consumed its
two-request allowance before the Research phase. Research already has a durable v21 selector and
stable cursor, so tying its liveness to the main run is unnecessary.

## Decision

- `upstream_reconciliation_research_drain` is the single production owner of Research polling and
  the v21 scan cursor. The main claimed reconciliation path never sends Research HTTP requests.
- The drain processes at most one candidate and schedules its next normal run no earlier than five
  seconds later. It shares the instance-wide single actual-request lease but does not consume the
  aged main reconciliation turn.
- A claim-fenced control transaction accepts the Research result, any affected
  `period_reconciliation` Key cooldown, the exact processed cursor, and the drain observation
  together. Cancellation, stale claims, and local pressure advance none of them.
- Selection applies the same closed-period eligibility before the 80-row limit and during hydrate.
  Only an actually processed candidate advances the cursor. A five-minute forced wrap rediscovers
  dynamically eligible rows behind the cursor; accepting the forced page refreshes the sweep clock
  in the same transaction so the selector does not repeatedly rescan the prefix.
- A Key-level `429` affects only that Key. Other due Keys remain selectable; when every due Key is
  cooling, a separate bounded lookup wakes the representative at the globally earliest eligible
  cooldown expiry rather than the earliest value in the current cursor page.
- Startup, the stale-job watchdog, and a safely completed main run ensure the unique drain
  representative. Research no longer contributes to the main representative's continuation time.
- The accepted drain transaction also writes its ten-minute progress window and finishes or
  enqueues the unique next representative. Logs and runtime counters are emitted only from that
  accepted receipt, so a stale claim or failed transaction cannot create false convergence evidence.

## Consequences

- Main work keeps its complete two-request budget, while due Research has an independent liveness
  path capped at 12 polls per minute with burst one.
- Research outcome and cursor acceptance no longer form two transactions.
- Existing v21 tables and indexes are sufficient; no schema migration or historical replay is
  required.
- ADR 0003's reserved-tail scheduling decision is superseded. Its requirement that main results be
  durable before optional follow-up remains satisfied because production Research is now a separate
  claimed job.
