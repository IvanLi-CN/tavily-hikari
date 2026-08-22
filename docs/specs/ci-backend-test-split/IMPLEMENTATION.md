# GitHub Actions 后端测试拆分与并行提速 实现状态（#3grrf）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: in progress
- Lifecycle: active
- Catalog note: CI backend lanes, checksum-addressed artifacts, and stable aggregate gate

## Coverage / rollout summary

- PR1 目标：
  - backend semantic shards
  - single-build `web/dist` artifact reuse
  - stable `Backend Tests` aggregate gate
  - `Compose Smoke` / `Build (Release)` critical-path unblock
- PR2 目标：
  - shard manifest + coverage verifier
  - lib / bin test job-matrix parallelization without reducing test count

## Implemented Now

- CI five-minute backend path
  - `Backend Shard Plan` no longer waits for `web-assets`; it creates the minimal embedded-web
    fixture, compiles all backend targets with `ci-test`, verifies coverage, uploads one backend
    bundle, and exports a sixteen-lane matrix within the fixed maximum.
  - The plan restores `target/ci-test` with a platform, Rust toolchain, profile/linker, and
    `Cargo.lock` cache key. It retains Cargo fingerprint invalidation for changed source and keeps
    the cached compilation state separate from the per-run, checksum-verified backend bundle.
  - `Backend Test Lane` jobs now download, checksum-verify, and run a lane from the self-contained
    bundle. The bundle includes a read-only source snapshot for tests that use the compile-time
    manifest directory; the former lib/bin/integration jobs each repeated frontend artifact
    downloads, system packages, Rust setup, and Cargo cache restoration.
  - `Backend Tests` remains the stable aggregate check. The target is the native Actions interval
    from plan start to aggregate completion, with no new required performance check.
  - Unrelated heavy jobs keep their existing commands but wait for the backend aggregate, leaving
    the hosted runner concurrency available for the fixed sixteen backend lanes. This changes only
    job ordering; it does not alter release, lint, or version-layer validation semantics.
- Artifact and lane contracts
  - `ci-test` inherits `test` while disabling debug information and incremental compilation.
  - `build.rs` can consume `TAVILY_HIKARI_WEB_DIST_DIR`; CI supplies the minimal fixture while the
    normal `web/dist` fallback is unchanged.
  - The bundle writer emits format 2 with one checksum-addressed copy per executable. Filenames
    preserve each source binary's prefix so Cargo sibling-binary resolution works without adding a
    second copy. The loader supports both formats and verifies SHA-256 before execution.
  - `lib-request-rollup`, `lib-account-user`, `bin-admin-api`, and `bin-ha-rest` are split into
    mutually exclusive semantic groups. Rollup integrity, alert projection, and reconciliation
    are independent groups so measured long tails can be packed separately. Manifest estimates
    feed stable LPT lane packing.
  - Reconciliation coverage is divided into maintenance, upstream-reconciliation, and projection
    groups. This keeps the long upstream test binary from queueing behind unrelated reconciliation
    prefixes while the coverage verifier continues to reject overlap or omissions.
  - Request rollup storage is divided into rollup storage, request-log retention, and scheduled
    request-maintenance groups. Admin resources are divided into identity, observability, and
    settings groups. The manifest contract tests preserve the old prefix unions, so the split can
    only alter lane placement, not test ownership.
  - The admin dashboard SSE refresh test is an independent single-process, single-thread shard.
    Identity excludes that exact name, preserving one owner while preventing unrelated admin API
    filters from delaying its event deadline. Operational maintenance has a four-process cap; all
    other shards retain their explicit cap or the default of three.
  - MCP main-binary coverage is split into mutually exclusive billing, rebalance-session,
    rebalance-control, research, and system groups so long semantic prefixes cannot share one
    lane tail. Research and system groups retain their serial process boundaries.
  - Manifest weights are calibrated from native shard timings; affinity, reconciliation, LinuxDo,
    reporting, and server HTTP contract retain enough weight to keep the sixteen-lane LPT output
    within the 120-second lane budget after semantic splits.
  - HA lifecycle coverage is divided into mutually exclusive lifecycle and lifecycle-state groups;
    this keeps the long HA prefix family from becoming a single atomic lane tail.
  - `prepare-artifacts` logs compilation, test-list discovery, and bundle-staging elapsed markers.
    They make a cold-compile regression attributable without turning elapsed time into a CI gate.
    Backend artifact upload uses a lower compression level to reduce CPU time on the critical path;
    artifact metadata remains subject to the existing size budget.
- Development execution
  - `run-all`, `run-shard`, and `run-lane` accept Cargo-job, filtered-process-worker, and
    filtered-test-thread controls. The local default is `2/1/2`; `--diagnostic` is `1/1/1`.
  - Pre-commit commands now run sequentially and keep Clippy as
    `cargo clippy --locked -j 2 -- -D warnings`.

- `src/tests/mod.rs` + `src/tests/support.rs` + `src/tests/**`
  - 库内测试已从 `include!(\"chunk_*.rs\")` 机械切片切回真实语义模块，并把共享 helper 显式收敛到 `support` 层。
  - `src/tests/chunk_04_tail.rs` 已删除；observability / oauth / announcement / coalescer 等测试迁入语义模块文件。
- `src/server/tests.rs` + `src/server/tests/**`
  - server/bin 测试已从单个 `mod tests { include!(...) }` 机械聚合改为真实模块树，并通过 `core_support_and_parsing`、`upstream_support_and_manual_jobs` 等显式 support helper 保持复用关系。
- `src/server/spa.rs` + `src/server/tests/admin_token_filters_and_maintenance.rs`
  - 修复 `registration-paused.html` 在 embedded web assets 开启时误覆盖本地静态 fallback 的回归，并补上 targeted regression test，确保 shard 后的 release/build path 保持原有行为。

## Current Validation

- `python3 scripts/test_ci_backend_tests.py`: passed eleven runner contract tests covering format 2
  checksum verification, format 1 loading, deduplicated references, portable lane runtime, stable
  LPT output, minimal web fixture, and resource defaults.
- `actionlint .github/workflows/ci.yml`, `cargo fmt --all -- --check`, Markdown formatting, and
  `cargo clippy --locked -j 2 -- -D warnings`: passed.
- `cargo test --locked -j 2 --all-features --lib runtime_logging::tests::runtime_memory_helpers_parse_status_and_cgroup_values -- --exact --test-threads=2`: passed.

## Historical Evidence

The following evidence describes the earlier three-matrix topology. It remains useful for comparing
behavioral coverage, but it is not evidence for the current lane topology or five-minute objective.

- 本地 shard coverage 验证：
  - `python3 scripts/ci_backend_tests.py verify`
  - 结果证明当前 manifest 覆盖 `396 lib + 334 main-bin` tests，且 `bin-support` 与 5 个 integration suites 全覆盖，无 overlap、无 unmatched。
- 代表性本地 shard 复现：
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-account-user`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-request-rollup`
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-admin-api`
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-mcp-billing`
  - 结果表明最后几个慢 shard 主要由一串 `12-18s` 级顺序慢测试组成，而不是执行器挂死。
- build-once fanout 本地证据：
  - `python3 scripts/ci_backend_tests.py prepare-artifacts --output-dir /tmp/backend-test-artifacts`：当前墙钟约 `70.71s`
  - `python3 scripts/ci_backend_tests.py verify --prebuilt-root /tmp/backend-test-artifacts`：通过
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-admin-api --prebuilt-root /tmp/backend-test-artifacts`：当前墙钟约 `94.03s`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-request-rollup --prebuilt-root /tmp/backend-test-artifacts`：当前稳定墙钟约 `62.95s`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-core --prebuilt-root /tmp/backend-test-artifacts`：当前稳定墙钟约 `38.61s`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-forward-proxy --prebuilt-root /tmp/backend-test-artifacts`：当前稳定墙钟约 `46.06s`
  - `python3 scripts/ci_backend_tests.py benchmark --max-workers 8`：当前稳定墙钟约 `169.35s`
- GitHub PR run 证据：
  - PR `#317` / head `a0ff34307cf8a81836bf75831ea24a6e13ad170c`
  - `CI Pipeline` run `27100670939`
  - 所有 shard、稳定 `Backend Tests` aggregate gate、`Compose Smoke (ForwardAuth + Caddy)`、`Build (Release)`、`Lint & Checks`、`Frontend Checks`、`Web Assets` 均成功。
  - PR 当前 `mergeStateStatus=CLEAN`、`mergeable=MERGEABLE`

## Remaining Gaps

- 当前 backend benchmark 已压到 `169.35s`，但单次 `cargo test --locked --all-features` 仍会串行执行 lib 与 main-bin 两大 test binary；若未来继续优化 owner 本地全量墙钟，应继续从 deterministic time 与 test binary 级别并发，而不是退回 substring runner。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
