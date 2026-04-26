# 配置与访问

这页不只是列环境变量，而是帮你回答三件事：

1. 这次部署至少要配哪些参数
2. 管理员到底怎么进来
3. 终端用户和客户端应该拿什么 token

## 先选一种访问模型

Tavily Hikari 至少有两层访问要分清：

- 管理员访问：谁可以进入 `/admin` 和调用 `/api/keys` 这类管理员接口
- 用户 / 客户端访问：谁可以调用 `/mcp` 或 `/api/tavily/*`

推荐决策：

- 本地开发：`DEV_OPEN_ADMIN=true`
- 自托管单实例：内置管理员登录
- 生产网关：ForwardAuth
- 终端用户登录：按需开启 Linux DO OAuth

## 最小运行参数

无论你采用哪种访问模型，下面这组参数都是最核心的：

| Flag / Env                        | 是否通常要关心 | 作用                           |
| --------------------------------- | -------------- | ------------------------------ |
| `--upstream` / `TAVILY_UPSTREAM`  | 是             | Tavily MCP 上游地址            |
| `TAVILY_USAGE_BASE`               | 是             | Tavily HTTP / usage 上游基地址 |
| `--bind` / `PROXY_BIND`           | 是             | 监听地址                       |
| `--port` / `PROXY_PORT`           | 是             | 监听端口                       |
| `--db-path` / `PROXY_DB_PATH`     | 是             | SQLite 数据库路径              |
| `LOW_QUOTA_DEPLETION_THRESHOLD`   | 可选           | 低余额 432 key 阈值            |
| `--static-dir` / `WEB_STATIC_DIR` | 视情况         | 静态前端目录                   |
| `--keys` / `TAVILY_API_KEYS`      | 可选           | 启动时一次性导入 key           |

补充说明：

- `TAVILY_UPSTREAM` 有默认值，通常不需要在本地额外指定；但如果你要接 mock、sandbox 或自建上游，就必须改掉它。
- `TAVILY_USAGE_BASE` 默认是 `https://api.tavily.com`，它影响 usage / 配额同步相关能力。
- `TAVILY_UPSTREAM` 按完整的 MCP 端点解释；如果你的反代保留了 path prefix，配置值里需要包含最终的 `/mcp` 路径。
- `TAVILY_USAGE_BASE` 也可以带 path prefix；Hikari 会在这个 prefix 后继续追加 `/search`、`/extract`、`/crawl`、`/map`、`/research`、`/research/{id}` 与 `/usage`。
- `LOW_QUOTA_DEPLETION_THRESHOLD` 默认是 `15`。当上游 key 返回 432，且最新已知剩余额度小于等于这个值时，Hikari 会在当前 UTC 月把它排除出正常 key 池，但仍允许作为最终兜底。
- `WEB_STATIC_DIR` 不配置时，会自动尝试使用当前仓库下的 `web/dist`。
- `TAVILY_API_KEYS` 只是引导启动时导入 key 的助手，不适合长期运维；长期管理还是用管理员后台或管理员 API。

## 不同部署形态的最小配置

### 本地开发

这是最少配置的路径：

```bash
export DEV_OPEN_ADMIN=true
export PROXY_BIND=127.0.0.1
export PROXY_PORT=58087
```

这时你可以：

- 直接访问管理员接口
- 直接注入第一把上游 Tavily key
- 在本地缺少 token 时走开发回退路径验证 `/mcp` 和 `/api/tavily/*`

这只适用于本地调试，不适合长期运行。

### 自托管单实例

如果没有独立网关，最简单的是启用内置管理员登录：

```bash
export ADMIN_AUTH_BUILTIN_ENABLED=true
export ADMIN_AUTH_BUILTIN_PASSWORD_HASH='<phc-string>'
export ADMIN_AUTH_FORWARD_ENABLED=false
```

这是单机自托管最稳的最小配置。此时管理员通过浏览器登录拿到 HttpOnly cookie，再进入 `/admin` 和管理员 API。

### 生产网关 / 零信任入口

如果前面有可信反代或统一身份网关，推荐直接使用 ForwardAuth：

```bash
export ADMIN_AUTH_FORWARD_ENABLED=true
export FORWARD_AUTH_HEADER=Remote-Email
export FORWARD_AUTH_ADMIN_VALUE=admin@example.com
export FORWARD_AUTH_NICKNAME_HEADER=Remote-Name
```

这时是否具备管理员权限，不看 cookie，而看可信代理注入的请求头值。

## 管理员访问模型

### ForwardAuth

这是生产推荐方案。

关键点只有三个：

1. `FORWARD_AUTH_HEADER` 指向哪个“用户身份头”
2. `FORWARD_AUTH_ADMIN_VALUE` 什么值会被视为管理员
3. 你的反向代理是否真的会把这个头注入到请求里

最容易误解的一点是：

- `ADMIN_AUTH_FORWARD_ENABLED=true` 是默认值
- 但如果你根本没有配置 `FORWARD_AUTH_HEADER`，那它不会凭空产生管理员权限

也就是说，“开启 ForwardAuth 开关”不等于“已经完成管理员鉴权接入”。

### 内置管理员登录

内置管理员登录适合：

- 单实例自托管
- 局域网内使用
- 还没接独立 SSO / ForwardAuth 的场景

必填规则：

- 只要 `ADMIN_AUTH_BUILTIN_ENABLED=true`
- 就必须提供 `ADMIN_AUTH_BUILTIN_PASSWORD` 或 `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`

更推荐：

- 使用 `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`
- 不要长期保留明文 `ADMIN_AUTH_BUILTIN_PASSWORD`

生成 hash 的方式：

```bash
echo -n 'change-me' | cargo run --quiet --bin admin_password_hash
```

### `DEV_OPEN_ADMIN`

这是开发快捷通道：

```bash
export DEV_OPEN_ADMIN=true
```

作用包括：

- 放开管理员接口权限
- 允许本地调试时在没有正式 token 的情况下验证 `/mcp` 与 `/api/tavily/*`

不要把它当成正式访问模型。

## Linux DO OAuth

Linux DO OAuth 只负责用户侧登录，不负责管理员鉴权。

如果你希望终端用户通过网页登录并自动绑定自己的 Hikari token，就启用它：

```bash
export LINUXDO_OAUTH_ENABLED=true
export LINUXDO_OAUTH_CLIENT_ID='<client-id>'
export LINUXDO_OAUTH_CLIENT_SECRET='<client-secret>'
export LINUXDO_OAUTH_REDIRECT_URL='https://<your-host>/auth/linuxdo/callback'
```

这三个参数缺一不可：

- `LINUXDO_OAUTH_CLIENT_ID`
- `LINUXDO_OAUTH_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

其他 OAuth 参数有默认值，只有在你需要自定义 provider 端点时才需要改：

- `LINUXDO_OAUTH_AUTHORIZE_URL`
- `LINUXDO_OAUTH_TOKEN_URL`
- `LINUXDO_OAUTH_USERINFO_URL`
- `LINUXDO_OAUTH_SCOPE`

会话相关参数也可以调整：

- `USER_SESSION_MAX_AGE_SECS`
- `OAUTH_LOGIN_STATE_TTL_SECS`

如果你不启用 Linux DO OAuth，也不影响管理员自己发 token 给客户端使用。

## 客户端到底该拿什么 token

这点要分清楚：

- 管理员接口：用管理员访问模型，不用 Hikari token
- `/api/tavily/*`：用 Hikari token
- `/mcp`：也用 Hikari token

Tavily Hikari 发放的 token 形态是：

```text
th-<id>-<secret>
```

这个 token 才应该交给：

- Cherry Studio
- 脚本
- 自定义后端
- MCP 客户端

真实的 Tavily 官方 key 不应该直接交给这些终端。

## 少见但有用的高级参数

大多数部署不需要改下面这些，但它们是正式支持的运行时契约：

| Flag / Env                     | 作用                                                |
| ------------------------------ | --------------------------------------------------- |
| `XRAY_BINARY`                  | share-link forward proxy 使用的 Xray 可执行文件路径 |
| `XRAY_RUNTIME_DIR`             | Xray 运行时目录                                     |
| `API_KEY_IP_GEO_ORIGIN`        | key 注册 IP 地理信息查询来源                        |
| `ADMIN_MODE_NAME`              | 覆盖管理员模式显示昵称                              |
| `FORWARD_AUTH_NICKNAME_HEADER` | 从网关传入用户昵称显示                              |
| `ADMIN_AUTH_BUILTIN_PASSWORD`  | 内置管理员明文密码，兼容老方式，不推荐长期使用      |

如果你只是本地开发、单机自托管或标准网关部署，通常不需要先碰这些。

## 一页决策总结

如果你现在只想快速做对配置，直接照这个判断：

- 本地开发：`DEV_OPEN_ADMIN=true`
- 单机自托管：`ADMIN_AUTH_BUILTIN_ENABLED=true` + `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`
- 网关接入：`ADMIN_AUTH_FORWARD_ENABLED=true` + `FORWARD_AUTH_HEADER` + `FORWARD_AUTH_ADMIN_VALUE`
- 终端用户网页登录：再额外启用 `LINUXDO_OAUTH_ENABLED=true`
- 客户端接入：始终使用 `th-<id>-<secret>`，不要直接发 Tavily 官方 key

## 继续阅读

- [快速开始](/zh/quick-start)
- [HTTP API 指南](/zh/http-api-guide)
- [部署与高匿名](/zh/deployment-anonymity)
- [FAQ 与排障](/zh/faq)
- [Storybook](/zh/storybook.html)
