# Tavily Hikari 高匿代理应用笔记

本笔记记录 Tavily Hikari 在“高匿透明”场景下如何约束上游身份暴露面。目标不是伪装成某个具体浏览器，而是把所有 Tavily 出站请求收敛为可审计、可配置、最小暴露的代理身份。

## 核心目标

- **严格白名单**：不同上游路径只允许各自最小必需头部集合，未显式允许的字段一律丢弃。
- **去指纹化**：Tavily HTTP 与 Rebalance MCP HTTP 永远不发送客户端 `User-Agent`；Control MCP 也不会透传客户端 UA，只能使用管理员显式配置的固定值。
- **策略化项目标识**：`X-Project-ID` 不再默认原样上送，而是由系统设置控制为透传、固定名称或按访问令牌分段匿名化。
- **可观测而不泄密**：请求日志只记录最终透传/丢弃了哪些字段名，不记录 HMAC secret、官方 API key、完整 Hikari token 或客户端原始项目标识。

## 流程概览

1. **Tavily HTTP API / Rebalance MCP HTTP**：只允许 `Accept`、`Accept-Encoding`、`Content-Type`，再由策略层决定是否注入 `X-Project-ID`。客户端 `User-Agent`、`Origin`、`Referer`、`Cookie`、`Forwarded`、`X-Forwarded-*`、CDN 头和任意未知 `x-*` 默认全部丢弃。
2. **Control MCP**：只允许 `Accept`、`Accept-Encoding`、`Cache-Control`、`Content-Type`、`Last-Event-Id`、`Mcp-Protocol-Version`、`Mcp-Session-Id`、`Pragma`。`User-Agent` 仅当管理员配置了 `upstreamMcpUserAgent` 时才发送；空值表示完全省略。
3. **`X-Project-ID` 策略**：
   - `passthrough`：客户端提供非空值时原样发送。
   - `fixed`：发送管理员配置的固定名称。
   - `accessToken`：发送 `HMAC-SHA256(secret, "v1" + token_id + period_code)` 的 Base64URL-no-pad 值，默认启用。
4. **窗口化匿名与对账**：`accessToken` 模式按服务器业务时区切成 `00-11`、`11-22`、`22-24` 三段；完整窗口结束后再按实际上游 `/usage` 做一次多退少补。
5. **透明审计**：数据库 `request_logs` 记录 `forwarded_headers` 与 `dropped_headers` 字段名列表；管理端“系统状态”页面展示 configured/effective 策略、门禁、结算队列与最近 adjustment。

## 配置与运行

高匿模式的默认安全姿态来自系统设置，而不是单独 CLI 开关：

```bash
cargo run -- --bind 0.0.0.0 --port 58087
# 若需在启动时同步 Tavily API key，可追加 --keys "$TAVILY_API_KEYS"
```

管理员可在系统设置中调整：

- `X-Project-ID` 模式：`passthrough | fixed | accessToken`
- 固定项目名：仅 `fixed` 模式生效
- Control MCP `User-Agent`：空值表示不发送

状态确认页：

- `/admin/system-settings/status`

## 验证建议

1. 对 Tavily HTTP 或 Rebalance MCP HTTP 请求注入 `User-Agent`、`X-Forwarded-For`、`Cookie`、`Origin` 等字段，确认上游日志中的 `forwarded_headers` 不再包含它们。
2. 切换 `X-Project-ID` 模式并检查 `/admin/system-settings/status`，确认 configured/effective 值与 HTTP allowlist 一致。
3. 在 `accessToken` 模式下跨 `S1/S2/S3` 时间段验证同一 token 的上游项目标识会按窗口变化，而 token secret 轮换不改变匿名身份。
4. 当 API/MCP rebalance 开关都启用且旧 Control session 排空后，检查状态页门禁进入 ready，并在窗口结束后只执行一次结算。

## 运维提示

- 保持代理部署在可信网络内，避免旁路访问绕开 Hikari 的 Header policy。
- `passthrough` 会把客户端 `X-Project-ID` 原样上送，只适合明确需要兼容上游项目统计的场景。
- `accessToken` 只能隐藏项目名与客户端 UA，不能隐藏上游仍可从官方 key、出口 IP、时间分布推导出的统计关联。
- 若业务确需额外业务头，必须先修改代码 allowlist，而不是依赖“未知头默认透传”。

以上流程确保 Tavily Hikari 在高匿场景下默认收敛上游身份暴露面，同时保留管理员可控的兼容开关与可审计状态。
