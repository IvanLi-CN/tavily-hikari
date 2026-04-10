# 修复合并后漏发的 post-merge 发布阻塞（#9rdxm）

## 状态

- Status: 已完成
- Created: 2026-04-10
- Last: 2026-04-10

## 背景 / 问题陈述

- `PR #227` 于 2026-04-10 03:08（Asia/Shanghai）合入 `main` 后，`CI Pipeline` run `#24208388945` 在 `Backend Tests` 失败，导致同一 merge SHA 的 `Release` run `#24208510515` 被跳过。
- 当前 `main` 对应的最新正式版仍停留在 `v0.37.0`，而 `445a80f87b42ca1eccb60520a443d09326287f95` 依照既有 `type:minor + channel:stable` intent 本应发布为 `v0.38.0`。
- 失败最初集中在 `src/bin/mcp_session_delete_neutral_repair.rs` 的 `auth_candidates_include_standalone_rows_matched_by_error_text`；随后又在本地高频循环中复现了同类 standalone 漏匹配与 `failure_kind IS NULL` sibling regression，说明仅做第一轮 direct DB seeding 仍不足以让这条 repair 测试链路稳定收敛。

## 目标 / 非目标

### Goals

- 让 `mcp_session_delete_neutral_repair` 的 standalone auth regression 测试稳定、可重复，不再在 `main` CI 上偶发漏匹配。
- 保持 `load_auth_token_log_candidates` 对 standalone auth rows 的既有语义不变，优先修复测试搭建而不是顺手改业务行为。
- 以 `type:skip + channel:stable` 的修复 PR 恢复 `main` 的 post-merge 可发布前置条件。
- 在修复合入且 `main` CI 恢复后，对 `445a80f87b42ca1eccb60520a443d09326287f95` 执行 `Release workflow_dispatch(head_sha=...)` 回填，并验证 `v0.38.0` 落地。

### Non-goals

- 不修改 semver 计算规则、release label 策略或 PR 阶段门禁编排。
- 不把本次 infra/test 修复本身作为新的产品 release 发布。
- 不修改 Rust/Web 对外接口、日志口径或 MCP/HTTP 业务行为。

## 范围（Scope）

### In scope

- `src/bin/mcp_session_delete_neutral_repair.rs`
- `docs/specs/README.md`
- `docs/specs/9rdxm-post-merge-release-unblock/SPEC.md`
- GitHub PR / CI / Release run bookkeeping（用于 merge + backfill 验证）

### Out of scope

- `.github/workflows/release.yml` / `.github/workflows/ci.yml` 的规则变更
- release 版本号策略与标签规范
- 任何用户可见 UI 或 API 行为改动

## 需求（Requirements）

### MUST

- `auth_candidates_include_standalone_rows_matched_by_error_text` 必须改成最小直接 DB seeding：只写入该测试所需的 `auth_tokens` / `auth_token_logs` 前置数据，不再依赖 `create_access_token` 的广义 runtime 初始化链路。
- 若同类 standalone auth regression 仍共用同一不稳定 setup，则应一并切到 direct DB seeding，确保 harness 行为一致。
- 修复后必须通过至少一次 `cargo test --locked --all-features`，并完成 `50` 次 `cargo test --bin mcp_session_delete_neutral_repair -q` 循环且无同类断言失败。
- 修复 PR 必须使用 `type:skip` + `channel:stable`，明确保持“修 infra/test，不额外发产品版本”的边界。
- 修复 PR 合并后，必须对 `445a80f87b42ca1eccb60520a443d09326287f95` 执行 `Release` 的 `workflow_dispatch(head_sha=...)` 回填；若执行时 `v0.38.0` 已存在，则保留验证结果并记录跳过原因。
- 本 spec 最终必须记录：失败 run、修复 PR、回填 run、最终 release URL，以及“为什么这次没有改 release 版本/label 规则”的边界说明。

### SHOULD

- 优先把 flaky 根因限制在测试 harness 层，避免因为未复现的 CI-only 波动去扩大 `load_auth_token_log_candidates` 的查询变更面。
- 若 review / CI 收敛阶段出现新的高价值阻塞项，应在同一 PR 内按根因批量修复，避免多轮无意义 push。

### COULD

- 若修复后仍能复现 standalone auth row 漏匹配，可补一条更窄的 targeted regression，专门锁定 `request_log_id IS NULL` 且仅依赖 `auth_token_logs.error_message` 的候选命中路径。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 本地 / CI 执行 `cargo test --bin mcp_session_delete_neutral_repair` 时，standalone auth regression 直接 seed 一个最小合法 `auth_tokens` row，再写入 `request_log_id IS NULL` 的 `auth_token_logs` row；测试 harness 使用单连接 SQLite pool，查询侧对 standalone / joined auth rows 走显式分支，随后稳定命中 `load_auth_token_log_candidates`。
- 修复 PR 合入 `main` 后，新的 `CI Pipeline` push run 必须通过，恢复 release workflow 的自动前置条件。
- 在 `main` 恢复健康后，对旧 merge SHA `445a80f87b42ca1eccb60520a443d09326287f95` 发起 `workflow_dispatch` 回填，使漏掉的 stable release 重新生成 `v0.38.0`、对应 GitHub Release 与 GHCR stable tags。

### Edge cases / errors

- 若本地仍无法复现旧失败，不得以“本地全绿”否定 GitHub 失败；根因判断应继续以 GitHub main CI 事实与循环验证结果为准。
- 若 `workflow_dispatch` 时仓库已存在 `v0.38.0`，不得重复创造新稳定版本；应记录 release 已存在并只补齐验收证据。
- 若修复 PR 合入后 `main` CI 仍失败，则必须继续在同一快车道里修复到 `main` 健康为止，不能直接转为 release backfill。

## 验收标准（Acceptance Criteria）

- Given `auth_candidates_include_standalone_rows_matched_by_error_text` 在 GitHub main CI 曾出现 `left: 0, right: 1`
  When regression 改为 direct DB seeding 并重新执行验证
  Then `cargo test --bin mcp_session_delete_neutral_repair -q` 的 50 次循环中不得再出现该断言失败。
- Given 修复分支与合入后的 `main`
  When 执行 `cargo test --locked --all-features`
  Then 结果必须通过，且不得引入新的 release-only 规则改动。
- Given merge SHA `445a80f87b42ca1eccb60520a443d09326287f95`
  When `Release` workflow 以 `workflow_dispatch(head_sha=445a80f87b42ca1eccb60520a443d09326287f95)` 回填
  Then 仓库必须可见正式版 `v0.38.0`（或已有同版本 release 的幂等结果），并能定位到 GitHub Release 链接、GHCR `latest` / `v0.38.0` 标签与 PR #227 release comment。
- Given 本次修复范围
  When 全流程结束
  Then 修复 PR 本身不得额外产生产品 release，且 spec/README 必须说明原因是沿用 `type:skip + channel:stable` 的既有边界，而非修改 release 规则。

## 实现前置条件（Definition of Ready / Preconditions）

- live incident 已冻结：`PR #227`、merge SHA `445a80f87b42ca1eccb60520a443d09326287f95`、`CI Pipeline #24208388945`、`Release #24208510515`、最新 release `v0.37.0`、目标回填版本 `v0.38.0`。
- 当前修复策略已锁定为 fast-track / merge+cleanup；允许自动推进 push、PR、review-loop、merge 与 release backfill。
- `type:skip + channel:stable` 已确认为本次修复 PR 的默认 label 组合。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cargo test --bin mcp_session_delete_neutral_repair -q`
- `for i in {1..50}; do cargo test --bin mcp_session_delete_neutral_repair -q; done`
- `cargo test --locked --all-features`

### UI / Storybook (if applicable)

- Not applicable.

### Quality checks

- `cargo fmt --check`
- `git diff --check`
- PR-stage review-loop（`gpt-5.4`, `pr-convergence`）

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增本 spec 索引，并在 merge/backfill 完成后同步状态、日期与结果说明
- `docs/specs/9rdxm-post-merge-release-unblock/SPEC.md`: 记录本次 flaky 修复、验证结果、PR 编号、release backfill run 与最终 release URL

## 计划资产（Plan assets）

- Directory: `docs/specs/9rdxm-post-merge-release-unblock/assets/`
- Visual evidence source: 本计划为非 UI / 非视觉交付面修复，无需 owner-facing 截图

## Visual Evidence

本次为测试与发布流程收敛，不要求视觉证据。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 incident 事实，并建立 spec / README 索引
- [x] M2: 把 standalone auth regression 切到 direct DB seeding，去掉不必要的 runtime token 创建依赖
- [x] M3: 完成本地验证、PR-stage review-loop、修复 PR 合并与 main CI 恢复
- [x] M4: 完成 `445a80f87b42ca1eccb60520a443d09326287f95` 的 stable release backfill，并把最终 run / URL 回填到 spec

## 方案概述（Approach, high-level）

- 先把 flaky 行为收缩到最小测试 harness：schema 初始化仍沿用现有测试入口，但 token row 直接写库，避免额外依赖 `create_access_token` 的随机生成与 runtime 侧效应。
- 验证以 `repair binary` 循环与全量 `cargo test --locked --all-features` 双轨收口，确保不是只把单条用例临时修绿。
- 发布恢复继续沿用现有 label-driven release 规则，只通过 `type:skip` 修复 PR 恢复 main 健康，再以 `workflow_dispatch` 幂等回填漏发版本。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若根因不在 harness，而在 `load_auth_token_log_candidates` 的 CI-only 查询边界，可能仍需追加一轮更窄的查询加固。
- 风险：若在下一次 stable release 之前未完成 backfill，`v0.38.0` 的目标版本可能被后续 stable merge 占用。
- 假设：`Release workflow_dispatch` 继续以当前 default-branch workflow 定义执行，并对既有 merge SHA 提供幂等 backfill 语义。

## Incident / Release ledger

- Merged PR: `#227`
- Merge SHA: `445a80f87b42ca1eccb60520a443d09326287f95`
- Failing CI run: `#24208388945`
- Skipped release run: `#24208510515`
- Latest stable before recovery: `v0.37.0`
- Target backfill version: `v0.38.0`
- Recovery CI run: `#24227470494`
- Fix PRs: `#229`, `#230`
- Backfill release run: `#24227733663`
- Final release URL: [v0.38.0](https://github.com/IvanLi-CN/tavily-hikari/releases/tag/v0.38.0)
- PR #227 release comment: [#issuecomment-4220964591](https://github.com/IvanLi-CN/tavily-hikari/pull/227#issuecomment-4220964591)
- GHCR stable tags: `ghcr.io/ivanli-cn/tavily-hikari:latest`, `ghcr.io/ivanli-cn/tavily-hikari:v0.38.0`
- Release-rule boundary: 保持既有 semver / label 规则不变；`#229` 与 `#230` 均使用 `type:skip + channel:stable`，只恢复 `main` 的 post-merge 发布前置条件，不额外生成新的产品版本。

## 变更记录（Change log）

- 2026-04-10: 创建 spec，冻结“先修 standalone auth regression，再回填 `v0.38.0`”的快车道执行合同。
- 2026-04-10: standalone auth regression 改为最小 schema + direct DB seeding，`mcp_session_delete_neutral_repair` 50 次循环与 `cargo test --locked --all-features` 已在本地通过。
- 2026-04-10: follow-up PR `#230` 追加单连接 SQLite 测试 harness 与 standalone/joined auth candidate 显式分支，`main` 的 `CI Pipeline` run `#24227470494` 恢复为 success。
- 2026-04-10: `Release` workflow_dispatch run `#24227733663` 为 `445a80f87b42ca1eccb60520a443d09326287f95` 回填稳定版 `v0.38.0`，GitHub Release、GHCR `latest` / `v0.38.0` 标签与 PR `#227` release comment 已全部落地。

## 参考（References）

- `docs/specs/w6m86-mcp-session-delete-neutral-repair/SPEC.md`
- GitHub Actions `CI Pipeline` run `24208388945`
- GitHub Actions `Release` run `24208510515`
- Merge commit `445a80f87b42ca1eccb60520a443d09326287f95`
