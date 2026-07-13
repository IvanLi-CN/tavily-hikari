# 快速开始

## 先选一种启动方式

- 本地开发：最快，适合先确认后端、前端和 HTTP 代理是否正常。
- 单容器 POC：最快看到发布镜像的实际形态。
- Docker Compose / ForwardAuth：适合长期运行或准备接到公网入口。

## 前置依赖

- Rust `1.91+`
- Bun `1.3.10` 或与仓库锁定版本一致的更新版本
- 本地 Rust 构建所需的 SQLite 运行时依赖

## 本地开发（最快）

```bash
# 后端；DEV_OPEN_ADMIN 只用于本地调试
DEV_OPEN_ADMIN=true cargo run -- --bind 127.0.0.1 --port 58087

# 前端 dev server
cd web
bun install --frozen-lockfile
bun run --bun dev -- --host 127.0.0.1 --port 55173
```

验证地址：

- 后端健康检查：`http://127.0.0.1:58087/health`
- 前端控制台：`http://127.0.0.1:55173`

补充说明：

- `DEV_OPEN_ADMIN=true` 会放开管理员接口，也允许 `/mcp` 与 `/api/tavily/*` 在本地缺少 token 时走开发回退路径。
- 这个开关只适合本地验证；长期运行时请改用 ForwardAuth 或内置管理员登录。

## 注入第一把 Tavily key

```bash
curl -X POST http://127.0.0.1:58087/api/keys \
  -H "Content-Type: application/json" \
  -d '{"api_key":"tvly-..."}'
```

确认 key 已进入池子：

```bash
curl http://127.0.0.1:58087/api/keys
```

如果你不打算使用 `DEV_OPEN_ADMIN=true`，那就需要先满足下面三选一中的任意一种：

- 外层已配置 ForwardAuth，并且 `FORWARD_AUTH_HEADER` / `FORWARD_AUTH_ADMIN_VALUE` 已正确匹配。
- 已启用内置管理员登录，并先在浏览器完成管理员登录。
- 你临时把 `FORWARD_AUTH_HEADER=X-Forwarded-User`、`FORWARD_AUTH_ADMIN_VALUE=admin@example.com`
  注入到本地进程，再用 `X-Forwarded-User: admin@example.com` 这类头做调试。

## 立刻验证 HTTP 代理

在 `DEV_OPEN_ADMIN=true` 的本地模式下，可以直接发第一条搜索请求：

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust async runtime",
    "topic": "general",
    "search_depth": "basic",
    "max_results": 3
  }'
```

如果你已经在正常鉴权模式下创建了用户 token，则推荐改用：

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Authorization: Bearer th-<id>-<secret>" \
  -H "Content-Type: application/json" \
  -d '{"query":"rust async runtime","topic":"general"}'
```

这一步成功后，说明你已经打通了：

- 服务启动
- 管理员注入上游 Tavily key
- Hikari 到 Tavily HTTP facade 的完整请求链路

## 安装 CLI wrapper

拿到 Hikari 用户 token 后，可以安装 GitHub Release 分发的 wrapper：

```bash
curl -fsSL "https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh" | bash -s -- \
  --base-url "http://127.0.0.1:58087" \
  --token "th-<id>-<secret>"
```

之后通过 Hikari 运行官方 Tavily CLI 命令：

```bash
tvly-hikari search "rust async runtime" --json
```

可选安装 Agent Skills：

```bash
npx skills add https://github.com/IvanLi-CN/tavily-hikari --global
```

CLI 会以 `0600` 权限保存 Hikari token，并把流量发到 `<base-url>/api/tavily`。不要把 Tavily
官方 API key 当成下游 token 使用。

## 单容器 POC

```bash
docker run --rm \
  -p 8787:8787 \
  -e PROXY_BIND=0.0.0.0 \
  -e DEV_OPEN_ADMIN=true \
  -v "$(pwd)/data:/srv/app/data" \
  ghcr.io/ivanli-cn/tavily-hikari:latest
```

镜像内已包含 `web/dist`，SQLite 数据默认写入 `/srv/app/data/tavily_proxy.db`。

验证方式与本地开发相同，只是把地址换成 `http://127.0.0.1:8787`：

```bash
curl http://127.0.0.1:8787/health
curl -X POST http://127.0.0.1:8787/api/keys \
  -H "Content-Type: application/json" \
  -d '{"api_key":"tvly-..."}'
```

这条容器命令仍然使用 `DEV_OPEN_ADMIN=true`，所以它只是 POC 路径，不是生产建议。

## Docker Compose

```bash
docker compose up -d
```

仓库自带的 `docker-compose.yml` 会：

- 暴露 `8787`
- 挂载 `tavily-hikari-data` volume
- 使用 `ghcr.io/ivanli-cn/tavily-hikari:latest`
- 把 SQLite 数据持久化到 `/srv/app/data/tavily_proxy.db`

但它只启动 Hikari 本体，不会自动帮你提供管理员入口。所以第一次跑 compose 时，你需要再选一种管理方式：

- 临时本地验证：在 compose 环境变量里加 `DEV_OPEN_ADMIN=true`
- 自托管：启用内置管理员登录
- 网关模式：直接使用仓库中的
  [examples/forwardauth-caddy](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)

## 需要公网入口时，直接走 ForwardAuth 示例

如果你已经准备让管理员通过反向代理进入 Hikari，不要从零拼网关配置，直接从仓库现成样例开始：

```bash
cd examples/forwardauth-caddy
docker compose up -d
```

这个样例会同时拉起：

- Caddy 网关
- `auth-mock`，负责模拟 ForwardAuth
- `upstream-mock`，负责模拟 Tavily 上游
- Tavily Hikari

它的目标是先把“网关 + 管理员身份头 + Hikari”这一套链路跑通，再替换成你自己的上游与认证系统。

## 可选的本地验收面

```bash
# Storybook
cd web
bun install --frozen-lockfile
bun run storybook

# docs-site
cd docs-site
bun install --frozen-lockfile
bun run dev
```

- Storybook 默认地址：`http://127.0.0.1:56006`
- docs-site 默认地址：`http://127.0.0.1:56007`

这两个本地服务会互相回链，行为与最终 GitHub Pages 发布面保持一致。

## 下一步

- 需要完整的鉴权、内置管理员登录和 Linux DO OAuth 说明，看 [配置与访问](/zh/configuration-access)
- 需要完整的 HTTP 接入示例，看 [HTTP API 指南](/zh/http-api-guide)
- 需要生产部署与高匿名建议，看 [部署与高匿名](/zh/deployment-anonymity)
- 如果你已经跑起来，但卡在 401、502、管理员入口或持久化问题，看 [FAQ 与排障](/zh/faq)
