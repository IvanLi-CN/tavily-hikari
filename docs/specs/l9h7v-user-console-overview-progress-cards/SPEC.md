# 用户控制台全宽概览与实时进度卡改版（#l9h7v）

## 状态

- Status: 已完成（PR-ready）
- Created: 2026-06-11
- Last: 2026-06-11

## 背景 / 问题陈述

- 当前 `/console` landing 概览仍沿用窄列堆叠，`今日成功 / 今日失败 / 本月成功（UTC）` 与 4 张额度卡都挤在同一节奏里，超宽屏下利用率很差。
- 现有 4 张额度卡只显示数字，不展示周期内的用量走势，也无法表达“未来时段保留空位、不得伪造为 0”这一产品语义。
- 现有用户侧没有 landing 专用概览读模型；若直接把旧 `GET /api/user/dashboard` 膨胀成趋势接口，会把兼容合同和 owner-facing landing 需求绑死在一起。
- token detail 已有独立 SSE 职责，但 landing 概览仍缺少轻量 changed-only 推送路径，无法在不增加服务器压力的前提下稳定做到 5 秒级刷新。

## 关联规格

- `45squ-account-quota-user-console`
  - 继续复用用户控制台、用户额度、充值与 Token 列表的既有业务边界。
- `2nx74-user-console-single-page-landing`
  - 继续沿用 `/console` 单页 landing 合并后的信息架构，但升级 overview 版式与入口契约。
- `tz9kq-statistics-timezone-contract`
  - 保持“月口径 UTC、用户今日成功/失败可接受显式 browser today window、日配额仍是服务器本地自然日”。
- `q7u4m-unified-request-rate-memory-limiter`
  - `5 分钟请求频率` 严格复用真实 rolling `5m` 内存限频语义，不回退到自然窗口伪实现。
- `3zky1-admin-user-shared-usage-charts`
  - 复用 account usage rollup 与 historical limit snapshot 设计原则，但用户 landing 使用独立 current-period 合同与 UI。

## 目标 / 非目标

### Goals

- 把 `/console` shell 收敛成可用于超宽屏的全宽布局，并让 `Console Home`、充值区、Token 列表都进入新的宽屏节奏。
- 将概览区重构为两层信息架构：
  - 上层 3 张大号汇总卡：`今日成功`、`今日失败`、`本月成功（UTC）`
  - 下层 4 张大号趋势卡：`5 分钟请求频率`、`小时配额`、`日配额`、`月配额`
- 趋势卡必须展示真实用量背景图与上限横虚线，并让周期内未来时段保留 `null` 空占位，不得填 0、不得裁掉空间。
- 新增 landing 专用 `GET /api/user/dashboard/overview` 与 `GET /api/user/dashboard/events`，首屏 HTTP，后续 SSE changed-only，服务端最多每 5 秒发一帧，无变化仅发 `ping`。
- 保持旧 `GET /api/user/dashboard` 完整兼容，不把旧 dashboard 合同升级成全能趋势接口。
- 为 current-period 进度图补齐轻量读模型：
  - `requestRate`：真实 rolling `5m`
  - `quotaHourly`：当前小时 5 分钟粒度
  - `quotaDaily`：当前服务器本地自然日按小时粒度
  - `quotaMonthly`：当前 UTC 月按 UTC 日粒度
- 补齐 Storybook 宽屏 / 移动端入口与视觉证据。

### Non-goals

- 不给 token detail 新增趋势图、landing SSE 订阅、或 token-level usage-series 合同。
- 不修改 request-rate limiter 的真实语义，不把 rolling `5m` 改成自然 5 分钟整窗。
- 不重做充值逻辑、debug-sharing 逻辑、MCP probe、日志表格语义或 Token 管理能力边界。
- 不复用 admin 用户共享用量 tabs 的历史视窗接口去拼用户 landing current-period 进度图。
- 不做全站视觉翻新；本轮仅覆盖 `/console` shell 与 `Console Home` 概览。

## 范围（Scope）

### In scope

- `src/tavily_proxy/**`
  - landing 专用 overview 读模型、rolling request-rate series、current-period 额度 series
- `src/server/handlers/user.rs`
- `src/server/serve.rs`
- `src/models.rs`
- `src/tests/**`
- `src/server/tests/**`
- `web/src/api/**`
- `web/src/user-console/**`
- `web/src/UserConsole.stories.tsx`
- `web/src/UserConsole.stories.test.ts`
- `web/src/styles/**`
- `docs/specs/README.md`

### Out of scope

- Token detail 的趋势图与额外实时接口
- admin dashboard / admin user detail UI
- 额度业务规则、计费规则、请求日志保留策略

## 数据与接口契约

### 保留旧合同

- `GET /api/user/dashboard`
  - 继续只承担兼容 summary 合同。
  - 现有字段与消费者不变。

### 新增 landing overview HTTP

- `GET /api/user/dashboard/overview`
- query 继续接受 `today_start` / `today_end`
  - 仅影响 `summary.dailySuccess` / `summary.dailyFailure`
  - 不改变 `quotaDaily` 当前服务器本地自然日语义
- response:
  - `summary`
    - 结构与既有 `UserDashboard` 等价
  - `progress`
    - `requestRate`
    - `quotaHourly`
    - `quotaDaily`
    - `quotaMonthly`
- 每个 progress card 返回：
  - `used`
  - `limit`
  - `points[]`
- 每个 point 返回：
  - `bucketStart`
  - `displayBucketStart?: number | null`
  - `value: number | null`
  - `limitValue: number | null`

### 新增 landing overview SSE

- `GET /api/user/dashboard/events`
- 首帧发送 `event: snapshot`
- 后续最多每 5 秒一次：
  - 若 snapshot 与上一帧不同：发送 `event: snapshot`
  - 若无变化：发送 `event: ping`
- EventSource 断线后，前端只允许对 landing overview 单接口降级刷新；不得回退成多接口高频轮询。

## 周期语义

- `requestRate`
  - 表示真实 rolling `5m`，不是自然 5 分钟整窗。
  - 图上不做人造“未来空位”语义，只表达最近 rolling 窗口内的真实占用变化。
- `quotaHourly`
  - 表示当前小时从 `hh:00` 到 `hh:59:59` 的进度图。
  - 未来 5 分钟 bucket 保留 `null`。
- `quotaDaily`
  - 表示当前服务器本地自然日进度图。
  - 未来小时 bucket 保留 `null`。
- `quotaMonthly`
  - 表示当前 UTC 月进度图。
  - 未来 UTC 日 bucket 保留 `null`。
- 所有趋势卡的上限虚线都允许被真实值越过，不得截断图或强制 clamp 到 limit。

## 读模型约束

- 优先复用现有 `account_usage_rollup_buckets` 与 limit snapshot 表，避免扫描原始 `auth_token_logs`。
- 为 `quotaHourly` 增加 `business_credits / five_minute` rollup。
- 为 `quotaMonthly` 增加 `business_credits / utc_day` rollup，避免把服务器本地 `day` bucket 误用于 UTC 月内逐日进度。
- `requestRate` 当前窗口直接读取真实内存 limiter subject 时间戳，构造 rolling series，不写回数据库。

## UI / 体验契约

- `/console` shell 必须支持全宽布局，在 `2560px+` 屏宽下不再被 `1200px` 窄容器锁死。
- `Console Home` 的 overview 区域采用明确的 3 + 4 卡分层：
  - 汇总卡只显示数值与周期说明，不渲染背景图
  - 趋势卡渲染背景走势、数值、limit 虚线与周期辅助说明
- `Token Detail` 只跟随 shell 宽度、留白、卡片尺度与材质系统，不新增功能块。
- `390px` 手机端仍需保持顺序清晰，趋势卡允许单列堆叠，但不能丢失未来空占位语义。

## 验收标准

- `GET /api/user/dashboard` 现有消费者保持兼容。
- `GET /api/user/dashboard/overview` 返回 3 张汇总卡所需 summary 与 4 张趋势卡所需 current-period progress。
- `GET /api/user/dashboard/events` 最多每 5 秒推一帧 changed-only snapshot，无变化时发 `ping`。
- landing 首屏只做一次 overview HTTP；SSE 建立后不再触发多接口定时刷新。
- 趋势卡中：
  - `requestRate` 严格体现 rolling `5m`
  - `quotaHourly` / `quotaDaily` / `quotaMonthly` 的未来时段为 `null`
  - limit 线可被实际值越过
- `/console` 在手机、常规桌面、超宽屏下都保持可读；token detail 不出现新趋势图。
- 至少通过：
  - `cargo test`
  - `cargo clippy -- -D warnings`
  - `cd web && bun test`
  - `cd web && bun run build`
  - `cd web && bun run build-storybook`

## Visual Evidence

- Storybook `Console Home` 宽屏概览
  - 资产：`docs/specs/l9h7v-user-console-overview-progress-cards/assets/console-home-wide.png`
  - 说明：验证 `/console` 全宽 shell、3 张汇总卡、4 张趋势卡、上限虚线与未来空占位。
- Browser preview `Console Home` 汇总数字字形修复
  - 资产：`docs/specs/l9h7v-user-console-overview-progress-cards/assets/console-home-summary-typography.png`
  - 说明：验证 3 张汇总卡改为稳定的静态等宽数字排版，不再出现逐位滚动组件放大后的拆字问题。
- Storybook `Console Home` 390px 窄屏概览
  - 资产：`docs/specs/l9h7v-user-console-overview-progress-cards/assets/console-home-mobile.png`
  - 说明：验证手机断点下的单列堆叠与趋势卡保真展示。
- Storybook `Token Detail` shell 跟随图
  - 资产：`docs/specs/l9h7v-user-console-overview-progress-cards/assets/token-detail-shell.png`
  - 说明：验证 token detail 只跟随新 shell、留白与材质系统，不新增趋势区块。
