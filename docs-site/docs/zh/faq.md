# FAQ 与排障

## 常见问题

### Tavily Hikari 和 Tavily 官方是什么关系

Tavily Hikari 不是 Tavily 官方服务的替代品，而是部署在你自己的 Tavily key 前面的代理与控制层。

它负责这些事：

- 同时承接 `/mcp` 和 `/api/tavily/*`
- 给终端用户发放 Hikari 自己的 `th-<id>-<secret>` token
- 在多把上游 Tavily key 之间做调度、审计和额度控制

真实的 Tavily key 仍然由你自己持有，并由管理员面管理。

### Hikari token 和 Tavily API key 有什么区别

- Hikari token：发给终端用户、脚本、HTTP 客户端或 MCP 客户端使用
- Tavily API key：只存在于管理员面和上游请求里，不应直接暴露给终端

如果你希望保留密钥池、请求审计和额度控制，就应该把客户端指向 Tavily Hikari，而不是直接发官方 Tavily key。

### 什么时候用 `/mcp`，什么时候用 `/api/tavily/*`

- `/mcp`：给 Codex CLI、Cursor、Claude Desktop、VS Code 这类 MCP 客户端使用
- `/api/tavily/*`：给 Cherry Studio、脚本、自定义后端这类直接走 HTTP JSON API 的客户端使用

如果你的客户端配置界面只要求 `Base URL + API key`，通常就是走 `/api/tavily/*`。

### 如何通过 Hikari 使用 Tavily CLI 或 Agent Skills

安装 Release wrapper：

```bash
curl -fsSL "https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh" | bash -s -- \
  --base-url "https://<your-host>" \
  --token "th-<id>-<secret>"
```

之后运行 `tvly-hikari search "query" --json`。Agent Skills 是可选安装：

```bash
npx skills add https://github.com/IvanLi-CN/tavily-hikari
```

wrapper 会在本地保存 Hikari token，并把官方 `tvly` 命令路由到
`https://<your-host>/api/tavily`。

### 一定要启用 ForwardAuth 吗

不一定，但你至少需要一种管理员访问策略。

- ForwardAuth：生产推荐方案
- 内置管理员登录：适合自托管单实例
- `DEV_OPEN_ADMIN=true`：只适合本地或临时调试

如果没有任何管理员访问策略，你虽然能把服务跑起来，但没法稳定管理上游 key。

### 一定要启用 Linux DO OAuth 吗

不一定。

Linux DO OAuth 主要解决的是“终端用户登录并自动绑定 Hikari token”这件事。如果你的部署只是管理员内部使用，或者 token 由管理员手动发放，可以先不启用它。

### 数据会保存在哪里

长期数据主要保存在 SQLite。

- Flag / Env：`--db-path` / `PROXY_DB_PATH`
- 容器默认路径：`/srv/app/data/tavily_proxy.db`

这份数据库通常会保存 key 状态、用户 token 绑定、请求审计和其他运营相关数据，所以部署时要先想清楚持久化方案。

### 文档站和 Storybook 各自是干什么的

- docs-site：给部署者、操作者、集成方看的说明文档
- Storybook：给你核对 UI 状态、页面空态、对话框和管理员流程的验收面

如果你要接客户端或部署服务，看文档站；如果你要核页面表现，看 Storybook。

## 排障

### `/admin` 打不开，或 `/api/keys` 返回 401

这通常说明管理员访问策略没有配置完整。

先按下面顺序排查：

1. 如果你走 ForwardAuth，确认：
   - `ADMIN_AUTH_FORWARD_ENABLED=true`
   - `FORWARD_AUTH_HEADER` 与 `FORWARD_AUTH_ADMIN_VALUE` 已配置
   - 反向代理确实在请求里注入了匹配的管理员头值
2. 如果你走内置管理员登录，确认：
   - `ADMIN_AUTH_BUILTIN_ENABLED=true`
   - `ADMIN_AUTH_BUILTIN_PASSWORD_HASH` 已配置
   - 如果不用 ForwardAuth，就把 `ADMIN_AUTH_FORWARD_ENABLED=false`
3. 如果你只是本地验证，可以临时用 `DEV_OPEN_ADMIN=true`

补充一点：仓库根目录自带的 `docker-compose.yml` 只会启动 Hikari 本体，不会自动替你配置管理员入口。

### `/api/tavily/*` 返回 401 Unauthorized

优先检查 token 是否真的按 Hikari 规则传入：

- 推荐：`Authorization: Bearer th-<id>-<secret>`
- 对 `POST /api/tavily/*` 这类 JSON 请求，也可以在 body 里放 `api_key`
- 对 `GET /api/tavily/usage` 和 `GET /api/tavily/research/:request_id`，应使用 `Authorization` 头

如果你本地用了 `DEV_OPEN_ADMIN=true`，那只是开发回退路径，不是正式接入方式。

### `/api/tavily/*` 返回 429 Too Many Requests

这通常意味着：

- 当前 Hikari token 达到了小时请求数上限
- 当前 token 的额度已经耗尽

先看用户控制台，或者调用 `GET /api/tavily/usage` 确认当前使用情况；需要时再由管理员调整额度、标签或发放新的 token。

### `/api/tavily/*` 返回 502 Bad Gateway

最常见的原因有两个：

- 当前没有可用的上游 Tavily key
- Hikari 连不到上游地址

先检查：

- 管理员面里是否至少有 1 把可用 key
- `TAVILY_UPSTREAM` 与相关上游地址配置是否正确
- 你当前测试使用的是否是预期的 mock / sandbox / 正式上游

### 重启后 key、token 或审计日志丢了

这通常不是逻辑问题，而是 SQLite 没有持久化。

你需要确认：

- `PROXY_DB_PATH` 指向的目录是持久卷或宿主机挂载目录
- 容器升级或重建时，仍然复用同一份数据库文件

如果你用的是默认容器路径，至少要把 `/srv/app/data` 持久化出来。

### `docker compose up -d` 跑起来了，但首页看不到管理员入口

这是预期行为，不是 bug。

仓库根目录的 `docker-compose.yml` 只负责把 Hikari 拉起来，不会自动启用：

- ForwardAuth
- 内置管理员登录
- Linux DO OAuth

如果你要在这个基础上继续操作，请再补一层管理员访问模型。最短路径看 [部署与高匿名](/zh/deployment-anonymity)。

### 本地点击 Storybook 或文档站跳转打不开

本地交叉跳转依赖两个开发服务都在：

- Storybook：`http://127.0.0.1:56006`
- docs-site：`http://127.0.0.1:56007`

如果你只启动了其中一个，本地回链自然会失败。GitHub Pages 上的最终发布面不会依赖这两个本地端口。

### 想确认高匿名模式到底丢了哪些头

重点看请求审计里的这些字段：

- `forwarded_headers`
- `dropped_headers`

如果你要看完整设计背景，再去读仓库里的
[`docs/high-anonymity-proxy.md`](https://github.com/IvanLi-CN/tavily-hikari/blob/main/docs/high-anonymity-proxy.md)。

## 继续阅读

- [快速开始](/zh/quick-start)
- [配置与访问](/zh/configuration-access)
- [HTTP API 指南](/zh/http-api-guide)
- [部署与高匿名](/zh/deployment-anonymity)
- [Storybook](/zh/storybook.html)
