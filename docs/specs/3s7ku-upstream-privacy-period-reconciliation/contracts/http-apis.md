# HTTP API contracts

## System settings

`GET/PUT /api/settings/system` 增加：

- `upstreamProjectIdMode: "passthrough" | "fixed" | "accessToken"`
- `upstreamProjectIdFixedValue: string`
- `upstreamMcpUserAgent: string`

默认值分别为 `accessToken`、空字符串、空字符串。`fixed` 模式要求固定值非空且不超过 128 字节；
固定值与 UA 均拒绝控制字符，UA 不超过 256 字节。

## System status

`GET /api/settings/system/status` 为 admin-only，只读返回：

- configured/effective Project ID 与 Header policy；
- UA 是否省略及脱敏后的有效值；
- eligibility gates、gate completion、phase、current period、next epoch；
- active Control session 数；
- pending Research/usage queue 数量与最近 degraded 原因；
- 最近 signed adjustments（token 只显示稳定短 id，upstream key 只显示本地短 id）。

响应不得包含 HMAC secret、官方 API key、完整 Hikari token 或客户端原始 `X-Project-ID`。
