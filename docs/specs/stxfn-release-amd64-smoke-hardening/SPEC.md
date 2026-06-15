# Release：`amd64` smoke 启动加固与 `v0.36.7` 回填（#stxfn）

## 状态

- Status: 部分完成（3/4）
- Created: 2026-04-07
- Last: 2026-06-15

## 背景 / 问题陈述

- `Release` workflow 的 `Build and smoke image (amd64)` 在 `MCP billing smoke gate` 失败，`v0.36.7` tag 已创建但 GitHub Release 尚未生成。
- 当前 smoke gate 直接内联在 `.github/workflows/release.yml`，固定占用 `58088/58087`，失败时没有输出 `mock_tavily` / Docker / 端口诊断，复盘成本高。
- 现有 PR checks 不包含这条 release-only smoke，所以问题只会在合入 `main` 后暴露；本次目标是修复 release harness，而不是把 release 门禁前移到 PR。
- 回填 `workflow_dispatch(head_sha=旧提交)` 时，release job 会 checkout 到旧代码树；若 workflow 新增了旧提交不存在的 helper 文件，必须显式恢复这些 helper，避免回填路径因 `No such file or directory` 再次失败。

## 目标 / 非目标

### Goals

- 把 `MCP billing smoke gate` 抽成可复用脚本，降低 release workflow 内联复杂度。
- 将 mock upstream / proxy 监听端口改为动态分配，减少 runner 偶发端口冲突。
- 让 smoke gate 在 mock/proxy 启动失败时自动输出关键诊断信息，能直接定位是端口、进程还是容器问题。
- 保持现有 smoke 语义不变，并在修复合入后回填 `a37b6be54db2f114a0987293c9eca9e1281f21f1` 的 `v0.36.7` release。

### Non-goals

- 不修改 PR `CI Pipeline` 的门禁编排，不把 release-only smoke 前移到 PR。
- 不修改 Tavily 代理业务逻辑、计费语义、日志口径或数据库 schema。
- 不新发 `v0.36.8`；本次只回填既有 `v0.36.7`。

## 范围（Scope）

### In scope

- `.github/workflows/release.yml`
- `.github/scripts/release-mcp-billing-smoke.sh`
- `docs/specs/README.md`
- `docs/specs/stxfn-release-amd64-smoke-hardening/SPEC.md`

### Out of scope

- `.github/workflows/ci.yml`
- Rust/前端业务代码与用户可见界面
- 发布版本号策略、release comment 语义与 GHCR tag 规则

## 需求（Requirements）

### MUST

- `MCP billing smoke gate` 必须改为调用仓库内脚本，而不是保留大段 YAML 内联 shell。
- smoke 脚本必须支持动态选择 mock/proxy 端口，并允许通过环境变量覆盖以便复现故障。
- mock 或 proxy 启动失败时，脚本必须输出足够诊断信息（至少含 mock log、Docker 容器状态/日志、端口监听信息、数据目录状态）。
- 成功路径必须保持现有 smoke 断言：mock readiness、proxy `/health`、token 创建、MCP search、406/429、SQLite 账单与请求日志断言。
- SQLite 账单 / 日志断言必须对 observability sidecar 友好：优先读取 `observability.request_logs`，若当前布局仍是 legacy 单库或 backfill 恢复到了只含 `main.request_logs` 的旧代码树，则自动回退到 `main.request_logs`。
- 修复合入后，必须通过 `workflow_dispatch(head_sha=a37b6be54db2f114a0987293c9eca9e1281f21f1)` 成功回填 `v0.36.7` release。
- 当 release workflow 回填到不包含新 helper 的旧提交时，job 必须先恢复缺失 helper，再执行 smoke gate；恢复列表至少要覆盖 `release-mcp-billing-smoke.sh` 与新增的 SQLite 校验 helper。

### SHOULD

- 启动等待逻辑应在“HTTP 就绪”外额外检查进程/容器是否已经提前退出，尽量 fail-fast。
- 清理逻辑应统一由 `trap` 执行，避免失败路径残留容器或 mock 进程。

### COULD

- 脚本可提供少量可复用辅助函数（例如动态端口分配、HTTP 等待、诊断输出），供后续 release smoke 继续复用。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- Release job 构建完 `mock_tavily` 与本地 smoke image 后，调用 `.github/scripts/release-mcp-billing-smoke.sh` 执行整套验收。
- 脚本启动 mock upstream、等待就绪、预置测试 key，再启动 release image 容器并等待 proxy `/health`。
- proxy 就绪后，脚本继续执行 token 创建、MCP search、通知、406/429 注入与 SQLite 账单/日志断言；全部通过后退出 0。
- workflow 保持后续 `Build and push Docker image by digest`、`Publish multi-arch manifest (ghcr)`、`GitHub Release` 阶段不变。

### Edge cases / errors

- 若 mock 进程提前退出或端口不可绑定，脚本必须在 readiness 阶段立刻失败并输出 mock 日志与端口占用信息。
- 若 proxy 容器未就绪或中途退出，脚本必须输出 `docker ps`、对应容器日志与数据目录状态。
- 若 smoke 业务断言失败，脚本仍必须走统一 cleanup，并保留足够诊断输出供下一次复盘。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                     | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner）  | 使用方（Consumers）        | 备注（Notes）         |
| -------------------------------- | ------------ | ------------- | -------------- | ------------------------ | ---------------- | -------------------------- | --------------------- |
| Release MCP billing smoke script | cli          | internal      | New            | ./contracts/cli.md       | release workflow | GitHub Actions release job | workflow 内部脚本接口 |

### 契约文档（按 Kind 拆分）

- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given release smoke 在 runner 上遇到 mock 端口冲突或进程提前退出
  When `.github/scripts/release-mcp-billing-smoke.sh` 失败
  Then job 日志必须包含 mock 日志与端口/进程诊断信息，而不是只剩 `curl: (7)`。
- Given release smoke 正常运行
  When workflow 执行 `MCP billing smoke gate`
  Then 现有 token 创建、MCP search、406/429、SQLite 账单与日志断言必须全部保持通过。
- Given 修复已合入 `main`
  When 手动触发 `Release` workflow 且 `head_sha=a37b6be54db2f114a0987293c9eca9e1281f21f1`
  Then `Build and smoke image (amd64)`、`Publish multi-arch manifest (ghcr)` 与 `GitHub Release` 都必须成功，并能读取 `v0.36.7` release。
- Given 当前 PR checks
  When 本次修复完成
  Then `.github/workflows/ci.yml` 不应新增 release-only 门禁。

## 实现前置条件（Definition of Ready / Preconditions）

- release failure 现象、失败 run、目标回填 SHA 与 tag 已确认。
- 保持 PR 门禁不变、只修 release harness 的边界已确认。
- smoke 脚本接口与回填验收口径已冻结，可直接实施。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Script syntax: `bash -n .github/scripts/release-mcp-billing-smoke.sh`
- Workflow syntax: `python3 - <<'PY'` 解析 `.github/workflows/release.yml` 确认 YAML 合法
- Local smoke rehearsal: 在非发布环境执行脚本并通过完整 smoke（镜像可使用本地构建产物）
- Failure diagnostics rehearsal: 至少验证一次故意失败路径会输出 mock / Docker / 端口诊断

### UI / Storybook (if applicable)

- Not applicable.

### Quality checks

- `git diff --check`
- 变更脚本需保留可执行权限与 `shellcheck` 友好写法（如仓库未引入 shellcheck，则不新增工具依赖）

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增本 spec 索引，并在实现完成后更新状态/备注
- `docs/specs/stxfn-release-amd64-smoke-hardening/SPEC.md`: 记录实现、验证与回填结果

## 计划资产（Plan assets）

- Directory: `docs/specs/stxfn-release-amd64-smoke-hardening/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: 本计划为非 UI 改动，默认无需 owner-facing 截图

## Visual Evidence

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 release smoke 脚本并把 workflow 切换到脚本调用
- [x] M2: 完成动态端口、统一 cleanup 与失败诊断加固
- [x] M3: 完成本地脚本验证与故意失败诊断验证
- [ ] M4: 快车道完成 PR、合并与 `v0.36.7` 回填验证

## 方案概述（Approach, high-level）

- 保留现有 smoke 业务断言不变，只抽离执行框架与启动/诊断层。
- 通过动态端口与 fail-fast readiness 检查减少 runner 偶发冲突导致的假阴性。
- 通过统一 trap + debug dump，让 release-only 故障能直接在单次 workflow 日志里定位，而无需再次猜测。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：Docker runner 环境差异可能导致端口/网络行为与本地不同，因此最终仍需以 GitHub Release 回填 run 为准。
- 需要决策的问题：None。
- 假设（需主人确认）：`workflow_dispatch` 继续以当前 default-branch workflow 定义执行，并允许对既有 `v0.36.7` tag 做幂等回填。

## 变更记录（Change log）

- 2026-04-07: 创建 spec，冻结“只修 release harness、不改 PR 门禁、回填 `v0.36.7`”的执行合同。
- 2026-04-07: 完成脚本抽离、动态端口与失败诊断加固；本地占口失败演练与 shared testbox 完整 smoke rehearsal 已通过，等待 PR/merge/backfill。
- 2026-04-07: 回填 run 暴露旧提交缺失新 smoke helper 的兼容性问题；补充 workflow 在 backfill checkout 后自动恢复 helper 的约束与验证。
- 2026-06-15: smoke 脚本的 SQLite 断言改为 sidecar-aware，并把 backfill helper 恢复扩展到新增的 SQLite 校验 helper，避免 `request_logs` 读错数据库布局。

## 参考（References）

- `.github/workflows/release.yml`
- GitHub Actions run `24066040493`（`Build and smoke image (amd64)` 失败）
- `docs/specs/2c3ep-release-native-arm-images/SPEC.md`
- `docs/specs/kmmtg-release-pr-comment-upsert/SPEC.md`
