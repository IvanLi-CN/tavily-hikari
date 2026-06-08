# 部署与高匿名

## 先选部署形态

你基本会落在下面三种形态之一：

- 本机或单机容器 POC：先确认镜像、控制台和接口能跑起来
- 自托管长期运行：自己终止 TLS，并使用内置管理员登录
- 网关模式：把 Hikari 放在 Caddy / Nginx / Traefik 等反代后面，通过 ForwardAuth 注入管理员身份头

## 最小运行参数

无论你选哪种形态，下面这组参数是最核心的：

| Flag / Env                        | 说明                           |
| --------------------------------- | ------------------------------ |
| `--bind` / `PROXY_BIND`           | 服务监听地址                   |
| `--port` / `PROXY_PORT`           | 服务监听端口                   |
| `--db-path` / `PROXY_DB_PATH`     | SQLite 数据库路径              |
| `--static-dir` / `WEB_STATIC_DIR` | 前端静态资源目录               |
| `--upstream` / `TAVILY_UPSTREAM`  | Tavily MCP 上游地址            |
| `TAVILY_USAGE_BASE`               | Tavily HTTP / usage 上游基地址 |

然后你还需要再补一类“管理员访问策略”：

- ForwardAuth：推荐给生产或零信任网关
- 内置管理员登录：适合自托管单实例
- `DEV_OPEN_ADMIN=true`：只适合本地/临时验证

## 最小 Compose 部署

仓库根目录自带的 [`docker-compose.yml`](https://github.com/IvanLi-CN/tavily-hikari/blob/main/docker-compose.yml)
会直接启动 Hikari：

```bash
docker compose up -d
curl -i http://127.0.0.1:8787/health
```

这个 compose 文件已经帮你做了：

- 监听 `0.0.0.0:8787`
- 挂载 `tavily-hikari-data` volume
- 把数据库持久化到 `/srv/app/data/tavily_proxy.db`
- 使用镜像 `ghcr.io/ivanli-cn/tavily-hikari:latest`

但它不会帮你自动提供管理员入口，所以第一次上线前还要补一层：

- 临时本地验证：自己在环境变量里加 `DEV_OPEN_ADMIN=true`
- 自托管：启用内置管理员登录
- 正式网关：换成 `examples/forwardauth-caddy`

## ForwardAuth 网关示例

生产环境通常建议把 Tavily Hikari 部署在可信网关后面，由网关负责 TLS 终止与管理员身份头注入。

仓库现成样例在：

- [examples/forwardauth-caddy](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)

直接启动：

```bash
cd examples/forwardauth-caddy
docker compose up -d
```

这个示例会拉起：

- Caddy 网关
- `auth-mock`，负责模拟 ForwardAuth
- `upstream-mock`，负责模拟 Tavily 上游
- Tavily Hikari

默认行为：

- `GET /health` 公开可访问
- 其余路径都要先通过 Basic Auth
- 认证成功后，Caddy 会把 `Remote-Email`、`Remote-Name` 转发给 Hikari
- Hikari 会把 `Remote-Email=admin@example.com` 视为管理员

如果你想先验证网关链路，而不是马上连真实 Tavily 与真实 SSO，这个示例就是最短路径。

## 内置管理员登录自托管

如果你没有独立的 ForwardAuth 网关，可以直接开启内置管理员登录。

推荐做法：

```bash
export ADMIN_AUTH_BUILTIN_ENABLED=true
echo -n 'change-me' | cargo run --quiet --bin admin_password_hash
export ADMIN_AUTH_BUILTIN_PASSWORD_HASH='<phc-string>'
export ADMIN_AUTH_FORWARD_ENABLED=false
```

部署要点：

- 优先使用 `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`，不要长期保留明文密码
- 确保 TLS 终止可信，这样 session cookie 才能稳定带 `Secure`
- 把它视为自托管便利模式，而不是默认生产零信任方案

## 真实上线前的检查项

- `/health` 返回 200
- 至少注册 1 把上游 Tavily key
- 管理员能进入 `/admin` 或调通 `/api/keys`
- 至少成功跑通 1 次 `/api/tavily/search` 或 `/mcp`
- 数据库目录已经持久化，不会因为容器重启而丢失

## 持久化、备份与升级

需要长期保留的数据核心就是 SQLite 文件：

- 默认容器路径：`/srv/app/data/tavily_proxy.db`
- 升级前建议先备份这个文件
- 容器镜像本身是无状态的，升级通常就是换 tag 后重启
- 如果你还维护了 Caddy / 反代配置，也应该把这部分一起纳入备份

## SQLite 维护窗口建议

如果你遇到 `database is locked`、`quota_sync` 长时间停在 `running`、或者 request logs backlog
很大，建议把恢复步骤固定成同一套剧本：

1. 发布新镜像后做一次受控重启
2. 验证 `/health` 返回 `200`
3. 检查 `scheduled_jobs` 里没有新的长时间 `quota_sync*` `running`
4. 检查日志里 `database is locked` 不再持续爆发
5. 需要继续清理日志 backlog 时，优先运行 `request_logs_gc_once`
6. 只有在 `reclaimable_bytes >= 512MB` 或你明确要开维护窗口时，再运行 `db_compaction_once`

容器镜像内置了两个 operator CLI：

```bash
request_logs_gc_once --json
db_compaction_once --json
db_compaction_once --json --force
```

说明：

- `request_logs_gc_once` 用于 bounded 地清理 request logs / body backlog
- `db_compaction_once` 用于 SQLite 文件压缩；默认会尊重 reclaimable space 阈值，不满足条件时返回
  `skipped=true`
- `db_compaction_once --force` 只建议在明确的维护窗口里使用

## 高匿名透传

Hikari 支持在转发上游时清洗或重写敏感请求头。

它会重点处理这些事：

- 丢弃 `Forwarded`、`X-Forwarded-*`、`Via`、`CF-*` 等链路暴露头
- 需要时改写 `Origin`、`Referer`
- 在数据库里记录 `forwarded_headers` 与 `dropped_headers`，方便排障

设计背景与更细的匿名策略说明，见：

[`docs/high-anonymity-proxy.md`](https://github.com/IvanLi-CN/tavily-hikari/blob/main/docs/high-anonymity-proxy.md)

## 暴露面建议

典型暴露面包括：

- 公开首页与用户控制台
- `/admin` 管理端
- `/api/tavily/*` 给 HTTP 客户端用
- `/mcp` 给 MCP 流量用

## 发版形态

主运行时产物是容器镜像：

`ghcr.io/ivanli-cn/tavily-hikari:<tag>`

它内含前端静态资源。公开 docs-site 与 Storybook 则通过 GitHub Pages 单独发布。

如果你部署后卡在管理员入口、数据库持久化或上游 `502`，继续看 [FAQ 与排障](/zh/faq)。
