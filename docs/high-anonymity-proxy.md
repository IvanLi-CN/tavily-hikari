# Tavily Hikari 高匿代理应用笔记

本笔记记录 Tavily Hikari 在“高匿透明”场景下应如何处理 HTTP 头部，确保上游
Tavily 端点只看到代理自身，好像请求来自真实终端。

## 核心目标

- **自洁透传**：代理在转发前先“擦干净”自身痕迹，再只放行常见的 MCP 请求
  头部。
- **最小暴露**：所有会泄露客户端或代理链路信息的头部（如 `Forwarded`、
  `X-Forwarded-*`、`Via`）被直接丢弃。
- **主机名转换**：`Origin`/`Referer` 等字段会被自动改写为 Tavily 上游域名，
  避免透出访问者的真实站点。

## 流程概览

1. **头部过滤**：允许的标准请求头包括 `Accept`、`Accept-Language`、
   `Accept-Encoding`、`Authorization`、`Content-Type`、`User-Agent` 等；
   同时保留以 `x-mcp-`、`x-tavily-` 开头的业务头部。常见代理/CDN 会附带的
   头（`Forwarded`、`X-Forwarded-*`、`CF-Connecting-IP`、`CF-Ray`、
   `True-Client-IP`、`Fastly-Client-IP`、`Akamai-Origin-Hop`、`CDN-Loop` 等）会被
   直接丢弃，避免从这些字段推断真实来源。
2. **主机名改写**：若请求里包含 `Origin` 或 `Referer`，会将其 scheme、域名
   以及端口改写成 Tavily 上游地址，使其看起来像来自目标站点。
3. **安全默认值**：`Sec-Fetch-Site` 被固定为 `same-origin`，`Host` 与
   `Content-Length` 则交由 `reqwest` 重新计算，确保与上游一致。
4. **透明审计**：数据库 `request_logs` 额外记录 `forwarded_headers` 与
   `dropped_headers` 两列，分别列出最终透传给上游的字段和被代理拦截的字段，便于
   事后排查而不泄露敏感头部值。

## 配置与运行

高匿模式无需额外 CLI 参数，只要正常启动即可：

```bash
cargo run -- --bind 0.0.0.0 --port 58087
# 若需在启动时同步 Tavily API key，可追加 --keys "$TAVILY_API_KEYS"
```

## 验证建议

1. 发送包含 `X-Forwarded-For` 或 `Via` 等头的请求，确认上游不再收到这些头。
2. 对比请求/响应抓包，确保 `Origin`/`Referer` 被重写为 Tavily 上游域名。
3. 使用匿名检测工具验证目标站只看到代理出口 IP。

## 运维提示

- 保持代理部署在可信网络内，避免旁路访问泄露真实信息。
- 业务若确需携带真实 IP，应改用自定义头部（例如 `X-MCP-Client-IP`）并接受
  暴露风险，避免依赖被代理层直接转发链路头。

以上流程确保 Tavily Hikari 在高匿场景下既满足匿名要求，也尊重调用方对
请求内容的控制权。
