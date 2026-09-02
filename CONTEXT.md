# Tavily Hikari Context

## System Shape

Tavily Hikari is a single-product service with one owner-facing admin surface, one user-facing console, and one active business ingress. High availability is implemented as active/standby control around that single ingress, not as a distributed cluster manager.

## SQLite Workload Terms

- `foreground work`: request-path reads and writes, including administrator mutations whose request
  is the durable command. It uses an operation-specific bounded SQLite acquire and writer wait, so
  contention returns a retryable result rather than inheriting SQLite's default wait or competing
  indefinitely with background work.
- `maintenance control`: short durable queue metadata work such as claim, finish, continuation, and
  stale recovery. It has a fixed short pool/writer budget enforced by `SqliteRuntime`, never
  changes the database busy-timeout pragma, and never carries scans or remote I/O. A transient
  completion failure leaves its fenced claim running for type-specific stale recovery rather than
  dropping durable work or retrying indefinitely in the background.
- `maintenance bulk`: rebuilds, rollup persistence, GC, and local reconciliation projection. It
  obtains one instance-local admission permit only when two foreground pool slots are either idle
  or immediately allocatable within the configured pool maximum, foreground activity is at most
  five requests per second, and there was no recent SQLite contention. Request-stats flush is the
  bounded recovery exception: each nominal wake owns at
  most four adaptive `25..250` logical-key transactions within one 50ms retry budget, atomically
  restoring every uncommitted delta before yielding. The budget decides whether to acquire a
  connection and start another `BEGIN IMMEDIATE`; once a transaction starts, its runtime-owned
  commit or rollback resolves before the batch is classified. A timeout must never requeue a batch
  whose commit is still in flight.
- `recovery debt`: retained work that is safely eligible for automatic catch-up, including expired
  HA outbox events. It progresses through bounded work slices and never receives a special writer
  bypass.
- `deferred outcome`: a typed decision made before pool acquisition when admission rejects a bulk
  operation or when its bounded transaction sees transient writer contention. The affected durable
  work returns uncommitted deltas or records a retry time; unrelated eligible work remains free to
  progress.
- `admin privacy read controller`: an AppState-owned immutable privacy-status last-good value,
  observation time, and singleflight refresh flag. It schedules its dedicated SQLite snapshot only
  after ready and outside the HTTP response path. Snapshot acquire and `BEGIN` remain bounded to
  100ms through operation admission and a connection-local busy timeout, then close explicitly at
  a cooperative completion boundary; cached readers immediately
  receive `stale` coverage while a refresh is in flight or deferred. Shutdown fences new refreshes
  and waits for an active snapshot's explicit close. A deferred startup prewarm retries in the
  background until it publishes last-good or shutdown fences it. A true cold miss returns
  `503 Retry-After: 1` without opening a request-owned SQLite transaction.

## Reconciliation Terms

- `reconciliation controller`: the replicated control-plane state selected exclusively by the
  existing precise-reconciliation switch. `false` selects `compare`; writing `true` selects
  `active` immediately and records the next full business period as the billing boundary. A
  durable integrity failure selects `active_paused`, clears the legacy switch, and requires a new
  `true` write before actual billing can resume.
- `reconciliation mode`: the controller-derived durable-work policy selected for a run. `compare`
  observes upstream differences without changing billing truth; `active` applies the existing
  billing settlement rules only to work at or after its activation boundary; `active_paused`
  preserves work without scheduling a representative.
- `projection slice`: one resumable, claim-fenced historical-usage page. Its bounded read happens
  outside the write transaction. `ReconciliationProjection` source reads use a native SQLite
  progress-handler deadline; a deadline is a typed defer before merge and cursor advance. The
  handler is removed before the connection returns to the pool, otherwise that connection closes.
  Work merge and the stable keyset cursor advance commit atomically.
- `reconciliation read session`: one fresh SQLite snapshot for one preparation source `SELECT`.
  Recent/backlog candidates, candidate/billed-credit hydrate, Research candidates, and historical
  projection each receive the native 250ms deadline independently. A deadline is
  `projection_read_budget`: it stops later preparation and remote work and persists one
  claim-fenced 30-second continuation without changing work, settlement, or billing truth.
  Billed-credit hydrate is the pre-request source-read gate; settlement later reads current ledger
  state through a separately bounded finalization connection so charges written during HTTP remain
  visible without moving the native source deadline past the remote boundary. Claim, finish, and
  continuation remain `maintenance control` rather than part of this session.
- `reconciliation read-budget defer`: the `projection_read_budget` outcome emitted when a
  reconciliation read session reaches its SQLite deadline. It preserves work and billing truth,
  ends local preparation before another source read, projection slice, or remote attempt, records
  one same-claim delayed continuation for 30 seconds later, and is not a terminal reconciliation
  result.
- `terminal outcome`: a current work generation that needs no retry. Active non-zero differences
  are `settled`, zero differences are `no_adjustment`, and compare-mode non-zero differences are
  `observed`.
- `observation terminal`: the `observed` terminal outcome. It records shadow evidence while leaving
  billing truth unchanged and is never counted as a settlement.
- `retryable outcome`: `upstream_429`, `transport_failure`, `semantic_failure`, or
  `local_pressure`. It preserves the current work generation and its independent retry state until
  a later terminal outcome.
- `Key cooldown`: a `period_reconciliation`-scoped retry window written only for the upstream Key
  that returned `429`, using the `5/10/20/30` minute ladder and `Retry-After`. Non-cooling Keys keep
  progressing; if every eligible Key is cooling, the representative defers to the earliest expiry.
  Legacy global-backoff meta is compatibility data, not a live gate or wake source.
- `missing eligible upstream key`: a durable nonterminal input condition. It records a fixed
  fifteen-minute retry without incrementing semantic or transport failure state, and administrators
  see only its aggregate count.
- `partial key observation`: a node-local, generation-scoped successful upstream usage response for
  one key in a multi-key candidate. It is rebuildable diagnostic state, never a terminal result or
  HA outbox truth. The engine requests at most two missing keys per run and cannot sum or complete
  the candidate until every current-generation key is observed.
- `remote attempt budget`: the typed nonterminal continuation used when a candidate still has missing
  keys after the two-request run cap. It schedules one claim-fenced representative 30 seconds later
  without incrementing semantic, transport, 429, or local-pressure streaks.
- `transport failure kind`: a fixed, non-sensitive category (`connect`, `timeout`,
  `response_body`, `invalid_endpoint`, `credentials_or_database`, or `unknown`) attached to the
  local reconciliation observation. It is diagnostic state, never a terminal result or billing
  decision.
- `remote attempt lease`: the instance-owned one-at-a-time admission around an outbound upstream
  HTTP request only. It excludes local projection, candidate hydration, durable finalization, and
  Research bookkeeping. Manual work retains priority; after 120 eligible seconds, automatic main
  reconciliation and the Research drain compete for the next non-manual request turn. Main uses
  its runnable `available_at`; Research uses its durable `queued_at` debt anchor so a defer cannot
  reset its wait. Main wins an exact tie. A turn is consumed only when HTTP begins. After an
  accepted Research `remote_lease` continuation, its aged turn remains reserved until that resumed
  request starts; ordinary automatic remote work may prepare locally but cannot acquire its lease.
- `foreground_rps`: the instance-local recent request-rate heuristic used to protect foreground
  traffic. It is not a CPU, SQLite-pool, cgroup, or host-load metric. A non-aged Research drain
  defers above five requests per second; an aged Research turn bypasses only this heuristic for
  one bounded poll and still requires SQLite admission, the request lease, and a claim fence.
  Its durable `scheduled_jobs.queued_at` fairness anchor survives foreground, lease, read-budget,
  and control defers; an accepted poll or Key cooldown begins a new interval.
- `research selection page`: an indexed, due-only page of at most 80 Research rows, hydrated in
  bounded batches with a four-per-key and 20-row sweep cap. Its stable keyset cursor advances only
  after claim-fenced acceptance of an actually processed candidate; read pressure or cancellation
  leaves the page retryable. Eligibility is resolved before the page limit. A five-minute forced
  wrap rediscovers rows that become eligible behind the cursor, and accepting that wrap atomically
  starts the next sweep interval.
- `Research drain`: the unique durable `upstream_reconciliation_research_drain` representative that
  owns terminal Research polling and the v21 scan cursor. It performs at most one request every five
  seconds, commits the poll result, per-Key cooldown, exact cursor, and claim fence atomically, and
  never consumes the main reconciliation run's two-request budget. Main settlement and the drain
  share only the instance-wide request-scoped remote lease. Outcome counters are emitted only after
  one accepted receipt has atomically recorded the row result, Key state, cursor, progress window,
  claim finish, and next representative. Its no-request continuations are explicit:
  `foreground_pressure` and `read_budget` retry after 30 seconds, `remote_lease` after five
  seconds, and `control_defer` after 30 seconds; none changes cursor, outcome, Key state, or
  billing truth.
- `Research poll resolution`: a durable classification of a nonterminal Research row. `pollable`
  means the row may be selected by the drain; `unavailable` records a confirmed 404 and suppresses
  repeated polling while leaving `terminal_at` unset, preserving the existing 24-hour degraded
  protection for main reconciliation. A poll resolution is not a billing result and never changes
  actual billing truth.
- `Research credentials cooldown`: a six-hour cooldown in the
  `reconciliation_research_credentials` scope for one upstream Key after a 401/403 or an empty
  local secret. It is independent from the `period_reconciliation` 429 cooldown; healthy Keys keep
  progressing, and a successful 202 or terminal poll clears only the credentials scope.

## Observability Boundaries

- `foreground truth`: billing, request handling, and control-plane changes whose durable result is
  required before a request can succeed. They do not depend on observability persistence.
- `derived observability`: pressure buckets and alert/read diagnostics that may be stale because
  their source records can rebuild them. They use instance-local bounded queues and low-priority
  SQLite admission rather than holding a foreground request on the writer.
- `best-effort audit`: rebalance audit records are capped in memory and may report stale coverage
  when contention or capacity prevents persistence. A missing audit record never changes MCP
  response semantics, billing truth, or durable business work. The instance-owned writer debounces
  one second, commits at most ten audit rows in one transaction, atomically requeues an uncommitted
  batch, and backs off only sustained defers.
- `connection-scoped SQLite pages`: SQLite `CACHE_WRITE` page deltas sampled at operation-connection
  boundaries. They may attribute an operation's SQLite cache writes. Process and cgroup write-byte
  counters remain aggregate pressure labels and must not be presented as one query's writes.
- `SQLite write attribution sample`: a low-frequency, non-invasive diagnostic that records
  connection-scoped page deltas separately from database and WAL file state. It may identify a
  correlation, but never attributes process or cgroup I/O to one query and never changes SQLite
  checkpoint, WAL, or pragma behavior. File metadata is limited to the configured core and
  observability databases and their WAL files; it is never a checkpoint or directory scan.
- `staged pressure generation`: a source-fenced server-pressure rebuild generation that remains
  invisible until atomic publish. Source scans use 500-row keyset slices, transition events replay
  after publish, and obsolete generations are cleaned in 25-row slices so live-tail correctness
  does not require a whole-table replacement.

## Dashboard Read Terms

- `DashboardReadModel`: the single AppState-owned immutable last-good overview snapshot. A dirty
  generation may request one shared rebuild every ten seconds; a sixty-second bounded safety probe
  catches missed invalidations. Requests under SQLite pressure return last-good coverage rather
  than starting an independent rebuild. Quota sample freshness uses append-only primary-key/time
  watermarks and a payload build reuses the watermark already read by its triggering probe.
- `AlertProjection`: an observability-sidecar projection with separate stable cursor/fence lanes:
  the recent tail serves Dashboard and the historical lane serves administrator Events and Groups.
  Dashboard accepts a recent summary only at `recent_coverage=ok`; otherwise the read model retains
  last-good data. Administrator reads use the sidecar only at complete history coverage and keep an
  exact-query last-good cache for transient pressure. A cold or expired key returns an explicit
  retryable response instead of starting an expensive raw alert CTE, so incomplete history is never
  shown as empty.
- `idle alert probe`: a source-fence check that finds no work. It is not projection progress: it
  never advances a cursor or generation, and a separate low-frequency observation heartbeat keeps
  recent-tail coverage explicit.

## HA Terms

- `full_master`: the only node allowed to handle full writes.
- `provisional_master`: a node that already owns ingress traffic but is still write-fenced for high-risk admin/business mutations.
- `standby`: a synced node that does not serve external traffic.
- `recovery`: an old master that lost ingress authority and must not take traffic back until recovery work completes.
- `planned cutover`: an operator-initiated maintenance cutover from the current `full_master` to one eligible standby candidate.
- `standby_candidate`: the only peer role hint that may receive a planned cutover in the current release.
- `observer`: a peer shown in the control plane for visibility only; it never becomes a cutover target in this release.

## Control Plane Boundaries

- The HA control plane is single-surface and active-led.
- The current `full_master` is the only node allowed to initiate `planned cutover`.
- Peer inventory is runtime-configured through `HA_PEER_NODES_JSON`; there is no UI editing path in this release.
- Timeline truth for operator-visible HA actions lives in `ha_control_plane_events` and retains only the last 7 days.

## Current Release Constraints

- Multi-node UI and model support are intentionally ahead of multi-standby orchestration support.
- At most one peer may be marked `standby_candidate`.
- `planned cutover` targets must be recently observed, synced, not stale, not recovering, and currently in `standby`.
- Emergency/manual failover remains available through local `promote` and `finalize`.
