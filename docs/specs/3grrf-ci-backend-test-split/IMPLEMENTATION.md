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
- `scripts/ci_backend_test_manifest.json`
  - 固化当前 lib / main-bin / support-bin / integration test 的 shard 归属。
  - coverage verifier 已证明当前 union 覆盖 `354 lib + 325 main-bin + 24 support-bin + 20 integration` tests，无 overlap、无 unmatched。
- `scripts/ci_backend_tests.py`
  - `verify`：基于 `cargo test -- --list` 做 shard 覆盖等价校验并导出 matrix。
  - `run-shard`：不再依赖 `cargo test FILTER` / `--skip FILTER` 的子串匹配，而是先生成 test executable，再按精确测试名列表用 `--exact` 直跑对应 test binary，避免重复命中或漏跑。
- `src/server/spa.rs` + `src/server/tests/chunk_15.rs`
  - 修复 `registration-paused.html` 在 embedded web assets 开启时误覆盖本地静态 fallback 的回归，并补上 targeted regression test，确保 shard 后的 release/build path 保持原有行为。

## Verification Evidence

- 本地 shard coverage 验证：
  - `python3 scripts/ci_backend_tests.py verify`
  - 结果证明当前 manifest 覆盖 `354 lib + 325 main-bin + 24 support-bin + 20 integration` tests，无 overlap、无 unmatched。
- 代表性本地 shard 复现：
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-account-user`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-request-rollup`
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-admin-api`
  - 结果表明最后几个慢 shard 主要由一串 `12-18s` 级顺序慢测试组成，而不是执行器挂死。
- GitHub PR run 证据：
  - PR `#317` / head `a0ff34307cf8a81836bf75831ea24a6e13ad170c`
  - `CI Pipeline` run `27100670939`
  - 所有 shard、稳定 `Backend Tests` aggregate gate、`Compose Smoke (ForwardAuth + Caddy)`、`Build (Release)`、`Lint & Checks`、`Frontend Checks`、`Web Assets` 均成功。
  - PR 当前 `mergeStateStatus=CLEAN`、`mergeable=MERGEABLE`

## Remaining Gaps

- 当前实现范围内无阻断缺口。
- 若后续继续压缩墙钟时间，可在新主题里进一步细分 `lib-account-user`、`lib-request-rollup`、`bin-admin-api` 这类重 shard，但这不属于当前主题的必需收口项。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
