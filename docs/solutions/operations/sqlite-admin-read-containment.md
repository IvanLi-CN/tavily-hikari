---
title: SQLite admin read containment
module: tavily-hikari
problem_type: production_slow_queries
component: sqlite-admin-reads
tags:
  - sqlite
  - admin
  - performance
  - operations
status: active
related_specs:
  - docs/specs/ev4td-admin-recent-requests-performance-copy/SPEC.md
  - docs/specs/66t8u-admin-dashboard-overview-performance/SPEC.md
---

# SQLite admin read containment

## Context

Tavily Hikari uses SQLite for request logs, token logs, API key metrics, user management, and
dashboard admin reads. When the database grows, admin endpoints that aggregate facets or scan wide
history can occupy the limited `sqlx-sqlite` worker pool and make unrelated admin endpoints wait.

## Symptoms

- `sqlx-sqlite` worker threads stay at high CPU.
- Admin endpoints such as `/api/logs`, `/api/logs/catalog`, `/api/users`, `/api/tokens`,
  `/api/keys`, `/api/stats/forward-proxy`, and `/api/dashboard/overview` move from sub-second to
  seconds or minutes.
- Logs may include `database is locked` while health checks still look normal.

## Root Cause

The risky pattern is not a single slow query. It is a combination of unbounded or repeated admin
reads:

- catalog facets scanning request log history after every cache invalidation,
- legacy list pages selecting request/response bodies by default,
- repeated window stats over the same source table,
- multiple heavy admin reads running concurrently against the same SQLite worker pool.

## Resolution

- Default global request-log list and catalog reads to the configured retention window.
- Keep request/response bodies out of list rows; fetch bodies only from scoped detail endpoints or
  explicit diagnostic paths.
- Treat hot-write catalog invalidation as a load amplifier. Prefer short TTL caches for unfiltered
  catalog scopes, and invalidate on structural deletes such as request-log GC.
- Move global request-log catalog facets and legacy `/api/logs` totals/facets to a narrow,
  retention-bounded rollup table. Keep exact count semantics by retaining timestamp-level rollup
  filters, running canonical request-kind migration before rebuilding retained history, and
  canonicalizing legacy write-path rows before they enter rollup deltas. Persist the retention
  window used for the rebuild and rebuild again when it changes.
- Do not put rollup-backed catalog reads behind the same shared semaphore used by genuinely heavy
  admin reads. A catalog cache miss should not make `/api/users`, `/api/tokens`, or `/api/keys`
  queue behind it.
- Use a bounded admin heavy-read semaphore around facet catalogs, legacy page queries, user/token
  lists, key list facets, and similar management reads.
- Recheck cache after acquiring the semaphore so concurrent cache misses collapse into one heavy
  query.
- Replace repeated window scans with a single bounded scan that derives all needed windows, then add
  a short manager-scoped TTL cache when settings and live stats can request the same window set in
  one admin refresh cycle.

## Guardrails / Reuse Notes

- Do not fix SQLite worker saturation by increasing the worker pool first; that often makes the
  database do more concurrent work and increases lock pressure.
- New admin list endpoints should define a default time window or a small page/cursor contract
  before adding totals and facets.
- If a list hides bodies, compute canonical request kind and operational metadata in SQL before
  mapping rows, otherwise legacy rows that need body inspection can be misclassified.
- Keep trigger SQL simple. Complex legacy body classification can exceed SQLite parser limits when
  embedded in rollup triggers; prefer canonicalizing retained legacy rows before rollup rebuild,
  using a focused canonicalization trigger for legacy write-path rows, then keeping rollup triggers
  on stored canonical columns only.
- Production stop-the-bleed actions such as single-container restart are live changes and require
  explicit owner approval.

## References

- `src/store/key_store_request_logs_and_dashboard.rs`
- `src/store/key_store_token_logs.rs`
- `src/store/key_store_keys.rs`
- `src/forward_proxy/storage.rs`
