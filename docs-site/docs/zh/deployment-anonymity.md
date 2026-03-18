# 部署与高匿名

## 推荐部署形态

生产环境通常建议把 Tavily Hikari 部署在可信网关后面，由网关负责 TLS 终止与管理员身份头注入。

典型暴露面包括：

- 公开首页与用户控制台
- `/admin` 管理端
- `/api/tavily/*` 给 HTTP 客户端用
- `/mcp` 给 MCP 流量用

## 高匿名透传

Hikari 支持在转发上游时清洗或重写敏感请求头。

运维建议：

- 只透传必须到达 Tavily 的流量类型
- 按你的匿名策略清洗 `X-Forwarded-*`、`Origin`、`Referer`
- 保持审计日志开启，确保被丢弃与被透传的头部都可追踪

设计背景可参考仓库文档
[`docs/high-anonymity-proxy.md`](https://github.com/IvanLi-CN/tavily-hikari/blob/main/docs/high-anonymity-proxy.md)。

## ForwardAuth 示例

仓库内提供了 Caddy 方案示例：

- [examples/forwardauth-caddy](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)

如果你要快速落一个可信管理员入口，优先从这个示例开始。

## 内置管理员登录注意事项

如果你选择内置管理员登录：

- 优先使用口令哈希，而不是明文环境变量
- 保证 TLS 终止可信，便于 session cookie 正确标记为 `Secure`
- 把它视为自托管便利模式，而不是默认的 zero-trust 生产方案

## 发版形态

主运行时产物是容器镜像：

`ghcr.io/ivanli-cn/tavily-hikari:<tag>`

它内含前端静态资源。公开 docs-site 与 Storybook 则通过 GitHub Pages 单独发布。
