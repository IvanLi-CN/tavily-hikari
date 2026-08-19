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
  - docs/specs/admin-recent-requests-performance-copy/SPEC.md
  - docs/specs/admin-dashboard-overview-performance/SPEC.md
  - docs/specs/sqlite-write-lock-hardening/SPEC.md
  - docs/specs/admin-token-bulk-filters/SPEC.md
---

# SQLite admin read containment

## Current contract

Ordinary administrator HA status reads are cache-only. A background observation owner probes peers every 30 seconds with a bounded timeout, preserves last-good data, and marks an observation stale after 90 seconds without success. Promote, cutover, and finalize keep the live-probe path.

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
- For administrator alert catalog/events/groups, key last-good data by the normalized complete
  query rather than sharing one broad cache entry. Under SQLite admission pressure return only the
  matching stale result with `coverage`, `observedAt`, and `staleReason`; a cold key returns
  `503 Retry-After: 1` rather than starting a raw alert CTE.
- Apply the same last-good boundary to the single-key privacy-status read. Keep the immutable
  successful snapshot for 60 seconds; warm pressure returns it as stale with the observation time,
  while cold pressure fails fast with `503 Retry-After: 1`. The bounded read deadline is 250ms,
  including operation admission and result construction.
- Replace repeated window scans with a single bounded scan that derives all needed windows, then add
  a short manager-scoped TTL cache when settings and live stats can request the same window set in
  one admin refresh cycle.
- Collapse `/api/dashboard/overview` and admin SSE `snapshot` onto one freshness-aware shared
  snapshot loader. Reuse the same materialized overview within one refresh wave, but invalidate it
  immediately when summary totals, request-log signature, exhausted-key subset, disabled-token
  coverage, recent jobs, recent alerts, forward-proxy counts, quota-sync freshness, or current-hour
  anchor changes.
- Keep the dashboard freshness contract cheap. The SSE `/api/events` loop should watch a bounded
  freshness probe, not re-serialize `summaryWindows` and month-series payloads every two seconds.
  In this service the low-cost contract is enough when it includes summary totals, local
  day/month anchors, forward-proxy counts, retention window anchor, latest visible request-log id,
  exhausted-key ids, disabled-token coverage, recent-job signatures, recent-alert aggregates, and
  the current hour anchor.
- For admin summary/rankings/analysis-pressure reads that only need request-stat rollups, consume
  durable rollups only. Do not acquire a write connection or synchronously flush from a read;
  pending/flushing state belongs to freshness coverage while one background-admitted batcher
  persists the delta. This keeps first paint out of SQLite writer contention entirely.
- When the admitted background batcher encounters a bounded writer conflict, preserve its exact
  drained delta and return it to the coalescer before reporting degraded freshness. The next
  admitted cadence can persist it; an owner-facing read must continue returning durable last-good
  data rather than inheriting the failed write path.
- Keep the request-stats cadence bounded without allowing its in-memory tail to become a second
  pressure source: one nominal wake may commit at most four adaptive `25..250` logical-key
  transactions under one 50ms budget, then returns the remaining tail atomically. During a cold
  dashboard singleflight, SSE clients emit degraded frames instead of independently starting
  freshness reads against the small shared pool.
- When dashboard overview depends on coalesced request-stat rollups, split “probe freshness” from
  “rebuild payload”. The probe path should use non-flushing summary / rollup reads plus a pending
  coalescer signature, while the actual shared-snapshot rebuild may flush once. Reusing the rebuild
  freshness as the emitted SSE signature prevents the next 2s poll from seeing a stale pre-rebuild
  signature and rebuilding again immediately.
- Treat optional dashboard feeds as `last-good` data instead of all-or-nothing prerequisites. If
  disabled-token or recent-job reads fail, keep the core overview payload serving and let the
  optional slice surface `error`/empty coverage semantics on the next snapshot rather than timing
  out the whole admin page.
- Put the Dashboard overview behind one AppState-owned immutable read model. A dirty generation may
  request one shared rebuild every ten seconds and a bounded sixty-second probe catches missed
  invalidations; an HTTP or SSE request under pressure returns the last-good snapshot with explicit
  stale coverage instead of recomputing freshness.
- Keep dashboard alert summaries out of the raw CTE hot path. Persist a sidecar projection using a
  stable source cursor plus fence/tail replay, with an independent recent Dashboard tail and
  historical administrator lane. The projection worker materializes a bounded recent-summary record
  at most once per sixty-second window; a newer source generation marks that record stale until the
  next refresh. HTTP and SSE read that record plus coverage only. The read model consumes only
  complete recent coverage and retains last-good data while its tail catches up;
  Events/Groups wait for complete historical coverage. Expose coverage, observation time, and stale
  reason rather than issuing an exact event count on every admin status read.
- A derived alert projection must be a lower-priority maintenance writer. The Dashboard may consume
  only its bounded recent window, while administrator Events/Groups switch to the sidecar only after
  a stable cursor has replayed complete source history and reports explicit `ok` coverage. Keep
  writes in small replayable slices, convert transient SQLite contention into a typed defer, and
  leave the first post-start bulk window to recovery work with a durable SLO such as HA GC. During a
  full-history catch-up, keep administrator reads on the established source query rather than
  presenting incomplete sidecar coverage as an empty result.
- Treat an empty source fence as an idle result, not as a successful projection slice. Do not bump
  a durable cursor or generation merely to keep a scheduler loop moving. Refresh observation on a
  separate low-frequency cadence, and keep Dashboard summary work in SQLite with fixed windows,
  scalar counts, and a bounded top-group page rather than deserializing every projected payload.
- Guard shared admin snapshot loaders with both a drop-time reset and a stale-loading takeover
  window. A cancelled or wedged request must not leave `loading=true` forever and make every later
  request wait on a `Notify` that will never fire.
- Before exposing a newly restarted listener, give the existing cold singleflight snapshot loader
  its normal bounded head start. This prevents the first concurrent Dashboard clients from all
  arriving at the same one-second cold-read deadline while keeping the loader, timeout, and HTTP
  response contract unchanged.
- Keep optional freshness tokens on a short deadline and derive a conservative token from the
  already-built optional summary when the dedicated token query is slow or unavailable. Freshness
  precision is less important than preserving first paint for the owner-facing dashboard.
- Keep default structured perf logs on the owner-facing read path itself. Dashboard overview/shared
  snapshot and recent-request list/catalog endpoints should emit stable `component=admin_read event=...` records with `elapsed_ms`, route/scope metadata, and runtime memory headroom so low
  memory protection can be triggered and diagnosed without ad-hoc debug builds.
- For public metrics or SSE surfaces backed by request-stat rollups, gate synchronous flushes on
  persisted freshness plus the oldest pending coalesced write. Do not force a flush on every public
  read once the rollup window is already current enough.
- Move alert events/groups/recent summary/catalog to SQL-side pagination and aggregation. Pulling
  all matching alert events into Rust and then sorting, grouping, or paginating in memory does not
  survive a retained `auth_token_logs` window.
- For alert grouped reads on SQLite, avoid parser-sensitive named window clauses when the same
  partition logic can be expressed inline. Keep the grouped projection in SQL, but prefer inline
  `OVER (...)` windows plus a final `group_rank = 1` collapse over `WINDOW ... AS (...)` syntax on
  the production read path.
- When expanding a selected mother-group page back into raw alert events, ensure every
  subject/time predicate block is added through the `separated(" OR ")` boundary itself. If the
  whole block is emitted with `push_unseparated(...)`, SQLite receives adjacent predicates like
  `(...)(...)` and `/api/alerts/groups` fails with `near "("`.
- Canonicalize alert `request_kind` inside the SQL projection before filtering or grouping rows.
  Mixed legacy keys such as `tavily_search` / `mcp_search` otherwise drift from the canonical
  request-kind keys returned by the HTTP contract and can make filtered pages appear empty.
- Prefer `auth_token_logs`-native fields and narrow joins on alert reads. If a path only needs
  request kind, failure class, token, or mirrored API-key metadata, do not widen it with a
  `LEFT JOIN request_logs` just to re-derive fields already stored on the alert-side truth table.
- When a grouped alert projection carries both aggregate window columns and a hydrated
  `latest_event`, prefer the latest event's semantic window for owner-facing copy. Mixed local
  limit families can share one alert type while using different windows such as rolling `5m`
  request-rate caps and rolling `60m` business-call caps; if the UI trusts stale group-level window
  defaults first, dashboard badges can mislabel the actual alert cause.
- For per-user IP statistics over `request_logs`, force the user/IP/time index on count, sample, and
  timeline reads. On large databases SQLite can prefer the visibility/time index for
  `visibility + created_at` predicates and then build temporary B-trees for `GROUP BY`,
  `COUNT(DISTINCT)`, and ordering, which turns `/api/users?sort=recentIpCount7d` and
  `/api/users/:id` into multi-second reads.
- For list pages that need per-user request-log facts, page the user set before hydrating secondary
  details. If a query is bounded by a small user set but SQLite chooses a broad time/visibility
  index, reshape it or use `INDEXED BY` so it seeks by user first instead of scanning the full
  retained window.

## 101 readback

- Current production stack resolution on machine 101 is unambiguous:
  - stack root: `/home/ivan/srv/ai`
  - compose file: `/home/ivan/srv/ai/docker-compose.yml`
  - container: `tavily-hikari`
  - persistent volume: `ai-tavily-hikari-data`
  - database paths inside the container:
    - `/srv/app/data/tavily_proxy.db`
    - `/srv/app/data/tavily_proxy-observability.db`
- Read-only inspection on 2026-06-21 showed the container healthy but the data files already large
  enough to amplify wide scans:
  - `tavily_proxy.db`: about `3.4G`
  - `tavily_proxy-observability.db`: about `408M`
  - `tavily_proxy.db-wal`: about `724M`
- A controlled in-container admin-style request to
  `http://127.0.0.1:8787/api/dashboard/overview` with the production forward-auth headers still
  took about `4.70s` on 2026-06-21.
- Production 101 grouped-alert reads hit a SQLite parser failure before the compat rewrite:
  `database error: (code: 1) near "(": syntax error`.
- Recent production logs from the same container still show overview-adjacent SQLite pressure, for
  example:
  - a retained-window aggregate over `observability.request_logs` logged at about `925ms`
  - a write into `observability.request_logs` logged at about `938ms`
  - a follow-up `SELECT request_kind_key, ... FROM request_logs WHERE id = ?` logged at about
    `1.03s`
- Treat this as the anti-pattern signature: if overview freshness, snapshot polling, or month-series
  reads keep touching `observability.request_logs` outside a minute-tail fallback, the read path
  will contend with live writes and grow with retained history.
- On 2026-06-24 a stricter in-container replay on the same stack regressed further:
  `curl -m 20 http://127.0.0.1:8787/api/dashboard/overview` timed out with
  `HTTP 000 TOTAL 19.999741`, while the real `/admin/dashboard` shell still rendered and multiple
  tiles stayed stuck on `正在加载仪表盘数据…`. That combination is the signal that the dashboard
  shared-snapshot path itself has become the bottleneck, not the shell or auth path.
- On 2026-07-22 the same production family regressed on another owner-facing surface:
  `/admin/analysis/rankings?tab=last24h` rendered the shell but stayed on `等待首帧快照`, while
  in-container `curl -m 5` to `/api/users/rankings`, `/api/summary`, `/api/summary/windows`, and
  `/api/analysis/pressure` all timed out with `0 bytes` even though
  `/api/stats/forward-proxy/summary` stayed fast. Logs around `2026-07-22 17:39` showed
  `database is locked`, `request_logs gc bootstrap schema` taking about `33s`, and slow
  `observability.request_logs` updates, which identified read-before-flush contention rather than
  a broken page shell.

## Guardrails / Reuse Notes

- A fast SSE cadence does not require the same cadence for database freshness. Keep a cheap dirty
  generation in memory, share one singleflight snapshot between HTTP and SSE, and enforce a minimum
  rebuild/probe interval. Last-good data is preferable to multiplying expensive reads during a
  writer incident.
- Partial indexes for retained operational logs should be created by an idempotent post-ready
  maintenance task. Add an `EXPLAIN QUERY PLAN` regression proving the bounded candidate query can
  use the index, and preserve last-good coverage while the index is pending.

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
- When an admin read path embeds `CASE` classification inside SQLite scalar or aggregate
  expressions, avoid adding wrapper parentheses around the `CASE` inside `COALESCE`,
  `COUNT(DISTINCT ...)`, `MIN(...)`, or `MAX(...)`. Production-grade older SQLite parsers can
  reject forms such as `COUNT(DISTINCT (CASE ...))`, `MIN((CASE ...))`, or
  `COALESCE((CASE ...), ...)` with `near "("` even when newer local SQLite builds accept them.
- Add query-plan regression tests for admin read hot paths when the fix depends on SQLite choosing a
  specific index. Local small databases may return quickly even when the planner would be disastrous
  on production data volume.
- When admin and public read paths share one rollup family, keep one freshness contract. Letting
  HTTP and SSE each invent separate “maybe flush” logic is an easy way to reintroduce duplicate
  scans and inconsistent first-paint latency.
- `COUNT(DISTINCT ...)` over request logs is especially prone to temp B-trees; keep its input
  cardinality small with user-first filtering and avoid running it over all visible rows in a recent
  time window for every admin refresh.
- Production stop-the-bleed actions such as single-container restart are live changes and require
  explicit owner approval.

## Bounded reconciliation and memory observations

- Candidate selection for reconciliation should fetch an indexed, bounded page before grouping or
  hydrating Research state. Admit that local preparation as bounded bulk work, release admission
  before remote I/O, keep the 12/8 recent/backlog fairness contract, and never run a global queue
  aggregate before the first remote attempt.
- A rejected reconciliation preparation admission is a typed deferred outcome, not a successful zero-work
  run. The scheduler must use a short control transaction to retain one durable representative with a fixed
  delayed wake before releasing the worker.
- Expose a bounded `ReconciliationObservation`: `hasEligible` and oldest-candidate age are precise
  for the bounded probe, while `queueEstimate=null` and `coverage=unknown` are required before the
  first observation. Never render an unknown queue as zero.
- When three rounds have candidates but no remote attempt and local budget exhaustion, persist one
  representative delayed job and apply a short local backoff. Keep that state separate from the
  `2/5/10/30` minute upstream-429 backoff, which only changes after real remote 429 attempts and
  honors the maximum Retry-After. This protects the SQLite worker from a one-minute no-op loop
  without hiding real progress.
- For in-memory owner-facing usage windows, retain only the last hour as events and aggregate older
  1–25 hour history into five-minute buckets. Backfill with 500-row pages and merge the captured live
  tail so a lock-held full-history copy is never required.

## References

- `src/store/key_store_request_logs_and_dashboard.rs`
- `src/store/key_store_token_logs.rs`
- `src/store/key_store_keys.rs`
- `src/store/key_store_alerts.rs`
- `src/forward_proxy/storage.rs`
