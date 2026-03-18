# 配置与访问

## 核心运行参数

| Flag / Env                        | 作用                      |
| --------------------------------- | ------------------------- |
| `--upstream` / `TAVILY_UPSTREAM`  | Tavily 上游 MCP 地址      |
| `--bind` / `PROXY_BIND`           | 监听地址                  |
| `--port` / `PROXY_PORT`           | 监听端口                  |
| `--db-path` / `PROXY_DB_PATH`     | SQLite 文件路径           |
| `--static-dir` / `WEB_STATIC_DIR` | 静态资源目录覆盖          |
| `--keys` / `TAVILY_API_KEYS`      | 可选的一次性 key 导入助手 |

虽然存在 `TAVILY_API_KEYS`，但生产环境仍建议通过管理员 API 或 Web 控制台维护 key 生命周期。

## 管理员访问模型

### ForwardAuth

生产环境推荐使用。

```bash
export ADMIN_AUTH_FORWARD_ENABLED=true
export FORWARD_AUTH_HEADER=Remote-Email
export FORWARD_AUTH_ADMIN_VALUE=admin@example.com
export FORWARD_AUTH_NICKNAME_HEADER=Remote-Name
```

是否具备管理员权限，取决于可信代理注入的请求头值。

### 内置管理员登录

适合自托管或暂时没有独立 ForwardAuth 网关的场景。

```bash
export ADMIN_AUTH_BUILTIN_ENABLED=true
export ADMIN_AUTH_BUILTIN_PASSWORD_HASH='<phc-string>'
export ADMIN_AUTH_FORWARD_ENABLED=false
```

启用后首页会展示管理员登录入口，并通过 HttpOnly cookie 保护 `/admin` 与管理员 API。

## Linux DO OAuth（用户侧）

```bash
export LINUXDO_OAUTH_ENABLED=true
export LINUXDO_OAUTH_CLIENT_ID='<client-id>'
export LINUXDO_OAUTH_CLIENT_SECRET='<client-secret>'
export LINUXDO_OAUTH_REDIRECT_URL='https://<your-host>/auth/linuxdo/callback'
```

行为摘要：

- 首次成功登录会自动创建并绑定 1 个 Hikari access token
- 后续登录复用同一绑定
- 新用户默认没有内置基础额度，需要靠标签或管理员手动发放额度

## 给 HTTP 客户端使用的 token

Tavily Hikari 发放的访问令牌格式为 `th-<id>-<secret>`。\
这个 token 面向终端用户与 HTTP 客户端，而不是直接暴露 Tavily 官方 key。

- 管理员 API 使用管理员鉴权
- `/api/tavily/*` 使用 Hikari access token
- `/mcp` 会先经过 Hikari 的路由与匿名策略，再转发到上游

## 继续阅读

- [HTTP API 指南](/zh/http-api-guide)
- [部署与高匿名](/zh/deployment-anonymity)
- [Storybook 导览](/zh/storybook-guide.html)
