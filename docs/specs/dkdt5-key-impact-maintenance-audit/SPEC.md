# Key 影响可见化与维护记录审计（#dkdt5）

## 状态

- Status: 已实现（待审查）
- Created: 2026-03-17
- Last: 2026-03-17

## 背景 / 问题陈述

- 现有调用日志只能展示 HTTP / Tavily 状态与原始错误，管理员无法直接判断“这次调用对 Key 造成了什么状态影响”。
- 系统已经具备隔离、标记耗尽、恢复 active 等 Key 健康维护动作，但缺少统一的 append-only 审计历史，无法区分系统自动维护与管理员手动维护。
- 用户侧与管理员侧日志的可见范围必须严格分层；本次需求要求管理员看得更细，但不能顺带扩大用户可见字段或泄漏敏感信息。

## 目标 / 非目标

### Goals

- 为管理员全局调用日志与管理员 Token 详情日志新增持久化的 `failure_kind`、`key_effect_code`、`key_effect_summary` 展示，避免纯前端猜测。
- 为 `request_logs` 与 `auth_token_logs` 同步落盘错误分类与 Key 影响字段，保证两处管理员日志入口看到一致语义。
- 新增 `api_key_maintenance_records` 作为 append-only 审计历史，覆盖系统自动维护与人工健康维护动作。
- 用户侧维持现有字段集合不变，仅允许在现有错误详情/错误文案中增加脱敏后的解决建议。
- 对 `1-5` 号错误在管理员与用户现有详情里补固定解决建议；`6-13` 号错误保持现有响应透传，只展示脱敏后的原始或归一化错误信息。

### Non-goals

- 不为用户日志接口新增字段、列或管理员专用信息。
- 不引入新的自动冷却、重试、熔断、代理切换策略；本次只做分类、持久化、展示与审计。
- 不把非健康类管理员动作（启用/禁用、分组、删除/恢复等）纳入新维护记录表。
- 不替换 `api_key_quarantines`；该表继续作为“当前隔离状态表”。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `dkdt5-key-impact-maintenance-audit` 索引行。
- `src/lib.rs`
  - 为 `request_logs` 与 `auth_token_logs` 增加 `failure_kind`、`key_effect_code`、`key_effect_summary` schema 与查询映射。
  - 新增 `api_key_maintenance_records` schema、索引与读写 helper。
  - 固化 `1-13` 号错误到 `failure_kind` 的映射，并把真正发生的 Key 状态变更压成 `key_effect_*`。
  - 自动维护动作写入维护记录：`auto_quarantine`、`auto_mark_exhausted`、`auto_restore_active`。
- `src/server/handlers/admin_*` / `src/server/dto.rs` / `src/server/proxy.rs`
  - 管理员日志 DTO 透出 `failure_kind`、`key_effect_code`、`key_effect_summary`。
  - 现有人工健康维护动作写入维护记录：`manual_clear_quarantine`、`manual_mark_exhausted`。
- `src/server/handlers/user.rs` / `src/server/handlers/public.rs`
  - 用户/public 日志接口维持字段形状不变，只在现有 `error_message` 文案中做脱敏后的建议拼接。
- `web/src/AdminDashboard.tsx` / `web/src/pages/TokenDetail.tsx` / `web/src/i18n.tsx`
  - 管理员两处日志入口新增 `Key 影响` 展示，并在详情里展示 `1-5` 号错误解决建议。
- `web/src/UserConsole.tsx` / `web/src/PublicHome.tsx`
  - 不新增字段，仅复用现有错误展示承载脱敏后的建议文案。
- `src/server/tests.rs` / `web` 相关测试或 stories
  - 补齐 schema、分类、DTO 分层、审计写入、UI 渲染与脱敏回归。

### Out of scope

- 新增用户可见日志列或新的用户日志 API 字段。
- 新建 Key 健康维护后台页面或新的管理员手动健康操作入口。
- 历史数据回填脚本；旧日志允许 `failure_kind` 为空、`key_effect_code` 为 `none`。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                      | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）   | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                  |
| --------------------------------- | ------------ | ------------- | -------------- | -------------------------- | --------------- | ------------------- | ---------------------------------------------- |
| `GET /api/logs`                   | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | server          | admin dashboard     | 新增管理员专用 `failure_kind` / `key_effect_*` |
| `GET /api/tokens/:id/logs/page`   | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | server          | admin token detail  | 新增管理员专用 `failure_kind` / `key_effect_*` |
| `GET /api/user/tokens/:id/logs`   | HTTP API     | external      | Modify         | `./contracts/http-apis.md` | server          | user console        | 字段不变，仅调整 `error_message` 文案          |
| `GET /api/public/token_logs`      | HTTP API     | external      | Modify         | `./contracts/http-apis.md` | server          | public token logs   | 字段不变，仅调整 `error_message` 文案          |
| `DELETE /api/keys/:id/quarantine` | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | server          | admin key detail    | 追加维护审计写入，无接口字段变化               |
| `POST /api/keys/import`           | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | server          | admin bulk import   | 现有 `marked_exhausted` 分支追加维护审计       |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 管理员查看全局调用日志或管理员 Token 详情日志
  When 某条日志命中了已定义的 `failure_kind`
  Then 页面显示持久化的 `Key 影响` 摘要，而不是前端临时推导。
- Given 用户查看自己的日志或 public token 日志
  When 同一条错误日志被返回
  Then 返回字段集合与当前一致，且只在现有 `error_message`/详情位置看到脱敏后的建议文案。
- Given 错误属于 `1-5` 号类型
  When 管理员或用户展开现有详情
  Then 能看到固定解决建议，不要求新增单独字段。
- Given 错误属于 `6-13` 号类型
  When 系统记录并透出日志
  Then 不增加平台代写方案块，实际业务响应保持透传，仅展示脱敏后的原始/归一化错误信息。
- Given 系统自动隔离、自动标记耗尽、自动恢复 active、手动解除隔离或手动标记耗尽发生
  When 对应动作执行成功
  Then `api_key_maintenance_records` 追加一条审计记录，并保存动作前后状态、来源与关联日志 id（若存在）。
- Given 错误为 `429`、`502/504`、`transport error`、`406` 或 `6-13` 中的协议/参数错误
  When 调用被记录
  Then `key_effect_code = none`，且不会误写维护记录。

## 实现前置条件（Definition of Ready / Preconditions）

- 管理员两处日志入口与用户/public 日志接口边界已冻结。
- `1-13` 号错误到 `failure_kind` / `key_effect_*` 的映射已定稿。
- 维护记录表字段、来源语义与写入边界已定稿。
- 用户侧“不扩字段，仅改现有错误文案”的限制已确认。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 错误分类、Key 影响映射、维护记录写入、脱敏与建议文案拼接。
- Integration tests: 管理员/用户/public 日志接口字段分层，人工维护动作审计落表。
- E2E tests (if applicable): 无新增外部 E2E；管理员日志两入口使用现有 UI / story 覆盖。

### UI / Storybook (if applicable)

- Stories to add/update: 管理员日志与管理员 Token 详情至少补一组含 `Key 影响` 与建议文案的状态样例。
- Docs pages / state galleries to add/update: 如已有 admin stories，则补对应错误态展示。
- `play` / interaction coverage to add/update: 展开日志详情时可见 `Key 影响` 与建议文案。
- Visual regression baseline changes (if any): 允许新增一列后的布局基线更新。

### Quality checks

- `cargo fmt --all`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun run build`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增本 spec 索引并跟踪状态。
- `docs/specs/dkdt5-key-impact-maintenance-audit/contracts/http-apis.md`: 固定管理员/用户日志 DTO 分层边界。
- `docs/specs/dkdt5-key-impact-maintenance-audit/contracts/db.md`: 固定新增字段与维护记录表结构。

## 计划资产（Plan assets）

- Directory: `docs/specs/dkdt5-key-impact-maintenance-audit/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## Visual Evidence (PR)

- 管理员全局日志：展示 `Key 影响` 列以及 `1-5` 号错误的详情建议。
- 管理员 Token 详情日志：展示 `Key 影响` 列与详情建议，验证两处入口一致。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: 新 spec、contracts 与 README 索引落地，冻结错误分类 / Key 影响 / 维护审计边界
- [ ] M2: `request_logs` / `auth_token_logs` 新增 `failure_kind` 与 `key_effect_*` 字段并补 migration / 查询映射
- [ ] M3: `api_key_maintenance_records` 落地，系统自动 + 现有人工健康维护动作写入审计
- [ ] M4: 管理员两处日志入口显示 `Key 影响`，用户/public 日志仅增强现有错误文案
- [ ] M5: 测试、构建、review-loop 与 merge-ready 收敛完成

## 方案概述（Approach, high-level）

- 以后端分析层为唯一真相源：在记录日志时一次性分类错误并固化 `key_effect_*`，前端只展示，不自行推断最终语义。
- 维护记录表采用 append-only 模式，保留每次自动/手动健康维护的时间线；当前隔离状态仍由 `api_key_quarantines` 单独维护。
- 用户/public 侧继续最小暴露原则：复用现有字段承载脱敏后的文案，不暴露管理员专用状态元数据。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：管理员两处日志入口底层数据源不同，若只改一侧查询会造成 `Key 影响` 语义漂移。
- 风险：错误文本可能夹带 URL query secret 或 API key，建议文案拼接前必须先统一脱敏。
- 假设：现有“手动标记耗尽”入口仅来自 admin import / validate 流程，本次不新增新的手动健康维护按钮。
- 假设：`1-5` 指向的错误类型按当前计划约定，即 `502/504`、`429`、`401 deactivated`、transport send error、`406`。

## 变更记录（Change log）

- 2026-03-17: 初始化规格，冻结日志分层、错误分类、Key 影响字典与维护记录表边界。

## 参考（References）

- `/admin/requests` 线上错误样本复盘（2026-03-17）
- 现有 `api_key_quarantines` 与 `request_logs` / `auth_token_logs` schema
