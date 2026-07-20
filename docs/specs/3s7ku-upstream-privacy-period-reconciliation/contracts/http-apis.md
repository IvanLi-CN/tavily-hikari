# HTTP API contracts

## System settings

`GET/PUT /api/settings/system` 增加：

- `upstreamProjectIdMode: "passthrough" | "fixed" | "accessToken"`
- `upstreamProjectIdFixedValue: string`
- `upstreamMcpUserAgent: string`
- `activeUpstreamMcpSessions: number`（只读摘要字段，用于系统设置 warning 入口）

默认值分别为 `accessToken`、空字符串、空字符串。`fixed` 模式要求固定值非空且不超过 128 字节；
固定值与 UA 均拒绝控制字符，UA 不超过 256 字节。

## System status

`GET /api/settings/system/status`（以及兼容别名 `GET /api/settings/system/privacy-status`）为 admin-only，只读返回：

- configured/effective Project ID 与 Header policy；
- UA 是否省略及脱敏后的有效值；
- eligibility gates、gate completion、phase、current period、next epoch；
- `activeUpstreamMcpSessions`；
- pending Research/usage queue 数量与最近 degraded 原因；
- `lastReconciliationRunAt`、`lastShadowAdjustmentAt`、`lastReconciliationEnqueueErrorAt` 三个 compare-only / reconciliation 诊断时间戳；
- 最近 signed adjustments（token 只显示稳定短 id，upstream key 只显示本地短 id）。

响应不得包含 HMAC secret、官方 API key、完整 Hikari token 或客户端原始 `X-Project-ID`。

phase 当前为：

- `configured`: 只完成了静态配置，shadow compare 尚未进入产数状态。
- `compare`: shadow compare 已经产数，但 precise cutover 仍未启用。
- `pending`: precise 前置门禁已经满足，正在等待下一完整业务时间段。
- `active`: precise reconciliation 已启用。
- `degraded`: 至少一个窗口进入 degraded settlement。

## Admin users

`GET /api/users` 在 compare-only 模式下新增 shadow 对账语义字段：

- `shadowDailyCreditsUsed: number | null`
- `shadowDailyAvailability: "confirmed" | "unavailable" | null`

compare-only 时合同固定为：

- `confirmed` 且 `delta != 0`：返回新方案 `24h` 绝对值，并允许 UI 展示相对当前的 secondary delta。
- `confirmed` 且 `delta == 0`：仍返回新方案 `24h` 绝对值，但 secondary delta 为空。
- `unavailable`：`shadowDailyCreditsUsed = null`，owner-facing UI 必须明确显示 unavailable，而不是横杠或当前值。

非 compare-only 路径可以返回 `shadowDailyAvailability = null`，前端不展示该列。

## MCP session bindings

`GET /api/settings/system/mcp-session-bindings` 为 admin-only，返回隐藏管理页所需的分页结果：

- 查询参数：
  - `status=active|revoked|all`
  - `created_from`
  - `created_to`
  - `updated_from`
  - `updated_to`
  - `page`
  - `per_page`
- 时间参数使用 RFC3339 / ISO timestamp。
- 服务端固定按 `updated_at desc` 返回。
- 返回字段：
  - `items[]`
  - `total`
  - `page`
  - `perPage`
  - `activeMatchingCount`

`items[]` 字段固定为：

- `proxySessionId`
- `authTokenId`
- `userId`
- `upstreamKeyId`
- `createdAt`
- `updatedAt`
- `expiresAt`
- `status` (`active|expired|revoked`)
- `revokedAt`
- `revokeReason`

接口与 UI 均不得暴露 raw `upstream_session_id`。

`POST /api/settings/system/mcp-session-bindings/revoke-selected`：

- 请求体：`{ "proxySessionIds": string[] }`
- 只释放命中的活跃 `upstream_mcp` session。
- 单条释放与勾选批量释放共用此接口。

`POST /api/settings/system/mcp-session-bindings/revoke-filtered`：

- 请求体沿用列表筛选字段：`status`、`createdFrom`、`createdTo`、`updatedFrom`、`updatedTo`
- 服务端忽略分页参数，只作用于当前筛选结果中的全部活跃 `upstream_mcp` session。
- 首版不支持独立 `expired` 筛选；`status=all` 时由服务端自行排除不可释放行。
