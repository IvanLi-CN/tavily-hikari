# GitHub Actions 后端测试拆分与并行提速 实现状态（#3grrf）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: completed
- Lifecycle: done
- Catalog note: CI backend split / artifact reuse / stable aggregate gate

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

- `.github/workflows/ci.yml`
  - 新增 `Backend Shard Plan` job，在 CI 内先执行 shard coverage verifier，再导出 lib / bin / integration 三类 matrix。
  - `backend-lib-tests`、`backend-bin-tests`、`backend-integration-tests` 已切到 manifest 驱动的 matrix shards。
  - 稳定 owner-facing `Backend Tests` aggregate gate 继续保留。
  - `Compose Smoke (ForwardAuth + Caddy)` 与 `Build (Release)` 不再等待整段 backend shards 收口后才启动。
  - `backend-shard-plan` 现会先准备一次 `backend-test-artifacts`，后续 lib / bin / integration shards 统一下载并复用，不再各自重复 `cargo test --no-run`。
- `scripts/ci_backend_test_manifest.json`
  - 固化当前 lib / main-bin / support-bin / integration test 的 shard 归属。
  - `forward_proxy::tests::` 已从 `lib-core` 拆成独立 `lib-forward-proxy` shard；`bin-admin-api` 则保留 shard 级 `filtered_process_workers=2` 的保守并发策略。
  - 当前 selector 已切到真实测试模块前缀，例如 `tests::request_rollup::...`、`tests::usage_series_and_backfills::...`、`server::tests::tavily_http_search::...`、`server::tests::mcp_billing_and_sessions::...`，不再依赖旧的平铺 `tests::foo_` / `server::tests::bar_` 前缀去猜测测试归属。
  - coverage verifier 已证明当前 union 覆盖 `396 lib + 334 main-bin` tests，且 `bin-support` 与 5 个 integration suites 全覆盖，无 overlap、无 unmatched。
- `scripts/ci_backend_tests.py`
  - `verify`：基于 `cargo test -- --list` 做 shard 覆盖等价校验并导出 matrix。
  - `run-shard`：不再依赖 `cargo test FILTER` / `--skip FILTER` 的子串匹配，而是先生成 test executable，再按精确测试名列表用 `--exact` 直跑对应 test binary，避免重复命中或漏跑。
  - `prepare-artifacts`：一次性构建全覆盖 test targets，再按 coverage target 拆分 artifact，并缓存每个 executable 的 `tests.json` 供 `verify --prebuilt-root` / `run-shard --prebuilt-root` 复用。
  - 现支持 shard 级 `filtered_process_workers`，在不破坏覆盖等价性的前提下允许重 shard 局部降并发，避免为稳定性回退到整 shard 串行。
- `src/tests/mod.rs` + `src/tests/support.rs` + `src/tests/**`
  - 库内测试已从 `include!(\"chunk_*.rs\")` 机械切片切回真实语义模块，并把共享 helper 显式收敛到 `support` 层。
  - `src/tests/chunk_04_tail.rs` 已删除；observability / oauth / announcement / coalescer 等测试迁入语义模块文件。
- `src/server/tests.rs` + `src/server/tests/**`
  - server/bin 测试已从单个 `mod tests { include!(...) }` 机械聚合改为真实模块树，并通过 `core_support_and_parsing`、`upstream_support_and_manual_jobs` 等显式 support helper 保持复用关系。
- `src/server/spa.rs` + `src/server/tests/admin_token_filters_and_maintenance.rs`
  - 修复 `registration-paused.html` 在 embedded web assets 开启时误覆盖本地静态 fallback 的回归，并补上 targeted regression test，确保 shard 后的 release/build path 保持原有行为。

## Verification Evidence

- 本地 shard coverage 验证：
  - `python3 scripts/ci_backend_tests.py verify`
  - 结果证明当前 manifest 覆盖 `396 lib + 334 main-bin` tests，且 `bin-support` 与 5 个 integration suites 全覆盖，无 overlap、无 unmatched。
- 代表性本地 shard 复现：
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-account-user`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-request-rollup`
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-admin-api`
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-mcp-core`
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
