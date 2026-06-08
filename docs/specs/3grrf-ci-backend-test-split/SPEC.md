# GitHub Actions 后端测试拆分与并行提速（#3grrf）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 当前 `CI Pipeline` 把后端 Rust 测试集中在单一 `Backend Tests` job 中，导致 `Compose Smoke` 与 `Build (Release)` 只能在其完成后再启动，关键路径长期维持在接近 1 小时。
- 同一条 workflow 中存在多次重复的 `web/dist` 构建；Rust jobs 只为了满足 `build.rs` 的嵌入资源契约，也会各自重新执行 `bun install && bun run build`。
- 现有 owner-facing 检查面默认把“后端是否通过”收敛到 `Backend Tests` 一个 check；如果直接拆成多个新 job 而没有稳定 gate，reviewer 很难快速判断整体后端回归是否已清零。
- 本主题需要一份长期规范，约束后续对 backend test topology、coverage 等价证明和并行化边界的修改方式，避免每次 CI 调优都重新发明规则。

## 目标 / 非目标

### Goals

- 将 `CI Pipeline` 的第一阶段提速限定为“拆分现有 backend tests + 复用单次 web build + 重排 smoke/build 依赖”，不减少任何现有测试用例执行数量。
- 保留稳定的 owner-facing `Backend Tests` gate，同时允许内部新增多个 backend shard jobs，用于缩短关键路径并提升失败定位精度。
- 明确 PR1 与 PR2 的分工：PR1 只做 workflow 拓扑与 artifact 复用；PR2 在保持 `cargo test` 语义不变的前提下，引入基于测试命名空间/前缀的 shard manifest 与 job/matrix 并行。
- 为后续并行化提供 coverage 等价证明约束：新旧执行图必须能证明 union 后覆盖到当前所有 `cargo test --lib`、`cargo test --test <...>`、`cargo test --bins` 的既有目标。

### Non-goals

- 不减少、跳过、`ignore`、条件禁跑任何现有 Rust 测试。
- 不引入 `cargo-nextest`、自定义测试 runner 或修改 Rust 测试线程模型。
- 不把本主题扩展到 `release.yml`、`docs-pages.yml` 或无关产品代码重构。
- 不要求一次性修复所有历史 flaky；仅处理会阻碍 shard 切分、覆盖等价证明或 job/matrix 并行稳定性的测试组织问题。

## 范围（Scope）

### In scope

- `.github/workflows/ci.yml`
  - 引入单次 web asset build / artifact 复用、backend semantic shards、stable aggregate gate，以及 `Compose Smoke` / `Build (Release)` 的关键路径重排。
- `docs/specs/3grrf-ci-backend-test-split/**`
  - 固化本主题的 contract、implementation status 与 decision trace。
- `scripts/**`（若需要）
  - 提供 repo-local shard manifest、coverage 校验脚本或 CI 执行辅助脚本，确保 PR2 的 shard 分配可验证、可维护。
- Rust 测试入口组织（仅在 PR2 必要时）
  - 允许最小必要的测试分组/入口整理，但必须以“保持断言语义不变、保持现有测试数不变、帮助 job/matrix 并行为目的”。

### Out of scope

- 业务逻辑、HTTP/MCP 接口、数据库 schema 或 release/deploy 策略变更。
- 通过修改断言、替换 mock、缩短 sleep 语义或改变 feature 面来“伪提速”。
- release-only smoke 前移到 PR `CI Pipeline` 之外的额外门禁。

## 需求（Requirements）

### MUST

- PR1 中所有当前会通过 `cargo test --locked --all-features --lib`、`cargo test --locked --all-features --test <...>`、`cargo test --locked --all-features --bins` 触发的目标，仍必须在新 CI 执行图里全部运行。
- PR1 必须保留一个稳定命名的 owner-facing `Backend Tests` check；内部 shards 可以新增，但 reviewer 不应失去总体 backend gate。
- PR1 必须把 frontend 构建收敛为单次 `web/dist` 产物并在需要的 jobs 中复用，避免每个 Rust/compose job 各自重复构建。
- PR1 必须让 `Compose Smoke` 与 PR 场景的 `Build (Release)` 只依赖真正需要的前置，不再等待整段 backend tests 收口。
- PR2 必须提供可自动验证的 shard manifest / coverage 校验，保证每个已有测试只被分配到一个 shard，且不存在未分配测试。
- PR2 的并行化只能基于现有 `cargo test` 语义与 job/matrix 并发，不得引入新的 runner。

### SHOULD

- backend shards 的命名应体现语义边界，例如 `lib core`、`lib app tests`、`integration tests`、`server/bin tests`，避免 reviewer 只能从实现细节猜测分片含义。
- shard manifest 应尽量使用稳定命名空间或测试名前缀，而不是脆弱的文件路径或临时排序。
- coverage 校验脚本输出应同时适合本地与 CI 使用，便于在 review 中快速证明“未减少测试数量”。

### COULD

- 如果 PR2 发现少量测试 helper 的共享组织阻碍 shard manifest，可做最小辅助脚本或命名空间整理，但应优先避免大规模测试搬迁。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- PR1：
  - 先构建一次 `web/dist` 并上传 artifact。
  - `lint` / backend shards / `Compose Smoke` / `Build (Release)` 下载同一份 web assets，而不是各自重新构建。
  - backend 测试拆成多个语义 shard，并新增 `Backend Tests` aggregate gate 汇总结果。
  - `Compose Smoke` 与 `Build (Release)` 只等待 `lint`、frontend checks 与 web assets，不等待 backend gate。
- PR2：
  - 基于 `cargo test -- --list` 的真实输出维护 shard manifest。
  - lib / bin tests 按命名空间或测试名前缀切到多个 matrix shards。
  - CI 在执行 shards 前先运行 coverage 校验，若出现 overlap / unmatched / stale selector 立即失败。

### Edge cases / errors

- 若新增测试未命中任何 shard selector，coverage 校验必须失败，而不是默默漏跑。
- 若同一测试被两个 selector 同时命中，coverage 校验必须失败，而不是允许重复运行制造假绿。
- 若某个 job 缺少 web assets artifact，相关 Rust build / compose smoke 必须以明确错误失败，而不是悄悄回退到本地现编。
- 若 future PR 重新把 `Compose Smoke` / `Build (Release)` 绑回 backend gate，应视为违反本 spec 的关键路径约束。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                               | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner）            | 使用方（Consumers）                                  | 备注（Notes）                                  |
| ------------------------------------------ | ------------ | ------------- | -------------- | ------------------------ | -------------------------- | ---------------------------------------------------- | ---------------------------------------------- |
| CI Pipeline backend shard topology         | workflow     | internal      | Modify         | None                     | `.github/workflows/ci.yml` | GitHub Actions PR / push runs                        | owner-facing `Backend Tests` gate 必须保持稳定 |
| web assets CI artifact                     | artifact     | internal      | New            | None                     | `.github/workflows/ci.yml` | lint / backend shards / compose / release build jobs | 仅用于 CI 内复用 `web/dist`                    |
| backend shard manifest / coverage verifier | cli          | internal      | New            | None                     | `scripts/**`               | local validation + GitHub Actions                    | PR2 引入，负责 coverage 等价证明               |

### 契约文档（按 Kind 拆分）

- `None`

## 验收标准（Acceptance Criteria）

- Given 当前 `CI Pipeline` 的 backend coverage 基线来自 `cargo test --locked --all-features --lib`、`cargo test --locked --all-features --test <...>`、`cargo test --locked --all-features --bins`
  When PR1 合入后的新 workflow 运行
  Then 所有这些目标都必须仍被执行，且 owner-facing `Backend Tests` gate 仍能直接反映 backend 总体通过/失败。

- Given PR 场景运行 `CI Pipeline`
  When backend shards 尚未全部完成
  Then `Compose Smoke (ForwardAuth + Caddy)` 与 `Build (Release)` 应能在其真实前置完成后启动，而不是继续被 backend gate 阻塞。

- Given PR2 启用 shard manifest
  When 新增或修改测试导致 selector 漂移
  Then coverage 校验必须因为 overlap 或 unmatched 失败，阻止漏跑测试进入绿 CI。

- Given 比较“调优前后耗时”
  When 统计最近成功 runs 的 job wall time
  Then 应使用 job `startedAt/completedAt` 而不是 run `createdAt/updatedAt`，并证明关键路径明显下降。

## 验收清单（Acceptance checklist）

- [ ] PR1 的 backend semantic shards、web asset artifact、aggregate gate 与关键路径重排已被明确描述。
- [ ] PR2 的 shard manifest、coverage 校验与 job/matrix 并行约束已被明确描述。
- [ ] 未减少测试数量、未引入新 runner、未改变测试语义的边界已写清楚。
- [ ] 关键失败模式（unmatched / overlap / artifact missing / dependency regression）已覆盖。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- PR1:
  - `cargo test --locked --all-features --lib -- --list`
  - `cargo test --locked --all-features --bins -- --list`
  - `cargo test --locked --all-features --test rust_source_line_budgets`
  - workflow syntax validation for `.github/workflows/ci.yml`
- PR2:
  - shard manifest coverage verifier over `cargo test -- --list` output
  - representative shard executions covering lib / integration / bin split
  - full `CI Pipeline` on PR head

### UI / Storybook (if applicable)

- None

### Quality checks

- `cargo fmt`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `bunx --bun prettier --check .github/workflows/ci.yml docs/specs/README.md docs/specs/3grrf-ci-backend-test-split/SPEC.md docs/specs/3grrf-ci-backend-test-split/IMPLEMENTATION.md docs/specs/3grrf-ci-backend-test-split/HISTORY.md`

## Visual Evidence

- None

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若 shard selector 设计不稳定，PR2 可能把新的测试命名漂移转化成维护负担；因此必须用自动 coverage verifier 做硬门禁。
- 风险：当前 `src/tests` / `src/server/tests` 的测试数量很大，单 shard 过粗时 PR2 提速有限；但该风险不能用减少覆盖来解决。
- 需要决策的问题：若 future 需要进一步压缩 lib shard 墙钟时间，是否继续做测试命名空间整理；本 spec 当前只要求最小必要整理，不强推大规模搬迁。
- 假设（需主人确认）：本次 stacked PR 按 `merge-ready` 收口，PR1 与 PR2 都允许通过同仓 stacked PR 方式交付。

## 参考（References）

- `.github/workflows/ci.yml`
- `docs/specs/xxgfb-release-binary-assets/SPEC.md`
- `docs/specs/9rdxm-post-merge-release-unblock/SPEC.md`
