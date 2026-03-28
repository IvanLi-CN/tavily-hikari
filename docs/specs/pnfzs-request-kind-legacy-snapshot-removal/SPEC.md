# Request Kind Legacy Snapshot 字段移除（#pnfzs）

## 状态

- Status: 已实现（待审查）
- Created: 2026-03-28
- Last: 2026-03-28

## 背景 / 问题陈述

- `msmcp` 已经把 request kind 历史 canonical 化收进数据库迁移门禁，服务启动成功即可视为历史升级完成。
- `legacy_request_kind_*` 与对外 `legacyRequestKind*` 审计字段原本只服务于兼容窗口内的无损快照，现在继续保留只会增加 schema、自愈迁移、DTO 和测试面的复杂度。
- 当前项目的第一方前端并不消费这些字段；继续保留它们只会让后续 request-kind 相关改动一直背着兼容包袱。

## 目标 / 非目标

### Goals

- 删除 `request_logs` / `auth_token_logs` 的 `legacy_request_kind_*` 列，并为已有数据库提供明确的删列迁移。
- 删除日志 API item 中的 `legacyRequestKindKey/Label/Detail` 返回字段。
- 删除 backfill / 启动迁移中的 legacy 快照写入逻辑，只保留 canonical 化本身。
- 保留 legacy alias 查询兼容：旧的 `request_kind=mcp:raw:/...`、`mcp:tool:*` 过滤仍能命中 canonical 结果集。

### Non-goals

- 不回退或改写 canonical request kind catalog。
- 不移除基于旧 `request_kind_key` 值的 alias 过滤兼容。
- 不修改 `method/path/query/request_body/response_body` 等原始事实字段。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
- `docs/specs/pnfzs-request-kind-legacy-snapshot-removal/**`
- `src/models.rs`
- `src/store/mod.rs`
- `src/server/{dto,proxy}.rs`
- `web/src/api.ts`
- `tests/request_kind_canonical_backfill.rs`
- `src/{tests,server/tests}.rs`

### Out of scope

- 新增 request kind 分类或别名规则。
- 与 request kind 无关的日志页面 UI 变更。

## 需求（Requirements）

### MUST

- 启动迁移必须能处理“已经存在 `legacy_request_kind_*` 列”的历史数据库，并在成功启动后移除这些列。
- 日志相关 API 响应必须不再返回 `legacyRequestKind*`。
- `request_kind_canonical_backfill` 与启动迁移必须继续把旧 raw/tool kind canonical 化，但不再保留 legacy 快照副本。
- `request_kind` 查询过滤与 facet 兼容行为必须保持不变，legacy alias 仍能命中 canonical 数据。

### SHOULD

- 尽量复用现有 `request_logs` rebuild 迁移路径，并为 `auth_token_logs` 增补同等级别的 rebuild 迁移。
- 保持 SQLite 迁移幂等，重复启动不应再次触发表重建。

### COULD

- 顺手清理仅为 legacy 快照存在而引入的辅助函数和测试夹具。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 旧库启动时：
  - 若两张日志表仍含 `legacy_request_kind_*`，启动迁移先完成删列重建，再继续正常初始化。
  - 若 legacy 列已不存在，迁移直接跳过，不重复重建。
- 历史 raw/tool kind 行经 startup migration 或 backfill 处理后：
  - 主 `request_kind_*` 改成 canonical 值。
  - 不再额外写入任何 legacy snapshot 字段。
- 日志 API / 管理页数据模型：
  - 只暴露 canonical `requestKind*`。
  - legacy alias 查询继续由后端 canonical 化后命中同一结果集。

### Edge cases / errors

- 若表重建中断，数据库不得停留在半成品表名；下次启动应能继续迁移。
- 若历史数据库根本没有 legacy 列，这个 PR 不能要求它们先被创建再删除。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                  | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）  | 备注（Notes）                   |
| ----------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | -------------------- | ------------------------------- |
| Request/Token log item schema | http-api     | external      | Delete         | ./contracts/http-apis.md | backend         | web, external admins | 删除 `legacyRequestKind*`       |
| request log tables            | db           | internal      | Delete         | ./contracts/db.md        | backend         | store, migrations    | 删除 `legacy_request_kind_*` 列 |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 一个包含 `legacy_request_kind_*` 列的历史数据库
  When 新版本启动成功
  Then `request_logs` 与 `auth_token_logs` 都不再包含这些列，且服务可正常提供请求日志接口。
- Given 历史样本 `mcp:raw:/mcp/search`
  When startup migration 或 `request_kind_canonical_backfill` 处理该行
  Then 主字段变为 canonical `mcp:unsupported-path`，且数据库/API 中不再保留对应的 `legacy_request_kind_*` / `legacyRequestKind*`。
- Given Admin / Key / Token 日志 API 响应
  When 返回日志分页数据
  Then payload 中不包含 `legacyRequestKindKey`、`legacyRequestKindLabel`、`legacyRequestKindDetail`。
- Given 查询参数仍使用 legacy alias，如 `request_kind=mcp:raw:/mcp/search`
  When 请求日志分页
  Then 结果仍命中 canonical `mcp:unsupported-path` 对应的同一数据集。

## 实现前置条件（Definition of Ready / Preconditions）

- 兼容窗口已结束，允许删除 legacy snapshot 列与对外字段。
- 启动成功即升级完成的语义已由前序 PR 落地。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: request kind mapping / migration helpers
- Integration tests: SQLite 升级后删列、日志 API 响应、legacy alias 查询兼容
- E2E tests (if applicable): None

### UI / Storybook (if applicable)

- None

### Quality checks

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增 `pnfzs` 索引并同步状态
- `docs/specs/msmcp-request-kind-canonicalization-lossless-history/SPEC.md`: 标记 legacy 清理已移交 follow-up spec

## 计划资产（Plan assets）

- Directory: `docs/specs/pnfzs-request-kind-legacy-snapshot-removal/assets/`
- Visual evidence source: maintain `## Visual Evidence` in this spec when needed.

## Visual Evidence

本次为后端/迁移收敛，不要求视觉证据。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 spec 与接口契约，明确 legacy snapshot 删除范围
- [x] M2: 删除模型 / DTO / API 类型中的 `legacyRequestKind*`
- [x] M3: 完成 SQLite legacy 列删列迁移，并移除 legacy snapshot 写入
- [ ] M4: 补齐 migration/backfill/接口回归测试并完成快车道 PR 收口

## 方案概述（Approach, high-level）

- 把“legacy snapshot 列删除”做成一次性数据库迁移，而不是只在代码层面忽略。
- 保留 legacy alias 过滤辅助 SQL，让历史 raw/tool key 的查询兼容不依赖 legacy 列。
- 将 request kind backfill 收缩为“canonical 化主字段”的单一职责，减少后续维护面。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：SQLite 表重建必须同时覆盖 `request_logs` 与 `auth_token_logs`，否则容易出现一张表删列、一张表残留的半升级状态。
- 风险：若遗漏测试夹具中的 legacy 列断言，容易让回归测试仍然绑定旧兼容合同。
- 假设：仓库内外消费者已经接受删除 `legacyRequestKind*` 对外字段。

## 变更记录（Change log）

- 2026-03-28: 创建 follow-up spec，定义 request kind legacy snapshot 字段移除与删列迁移合同。
- 2026-03-28: 完成 legacy snapshot 列/API 删除、双表删列迁移、回填职责收缩，以及本地 `cargo test` / `cargo clippy --all-targets -- -D warnings` 与 review proof。

## 参考（References）

- `docs/specs/msmcp-request-kind-canonicalization-lossless-history/SPEC.md`
- `src/store/mod.rs`
- `src/server/dto.rs`
- `web/src/api.ts`
