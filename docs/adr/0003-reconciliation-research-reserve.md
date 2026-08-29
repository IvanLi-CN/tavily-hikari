# ADR 0003: Due Research Reserves a Reconciliation Window

## Status

Accepted

## Context

Reconciliation starts main `/usage` work before terminal Research polling. A slow main request can
consume the former Research start threshold and leave due Research unprobed forever while the same
main work keeps retrying. Treating Research as only leftover time therefore does not provide a
progress guarantee.

Research must not run before main settlement. The engine may issue at most two serial main
settlement requests per run; Research remains an independent, globally single-concurrent sweep.
The decision must preserve compare-mode billing truth and the durable claim-fenced continuation
boundary.

## Decision

- Preparation reads due Research eligibility without issuing a Research request.
- When due Research exists, main remote work reserves two seconds for the later Research sweep and
  two seconds for the main result's durable finalization boundary before it starts a main request.
  A second slow main-key request may therefore be skipped.
- Research probes still start only after main observation retry/finalization reaches its durable
  boundary. The sweep uses at most its reserved two seconds, of which the final 500ms is a fixed
  local persistence reserve. Outbound probes may use only the preceding 1.5 seconds.
- A probe timeout writes its claim-fenced `retry` outcome, normalized `timeout` kind, and next poll
  time before the run defers. The page cursor and sweep marker advance together only after every
  selected row has a durable outcome; an unfinished page remains at its current cursor.
- When no Research is due, main remote work retains its existing request envelope and can use both
  serial main attempts.
- If the protected durable boundary cannot be reached, the run returns the existing typed deferred
  outcome and claim-fenced delayed representative. It does not start Research early, complete work,
  or change billing truth.
- Run diagnostics include the logical main remote request count and whether the Research reserve
  was required. These counters do not expose keys, tokens, request IDs, URLs, or response bodies.

## Consequences

- Due Research becomes independently progressable under recurring slow main `/usage` work.
- A run with due Research may complete fewer main-key requests than an otherwise identical run.
- The existing remote-attempt admission, two-main-attempt cap, mode semantics, and billing rules
  remain unchanged. Research retains its independent two-second sweep budget.
