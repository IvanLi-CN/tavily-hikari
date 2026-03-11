# Token Request Records 计费可视化与状态列正名（#jewvm）

## 状态

- Status: 已完成（5/5）
- Created: 2026-03-10
- Last: 2026-03-10

## 背景 / 问题陈述

- `/admin/tokens/:id` 的 `Request Records` 当前只能看到 HTTP 与 `mcp_status`，管理员无法逐条对照系统内实际记账的 credits，核查账单时需要额外查库。
- 同一批日志视图里把解析后的 Tavily / structured status code 标成 `MCP Status`，会误导人把它理解成“入口协议是否是 MCP”，而不是 Tavily 返回的业务状态码。
- `auth_token_logs.business_credits` 已经在后端数据库中落盘，但 token 日志接口与 SSE 快照没有把这个字段透出，导致前端只能看到结果，不能看到系统实际计入额度。

## 目标 / 非目标

### Goals

- 在 `/admin/tokens/:id` 的 `Request Records` 中新增 `Charged Credits` 列，明确显示系统内实际记账额度，便于逐条核对计费是否正确。
- 让 token detail 首屏 SSE 刷新、分页接口、桌面表格、移动卡片与展开详情都能看到同一条日志的 credits 信息。
- 全站同类日志视图统一把 `MCP Status` / `MCP` 正名为 `Tavily Status` / `Tavily`，仅调整展示语义，不改变底层 `mcp_status` 字段名与排序/过滤逻辑。
- 保持现有计费口径不变，只透传既有 `auth_token_logs.business_credits` 数据，不新增或重算任何 billing 逻辑。

### Non-goals

- 不修改 credits 计算公式、预扣 / 补扣流程、`counts_business_quota` 语义或任何 quota 口径。
- 不回补历史空值，不把 `NULL` 自动换算成 `0` 或按响应内容反推 credits。
- 不把 `Charged Credits` 新列扩散到 token detail 之外的其它业务面板；其它页面这次只做 `Tavily Status` 文案正名。
- 不触达 Tavily 生产端点做验证；只使用本地 mock / 现有测试数据与开发环境。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `jewvm-token-request-cost-visibility` 索引行。
- `src/lib.rs`
  - `TokenLogRecord` 增加 `business_credits: Option<i64>`，并让 `token_recent_logs` / `token_logs_page` 从 `auth_token_logs.business_credits` 透传该值。
- `src/server/dto.rs` / `src/server/proxy.rs`
  - `TokenLogView`、`TokenSnapshot.logs[]` 与 `/api/tokens/:id/logs`、`/api/tokens/:id/logs/page`、`/api/tokens/:id/events` 响应同步返回 `business_credits`。
- `src/server/tests.rs`
  - 为 token logs / page / events 增加 `business_credits` 映射与 `NULL` 兼容回归测试。
- `web/src/pages/TokenDetail.tsx`
  - 桌面表格新增 `Charged Credits` 列，移动卡片与详情摘要同步展示额度。
  - 桌面列顺序固定为 `Time / HTTP Status / Tavily Status / Charged Credits / Result / Error`。
- `web/src/index.css`
  - 调整 token detail 与 admin 日志表宽度策略，避免新增列后桌面表格和长错误文案互相挤爆。
- `web/src/i18n.tsx`、`web/src/AdminDashboard.tsx`、`web/src/UserConsole.tsx`
  - 将同类日志视图中的 `MCP Status` / `MCP` 统一正名为 `Tavily Status` / `Tavily`。
- `web/src/pages/TokenDetail.stories.tsx` / 相关 mock
  - 补齐有 credits / 无 credits 的可视化样例，方便本地构建或 Storybook 验收。

### Out of scope

- public / user-facing 新增 `Charged Credits` 列或任何新的计费面板。
- `mcp_status` JSON 字段改名、数据库 schema 调整、历史数据修复脚本。
- 与日志状态列无关的 MCP 文案（例如客户端接入指南、MCP probe、协议说明）。

## 接口契约（Interfaces & Contracts）

### Public / external interfaces

以下管理端接口新增 `business_credits` 字段，命名保持 snake_case：

- `GET /api/tokens/:id/logs`
- `GET /api/tokens/:id/logs/page`
- `GET /api/tokens/:id/events`（`snapshot` 事件内的 `logs[]`）

单条日志对象新增：

```json
{
  "id": 42,
  "http_status": 200,
  "mcp_status": 0,
  "business_credits": 4,
  "result_status": "success"
}
```

契约约束：

- `business_credits` 为 `integer | null`。
- 仅当底层日志 `billing_state = charged` 时才透出 `business_credits`；待补扣、重试中或未入账记录统一返回 `null`。
- `business_credits = null` 时前端展示 `—`，不得自行推算。
- `mcp_status` 字段名保持不变；前端只改 label，不改 wire format。

### Internal interfaces

- `TokenLogRecord` / `TokenLogView` / `TokenSnapshot.logs[]` 必须共用同一字段语义，避免分页接口与 SSE 首屏结构漂移。
- `PublicTokenLogView` 与用户公开日志接口本次不新增 `business_credits`，仅允许继续忽略该内部计费字段。

## 验收标准（Acceptance Criteria）

- Given 管理员打开 `/admin/tokens/:id`
  When 请求记录表格渲染完成
  Then 桌面表头顺序为 `Time / HTTP Status / Tavily Status / Charged Credits / Result / Error`。
- Given 某条 token log 的 `billing_state = charged` 且 `business_credits = 7`
  When 该行出现在首屏 SSE 刷新或分页接口返回中
  Then `Charged Credits` 显示 `7`，且移动卡片与展开详情看到同样的值。
- Given 某条 token log 尚未完成入账、处于待补扣/重试中，或 `business_credits = null`
  When 页面渲染日志
  Then 展示 `—`，不显示 `0`，也不从 `result_status` 或响应体反推。
- Given 管理端或用户控制台已有日志视图展示 `mcp_status`
  When 页面渲染该状态字段 label
  Then 文案显示为 `Tavily Status` / `Tavily`，但原有状态码值、排序与过滤逻辑不变。
- Given 后端返回 token logs / page / events
  When 测试读取 JSON
  Then 三个接口都能拿到 `business_credits`，且 `NULL` 数据兼容旧库记录。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt --all`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun run build`
- 开发环境手工验收 `/admin/tokens/:id` 的首屏 SSE 刷新、分页切换与移动视图布局。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新 spec 与 README 索引落地，冻结字段/文案/范围边界
- [x] M2: 后端 token logs / page / events 透传 `business_credits` 并补齐测试
- [x] M3: Token detail 桌面/移动/详情新增 `Charged Credits` 展示并调稳布局
- [x] M4: 全站同类日志视图 `MCP Status` / `MCP` 正名为 `Tavily Status` / `Tavily`
- [x] M5: 本地验证、快车道 PR、checks 与 review-loop 收敛完成

## 风险 / 开放问题 / 假设

- 风险：token detail 现有表格宽度较紧，新增一列后若不重排列宽，长错误信息可能直接挤爆桌面布局。
- 风险：SSE 首屏快照与分页接口由不同路径组装，若只修其一会造成第一页实时刷新和翻页结果不一致。
- 假设：`auth_token_logs.business_credits` 在当前仓库 schema 中已存在；旧数据为空值时只需要前后端按 `Option` 兼容。
- 假设：`Charged Credits` 文案表示“系统内部实际记账额度”，不是货币金额，也不是上游账单展示名。

## Visual Evidence (PR)

Storybook `Admin/Pages/TokenDetail / Default`: verifies the admin token detail page shows the `Charged Credits` column together with the renamed `Tavily Status` label in the request records table.

![Token detail request records desktop](./assets/token-detail-story-default-full.png)

![Token detail request records mobile](./assets/token-detail-story-mobile-full.png)

## 变更记录（Change log）

- 2026-03-10: 初始化快车道 spec，冻结 token detail 费用列、`Tavily Status` 文案正名，以及 `business_credits` 仅透传不改计费逻辑的边界。

- 2026-03-10: 已完成后端 `business_credits` 透传、Token detail `Charged Credits` 展示、全站 `Tavily Status` 文案正名；`cargo test`、`cargo clippy -- -D warnings`、`cd web && bun run build` 通过，并以本地 seeded 数据确认 `/api/tokens/:id/logs`、`/logs/page`、`/events` 返回一致。

- 2026-03-10: 快车道收敛完成；PR 截图已同步到 `## Visual Evidence (PR)`，最新 head SHA 的 checks 与 PR-stage review-loop 均已通过。
