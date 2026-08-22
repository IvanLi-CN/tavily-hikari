# ADR 0002: Test Execution Targets Stay Environment Neutral

## Status

Accepted

## Context

The backend suite requires substantially more CPU, memory, disk IO, and elapsed time than a
single focused test. The repository is used from environments with different capacity and does
not own the capacity, cache policy, or scheduling of every execution target.

The CI critical path must retain full backend coverage while avoiding duplicate frontend builds,
duplicate test executables, and repeated toolchain setup on every test shard.

## Decision

- Repository commands describe resource bounds and test semantics, not a particular host or
  scheduler.
- Full backend execution, release builds, and Compose checks are heavy targets. The caller selects
  where to run them before invocation; when that target is unavailable, the caller reports the
  missing validation instead of moving the work to another machine automatically.
- `scripts/ci_backend_tests.py run-all` and `run-shard` default to two Cargo jobs, one filtered
  process worker, and two filtered test threads. `--diagnostic` fixes all three limits at one.
- CI uses the `ci-test` Cargo profile, a minimal embedded-web fixture, a checksum-verified
  executable bundle, and balanced lanes. Full web asset production remains independently checked
  by the frontend build job.
- CI restores the `ci-test` compilation directory with a key that fixes the platform, Rust
  toolchain, profile/linker configuration, and `Cargo.lock`. Cargo fingerprints remain the source
  of truth for rebuilding changed project code; the cache never replaces the checksum-verified
  backend test bundle.
- External execution capacity, cache capacity and retention, and scheduling remain outside
  repository configuration.

## Consequences

- Developers have a low-pressure default for focused and full backend verification without
  weakening the pre-commit quality gate.
- CI can compile once and execute prebuilt binaries without installing a Rust toolchain in each
  lane.
- Documentation and agent instructions remain portable across execution environments.
