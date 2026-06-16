---
title: Backend deterministic time
module: tavily-hikari
problem_type: slow_backend_tests
component: backend-time
tags:
  - testing
  - rust
  - sqlite
  - tokio
  - performance
status: active
related_specs:
  - docs/specs/085cc-backend-deterministic-time/SPEC.md
  - docs/specs/3grrf-ci-backend-test-split/SPEC.md
  - docs/specs/2wdrp-sqlite-write-lock-hardening/SPEC.md
---

# Backend deterministic time

## Context

Backend tests were structurally slow because they waited on real wall-clock time for lock retries,
second-granularity ordering, polling loops, and startup-heavy schema paths. CI sharding reduced
queueing but could not remove those waits.

## Resolution

- Add one internal backend time seam and route behavior time through it.
- Keep production constants and persisted second-level timestamps unchanged.
- For tests that only care about ordering or freshness semantics, set explicit persisted timestamps
  instead of sleeping for one second.
- For tests that truly need controllable progression, use a local manual wall clock handle or a
  narrowly-scoped paused runtime path, but avoid freezing unrelated DB startup/pool behavior.
- If a “slow test” is actually dominated by startup/schema work, profile that path first; fix the
  hotspot before adding more test indirection.
- If CI shards run one exact test per process, treat that runner topology as a first-class
  performance problem. Batch safe prefix groups into a single test process and only fall back to
  exact-per-test execution for ambiguous filters.
- If one shard clearly benefits from modest intra-process parallelism while others do not, encode
  that policy per shard instead of raising `--test-threads` globally for the whole backend suite.
- If a shard still contains one interference-prone prefix, keep that prefix in an explicit
  `serial_prefixes` allowlist and continue parallelizing the rest of the shard; avoid all-or-nothing
  fallback to whole-shard serialization.

## Guardrails

- Do not claim a speedup by shrinking production retries or lock budgets.
- Do not use `ignore`, conditional skips, or alternate code paths for the fast test version.
- Do not blanket-apply `start_paused = true` to DB-heavy test classes; runtime-managed pool waits
  may freeze and produce misleading failures.
- Prefer direct state shaping over time sleeping when the assertion is about persisted values rather
  than actual asynchronous timer behavior.
- Do not switch shard runners back to raw `cargo test FILTER` substring matching unless coverage
  equivalence has been re-proven; keep any batching strategy constrained to filter sets whose
  substring match set is exactly equal to the intended prefix-owned tests.
