# ADR: Backend deterministic time over more CI sharding

## Status

Accepted

## Context

Backend CI had already been split by `3grrf`, but representative slow tests still spent wall-clock
time sleeping for lock waits, second-granularity ordering, polling windows, and startup-heavy
schema paths. Adding more shards would hide some latency but would not remove the underlying
real-time dependency from the tests themselves.

At the same time, production behavior depends on existing retry/backoff budgets and on second-level
persisted timestamps such as `last_used_at`, `created_at`, and `updated_at`. Changing those
production constants or forcing schema precision upgrades just to make tests faster would couple
test ergonomics to product semantics.

## Decision

- Introduce a single backend time seam in `src/backend_time.rs`.
- Keep public constructors and external contracts stable; add internal/test-only injection points
  such as `KeyStore::new_with_time` and `TavilyProxy::with_options_and_time`.
- Preserve production constants and second-level persisted timestamp fields.
- Prefer explicit persisted timestamps or local manual clock control in tests over real wall-clock
  sleeps.
- Fix structural startup hotspots when they are the real source of test slowness; do not paper over
  them with more CI sharding.

## Consequences

- Tests can remove real-time waits without changing production retry semantics.
- Coverage evidence remains honest because the real code paths still run.
- Some historical direct `Utc::now()` / `tokio::time::sleep(...)` calls will need incremental
  migration; this is acceptable as long as the seam becomes the only allowed direction for new code.
- Paused-runtime testing must be applied surgically. DB-heavy tests that rely on runtime-driven
  connection/pool timing cannot be blanket-switched to `start_paused = true`.
