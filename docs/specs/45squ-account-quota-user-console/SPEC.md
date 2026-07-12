# 账户级配额迁移与登录后用户控制台（#45squ）

## 状态

- Status: 已完成（fast-track）
- Created: 2026-03-02
- Last: 2026-07-09

## 背景 / 问题陈述

- 当前配额判定与展示主要基于 access token 维度，无法沉淀到账户层。
- LinuxDo 登录后仍停留在公共首页，缺少“我的控制台”入口与账户视角指标。
- 用户端需要最小可用的控制台：账户仪表盘 + token 管理（当前每账户仅 1 个 token，只读）。
- 随着自助充值与账号权益叠加进入常态，用户需要在 dashboard 右侧直接完成充值，并通过独立的“权益与订单”页理解当前额度构成、后续月份安排、资费规则与完整订单明细。

## 目标 / 非目标

### Goals

- 新增账户级配额模型（业务配额 hour/day/month + 任意请求 hourly-any）。
- 对历史绑定账户执行一次性回填：把 token 维度已用量/限额迁移到账户维度。
- 已绑定账户 token 请求改为账户级配额判定；未绑定 token 继续 token 级判定。
- 登录后统一进入 `/console`，并在访问 `/` 时对已登录用户自动跳转。
- 新增用户控制台页面：`/console` dashboard、`/console/billing` 权益与订单、`/console/tokens` 与 `/console/tokens/:id`。
- `/console/billing` 必须把当前权益构成、资费规则、未来自然月安排、近期订单与购买动作放在同一页中讲清楚，不混入账号资料语义。
- `/console` dashboard 在充值对当前用户可见时必须保留完整充值卡，复用 billing 页的订单、报价和创建订单状态。

### Non-goals

- 用户侧 token 轮换/禁用/删除（保持只读）。
- 多账户、多角色 RBAC。
- 未绑定 token 强制纳入账户级配额。

## 范围（Scope）

### In scope

- `src/lib.rs`
  - 账户级配额表、回填迁移、判定逻辑。
  - 用户控制台所需查询能力（dashboard/tokens/detail/secret/logs）。
- `src/server.rs`
  - 新增 `/api/user/dashboard`、`/api/user/billing/summary`、`/api/user/tokens*`。
  - 登录后 `/console` 路由与重定向策略。
- `web/`
  - 新入口 `console.html` + `console-main.tsx`。
  - 用户控制台页面（仪表盘、权益与订单、token 管理、token 详情）。

### Out of scope

- 管理员 `/admin` 的功能改造。
- 生产配额参数配置中心化改造。

## 接口契约（Interfaces & Contracts）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 账户已绑定 token 且 token 维度已有配额数据
  When 服务启动并执行一次性回填
  Then 账户级配额已用量与 token 级历史口径一致（允许窗口边界误差 ±1 bucket）。

- Given 已绑定账户 token 达到账户小时/日/月上限
  When 调用 `/mcp` 或 `/api/tavily/*`
  Then 返回 429 且按账户级限额判定。

- Given 未绑定 token
  When 调用 `/mcp` 或 `/api/tavily/*`
  Then 继续走原 token 级判定。

- Given OAuth 登录成功
  When 回调结束
  Then 跳转 `/console`；且已登录用户访问 `/` 自动跳转 `/console`。

- Given 进入 `/console#/dashboard`
  Then 能看到账户维度用量与限额（hourly-any/hour/day/month）。

- Given 进入 `/console/billing`
  Then 能看懂当前总额度里哪些来自基础/长期权益、哪些来自当前月充值、哪些属于当前月调整，并能直接看到资费规则、可横向切换的自然月时间线、近期订单与购买入口。

- Given 当前用户可见充值配置
  When 进入桌面 `/console`
  Then 账户概览右侧显示完整充值卡；billing 页面仍保留完整权益、排期与订单明细。

- Given `/console/billing` 没有未来月份充值排期
  When 用户查看未来安排
  Then 页面仍展示上月 / 本月 / 下月卡片，并用明确提示说明当前没有额外排期中的时效权益，而不是留白。

- Given 进入 `/console/billing` 且时间线包含当前月
  When 页面完成首次布局与滚动位置同步
  Then 当前月卡片默认处于选中状态并驱动下方详情；桌面端仍可同时展示上月 / 本月 / 下月卡片。

- Given 进入 `/console#/tokens`
  Then 能看到 token 列表列（token id、状态与最近使用、成功统计、复制、详情入口），且不再把账户级共享配额或内部备注字段重复渲染到前台 token 行。

- Given 用户在 `/console#/tokens/:id` 点击 `检测 MCP`
  When 浏览器发起 `tools/list` 探测
  Then 请求必须显式声明 `Accept: application/json, text/event-stream`，且前端可同时解析 JSON 与 SSE `data:` 响应体、从 SSE 中提取真正的 JSON-RPC response envelope，并把格式损坏的 2xx 响应判为失败。

- Given token 已达业务配额上限（hour/day/month 任一窗口）
  When 用户在 `/console#/tokens/:id` 点击 `检测 MCP`
  Then UI 先用详情页 quota snapshot 做前置判定；若缓存命中 blocked，会先复核最新 token detail 再决定是否跳过 billable `ping`；若现场 `ping` 返回 `quota_exceeded`，也会立即刷新 detail 供后续点击复用 blocked 状态，并继续执行 `tools/list` 做非计费连通验证，最终给出部分通过或受阻状态。

## 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run build`

## 里程碑

- [x] M1: Spec 与 contracts 建立
- [x] M2: 账户级配额存储与回填迁移落地
- [x] M3: 用户控制台 API 与路由落地
- [x] M4: 前端 `/console` 仪表盘 + token 管理落地
- [x] M5: fast-track 交付（push + PR + checks + review-loop）

## Visual Evidence

- source_type: ui_demo
  demo_entry_or_url: /console/billing?demo=1&announcements=closed
  state: clay-theme-current-month-lifted-with-chevron-navigation
  target_program: mock-only
  capture_scope: billing natural-month timeline
  requested_viewport: 1265x712
  viewport_strategy: ui-demo-source
  sensitive_exclusion: N/A
  PR: include
  evidence_note: verifies the initial desktop timeline leaves the previous and next month unselected while the current month is the only selected card after layout synchronization, using the shared Clay button elevation hierarchy and Lucide chevron month navigation.

![Billing timeline defaults to the current month with Clay lifted state and chevron navigation](./assets/billing-timeline-chevron-navigation.jpg)

- source_type: ui_demo
  demo_entry_or_url: /console/billing?demo=1&announcements=closed
  state: default
  target_program: mock-only
  capture_scope: browser-viewport
  requested_viewport: 1600x1900
  viewport_strategy: ui-demo-source
  sensitive_exclusion: N/A
  PR: include
  evidence_note: verifies the live desktop billing page on the stable announcement-closed demo route. The purchase area has been distilled to three short metrics only, with the misleading long explanatory sentence removed and the pricing semantics kept as "50.00 LDC 可换 1,000 月积分".

![Live desktop billing page demo with clean monthly-credit wording](./assets/console-billing-monthly-credit-proof-wide.png)

- source_type: ui_demo
  demo_entry_or_url: /console/billing?demo=1&announcements=closed
  state: narrow-page-proof
  target_program: mock-only
  capture_scope: browser-viewport
  requested_viewport: 440x5200
  viewport_strategy: ui-demo-source
  sensitive_exclusion: N/A
  PR: include
  evidence_note: verifies the same page at a narrow page-level viewport. The shorter purchase area still reads cleanly in the full mobile stack without reintroducing the removed explanatory paragraph.

![Live narrow billing page proof with readable purchase guide](./assets/console-billing-monthly-credit-proof-narrow.png)

- source_type: ui_demo
  demo_entry_or_url: /console.html?demo=1 (route preloaded to /console/billing)
  state: default
  target_program: mock-only
  capture_scope: browser-viewport
  requested_viewport: 1440x4096
  viewport_strategy: ui-demo-source
  sensitive_exclusion: N/A
  evidence_note: verifies the billing month stage now keeps only one visual emphasis: the selected card. After desktop users select another month, the previously current month falls back to the same quiet unselected treatment as its peers, and the lower detail panel switches to that month while collapsing base access, long-lived entitlements, and tag adjustments into one merged base-entitlements row.

![Live desktop billing page demo](./assets/console-billing-selected-month-desktop.png)

- source_type: ui_demo
  demo_entry_or_url: /console.html?demo=1 (route preloaded to /console/billing)
  state: mobile
  target_program: mock-only
  capture_scope: browser-viewport
  requested_viewport: 390x4096
  viewport_strategy: ui-demo-source
  sensitive_exclusion: N/A
  evidence_note: verifies the same billing page on mobile keeps the month stage as the first surface, advances the selected month with the compact controls, and switches the lower month-detail panel to the newly selected card while keeping the persistent baseline quota as one merged base-entitlements row.

![Live mobile billing page demo](./assets/console-billing-selected-month-mobile.png)

## Visual Evidence (PR)

Storybook `User Console/Billing/Billing Page`: verifies the dedicated `/console/billing` page keeps the clay user-console language while splitting the experience into current composition, pricing rules, natural-month schedule, recent orders, and purchase actions on one screen.

Storybook `User Console/Billing/Billing Page/Mobile`: verifies the same page collapses cleanly on a narrow viewport without losing the information order or purchase affordance.

Storybook `User Console/UserConsole/Console Home Tokens Focus`: verifies the `/console` token list removes the former quota-window column, keeps token status plus recent activity in the list, and still exposes copy/detail/reset actions without horizontal overflow.

![Storybook desktop token list without quota-window column](./assets/console-token-list-storybook-desktop.png)

![Storybook mobile token card without quota-window rows](./assets/console-token-list-storybook-mobile.png)

Local `/console#/tokens`: verifies the live user-console token list matches the Storybook contract on both desktop and mobile, with no repeated account-level shared quota fields in either layout.

![Live desktop /console token list without quota-window column](./assets/console-token-list-live-desktop.png)

![Live mobile /console token card without quota-window rows](./assets/console-token-list-live-mobile.png)

Storybook `User Console/UserConsole/Token Detail Overview`: verifies the MCP probe bubble now uses the same popover-surface tokens in both dark and light themes instead of a hard-coded white balloon.

Storybook `User Console/Fragments/Connectivity Checks/State Gallery`: aggregates the token-detail connectivity fragment across idle, running, success, partial, auth-failed, and quota-blocked states, and explicitly shows the MCP `tools/list` plus full `tools/call` sweep for every advertised tool without relying on separate full-page `UserConsole` stories.

Storybook `User Console/Fragments/Connectivity Checks/State Gallery`: renders MCP `tools/call` rows as structured action copy with a monospace tool-name chip, and widens the probe bubble enough to keep common Tavily tool names on one line while still handling long fixture names cleanly.

![Token detail probe bubble in dark theme](./assets/token-detail-probe-bubble-dark.png)

![Token detail probe bubble in light theme](./assets/token-detail-probe-bubble-light.png)

- source_type: storybook_canvas
- target_program: mock-only
- capture_scope: browser-viewport
- sensitive_exclusion: N/A
- submission_gate: approved
- story_id_or_title: user-console-fragments-connectivity-checks--state-gallery
- scenario: MCP full sweep + structured tool-call copy
- evidence_note: verifies the connectivity gallery shows the shortened MCP tool-call copy, renders the tool name as a distinct code-styled fragment, and keeps the long-name fixture readable without falling back to the old `MCP 工具调用 · {tool}` phrasing.
- image:
  ![Connectivity checks state gallery with structured MCP tool rows](./assets/connectivity-checks-state-gallery-tool-copy.png)

## 变更记录

- 2026-07-09: 继续压缩 `/console/billing` 购买区文案，移除购买说明中的整句规则解释与测试价格提示，只保留三项短指标和充值面板内的额度增量，避免在最终页面里继续堆叠长文本。
- 2026-07-09: 修正 `/console/billing` 购买说明文案语义，不再把 `1,000 月积分` 与 `50 LDC` 写成等价物；页面统一改成“月积分档位 / 生效月数”口径，并把被公告弹窗遮挡的旧证据替换为 `announcements=closed` 的干净整页证据。
- 2026-03-18: 补充 Token Detail probe 实调回归覆盖；前端将 MCP/API 检测步骤抽成共享定义并用请求级测试锁定浏览器侧调用，后端再新增真实合同测试，直接拉起 Hikari 验证 `/mcp tools/list` 返回的全部广告工具都能继续通过 `/mcp tools/call` 命中 mock upstream，同时覆盖 `/api/tavily/search|extract|crawl|map|research|research/:id` 的真实调用链；运行时与 Storybook 的 MCP probe 也已改成发现后逐个 `tools/call`，并把 probe 状态收敛为独立 `Connectivity Checks` gallery，移除仅为展示 probe 状态存在的全页 `UserConsole` stories。
- 2026-07-09: Billing 月份卡片取消“当前月”独立高亮；当前月只保留默认选中语义，未选中时与其他月份保持同一静态层级，相关桌面/移动端视觉证据已更新。
- 2026-07-12: 固定 Billing 时间线首次布局后的默认选择为当前月，避免初始滚动同步回退到上月；桌面三卡视图保持不变。
- 2026-07-09: 所选月份详情把“基础接入额度 / 长期权益 / 标签调整”合并为统一的“基础权益”行，月度调整与充值权益继续独立展示，相关桌面/移动端视觉证据已同步更新。
- 2026-07-08: 用户控制台新增独立 `/console/billing` 页面与顶部稳定入口；前端将权益构成、资费规则、未来自然月安排、近期订单与购买动作汇总为同页信息架构，后端补充 `/api/user/billing/summary` 供页面读取安全摘要。
- 2026-03-21: 优化 Token Detail 的 MCP 检测气泡显示；`tools/call` 项统一改成“调用 {tool} 工具”式文案，工具名以 monospace chip 独立展示，并放宽气泡宽度以减少常见 Tavily 工具名的割裂换行。
- 2026-06-20: 收紧 `/console` Token 列表语义，移除列表中的“配额窗口”整列与移动端对应 quota-window 条目；列表仅保留 token 自有识别信息、状态/最近使用、成功统计与操作入口，账户级共享配额回归到账户概览与 token 详情承载。
- 2026-06-20: 补充前台展示约束，用户控制台 Token 列表不再展示内部 `note/备注` 字段；相关视觉证据已更新为去备注后的桌面/移动最终态。
- 2026-03-09: 修复 Token Detail 探测气泡的暗色主题表面配色，并补充 Storybook 暗色/浅色双截图作为最新验收依据。
- 2026-03-07: 补充 Storybook 视觉证据，固定 `Quota Blocked` 场景截图到 spec 资产，用于 PR 合并前验收。
- 2026-03-06: 修复用户控制台 MCP 探测合同：浏览器 probe 显式发送双 Accept，兼容 SSE `tools/list`（含通知夹杂场景）并拒绝格式损坏的 2xx 成功体，同时在 token 配额耗尽时前置标记为受阻而非误报全失败，并在缓存配额可能过期时先复核最新 detail。
- 2026-03-02: 初始化规格，冻结范围、接口与验收口径。
- 2026-03-02: 完成账户级配额迁移与用户控制台交付，补齐登录跳转与用户接口链路。
- 2026-03-02: 完成 review-loop 收敛，修复快照查询批量化、详情页切换闪旧数据与登录回退边界。
