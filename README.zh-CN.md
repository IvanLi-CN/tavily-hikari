# Tavily Hikari

[![Release](https://img.shields.io/github/v/release/IvanLi-CN/tavily-hikari?logo=github)](https://github.com/IvanLi-CN/tavily-hikari/releases)
[![CI Pipeline](https://github.com/IvanLi-CN/tavily-hikari/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/IvanLi-CN/tavily-hikari/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-1.91%2B-orange?logo=rust)](rust-toolchain.toml)
[![Frontend](https://img.shields.io/badge/Vite-5.x-646CFF?logo=vite&logoColor=white)](web/package.json)

Tavily Hikari 是一个面向 MCP (Model Context Protocol) 的 Tavily 代理层，基于 Rust + Axum 构建，具备多密钥轮询、匿名透传与细粒度审计能力。后端通过 SQLite 维护密钥状态与请求日志，前端使用 React + Vite 提供实时的可视化运维界面，可直接查看 Key 健康、告警与历史流量。

## 功能亮点

- **多密钥轮询**：SQLite 记录最近使用时间，代理端始终挑选“最久未使用”的 Key，均衡配额并防止单 Key 被打爆。
- **短 ID 与密钥密级隔离**：每个 Tavily Key 会生成 4 位 nanoid，对外只暴露短 ID；真实 Key 仅管理员 API/Web 控制台可读取。
- **健康巡检**：一旦收到 Tavily 432（额度耗尽）会把 Key 标记为 `exhausted`，并在下一个 UTC 月初或管理员恢复后重新上阵。
- **高匿透传**：仅透传 `/mcp` 与静态资源，自动清洗 `X-Forwarded-*` 等敏感头并重写 `Origin/Referer`，细节见 [`docs/high-anonymity-proxy.md`](docs/high-anonymity-proxy.md)。
- **可视化运维**：`web/` 单页应用展示实时统计、请求日志、管理员操作入口，支持复制真实 Key、软删除/恢复等动作。
- **完整审计**：`request_logs` 表保留 method/path/query、状态码、错误信息、透传/丢弃头部等字段，方便回溯配额损耗与异常请求。
- **生产级 CI/CD**：GitHub Actions 对代码格式、lint、单元测试、release 镜像打包全流程把关，镜像发布至 `ghcr.io`。

## 组件与数据流

```
Client → Tavily Hikari (Axum) ──┬─> Tavily upstream (/mcp)
                                ├─> SQLite (api_keys, request_logs)
                                └─> Web SPA (React/Vite, served via /)
```

- 后端：Rust 2024 edition、Axum、SQLx、Tokio；负责 CLI、Key 生命周期、请求透传/审计、静态资源托管。
- 数据层：SQLite 单文件库，包含 `api_keys`（状态、短 ID、配额字段）与 `request_logs`（请求/响应/错误）。
- 前端：React 18 + TanStack Router + Tailwind CSS + DaisyUI + Vite 5；构建后输出 `web/dist`，由后端静态挂载或通过 Vite Dev Server 代理到 `http://127.0.0.1:58087`。

## 快速开始

### 本地运行

```bash
# 1. 启动代理（示例绑定高位端口）
cargo run -- --bind 127.0.0.1 --port 58087

# 2. （可选）启动前端 Dev Server
cd web && npm ci && npm run dev -- --host 127.0.0.1 --port 55173

# 3. 通过管理员接口注册 Tavily key（ForwardAuth 头视部署而定）
curl -X POST http://127.0.0.1:58087/api/keys \
  -H "X-Forwarded-User: admin@example.com" \
  -H "X-Forwarded-Admin: true" \
  -H "Content-Type: application/json" \
  -d '{"api_key":"key_a"}'
```

服务启动后可访问 `http://127.0.0.1:58087/health` 验证状态，或在浏览器打开 `http://127.0.0.1:55173` 使用控制台。所有 Tavily key 建议通过管理员 API 或 Web 控制台录入，避免把敏感密钥写入环境变量。

### Docker 部署

CI 在发布时会产出 `ghcr.io/ivanli-cn/tavily-hikari:<tag>` 镜像，可直接运行：

```bash
docker run --rm \
  -p 8787:8787 \
  -v $(pwd)/data:/srv/app/data \
  ghcr.io/ivanli-cn/tavily-hikari:latest
```

镜像已包含 `web/dist`，默认监听 `0.0.0.0:8787` 并把 SQLite 数据写入 `/srv/app/data/tavily_proxy.db`（可通过挂载卷持久化）。容器启动后同样需通过管理员接口或前端控制台为代理注册 Tavily key。

### Docker Compose

仓库内提供了一个最小化的 [`docker-compose.yml`](docker-compose.yml)，用于长期运行或一次性 POC：

```bash
docker compose up -d

# 以管理员身份注入首批 Tavily key
curl -X POST http://127.0.0.1:8787/api/keys \
  -H "X-Forwarded-User: admin@example.com" \
  -H "X-Forwarded-Admin: true" \
  -H "Content-Type: application/json" \
  -d '{"api_key":"key_a"}'
```

- 服务会自动使用 `ghcr.io/ivanli-cn/tavily-hikari:latest`，将 8787 端口暴露到宿主机。
- 通过 `tavily-hikari-data` 卷持久化 `/srv/app/data/tavily_proxy.db`，容器重启不会丢数据。
- 其他 CLI 参数可通过 compose 文件的 `environment` 字段覆写（例如自定义 upstream 或端口）。

若需要运行自定义镜像，可在 compose 文件里将 `image` 替换为 `build: .` 并在本地构建 `web/dist` 后执行 `docker compose up --build`。

## CLI / 环境变量

| Flag / Env                                                        | 说明                                                                                                                         |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `--keys` / `TAVILY_API_KEYS`                                      | Tavily API key 列表（可选），支持逗号分隔或多次传参，仅用于一次性导入或开发场景；生产环境推荐通过管理员 API/前端控制台录入。 |
| `--upstream` / `TAVILY_UPSTREAM`                                  | Tavily MCP 上游地址，默认 `https://mcp.tavily.com/mcp`。                                                                     |
| `--bind` / `PROXY_BIND`                                           | 监听地址，默认 `127.0.0.1`。                                                                                                 |
| `--port` / `PROXY_PORT`                                           | 监听端口，默认 `8787`。建议开发期使用高位端口（如 `58087`）。                                                                |
| `--db-path` / `PROXY_DB_PATH`                                     | SQLite 文件路径，默认 `tavily_proxy.db`。                                                                                    |
| `--static-dir` / `WEB_STATIC_DIR`                                 | Web 静态目录，若缺省且存在 `web/dist` 会自动挂载。                                                                           |
| `--forward-auth-header` / `FORWARD_AUTH_HEADER`                   | 指定 ForwardAuth 注入的“用户标识”请求头（如 `Remote-Email`）。                                                               |
| `--forward-auth-admin-value` / `FORWARD_AUTH_ADMIN_VALUE`         | 匹配到该值时视为管理员，可访问 `/api/keys/*` 接口。                                                                          |
| `--forward-auth-nickname-header` / `FORWARD_AUTH_NICKNAME_HEADER` | 可选，提供 UI 展示的昵称头（如 `Remote-Name`）。                                                                             |
| `--admin-mode-name` / `ADMIN_MODE_NAME`                           | 当缺少昵称头时用于覆盖前端显示的管理员名称。                                                                                 |
| `--dev-open-admin` / `DEV_OPEN_ADMIN`                             | 仅限本地调试的开关，跳过管理员校验（默认 `false`）。                                                                         |

首次运行会自动建表。若在 CLI/环境变量里显式传入 `--keys` 或 `TAVILY_API_KEYS`，会同步 `api_keys` 表：**在列表中**的 Key 会被新增或恢复为 `active`；**不在列表中**的 Key 会被标记为 `deleted`。默认推荐通过管理员 API/前端控制台维护 Key 集合。

## HTTP API 速览

| Method   | Path                   | 说明                                                             | 认证        |
| -------- | ---------------------- | ---------------------------------------------------------------- | ----------- |
| `GET`    | `/health`              | 健康检查，返回 200 代表代理可用。                                | 无          |
| `GET`    | `/api/summary`         | 汇总成功/失败次数、活跃 Key 数、最近活跃时间。                   | 无          |
| `GET`    | `/api/keys`            | 列出 4 位短 ID、状态、请求统计。                                 | 无          |
| `GET`    | `/api/logs?limit=50`   | 最近请求日志（默认 50 条），包含状态码与错误。                   | 无          |
| `POST`   | `/api/keys`            | 管理员接口，新增或“反删除”一个 Key。Body: `{ "api_key": "..." }` | ForwardAuth |
| `DELETE` | `/api/keys/:id`        | 管理员接口，软删除指定短 ID。                                    | ForwardAuth |
| `GET`    | `/api/keys/:id/secret` | 管理员接口，返回真实 Tavily Key。                                | ForwardAuth |

管理员身份由外层 ForwardAuth 注入的请求头判断；控制台仅在管理员会话下显示“复制原始 Key”按钮。

## 密钥生命周期 & 审计

- **额度感知**：当 Tavily 返回 432 时会自动将 Key 标记为 `exhausted`，轮询器将跳过该 Key，直到 UTC 月初或手动恢复。
- **调度算法**：优先选择最久未使用的 `active` Key；若全部被禁用则按照禁用时间回退，避免请求被直接拒绝。
- **日志字段**：`request_logs` 记录 method/path/query、上游响应体、状态码、错误堆栈、透传/丢弃头部，便于配额排障。
- **匿名策略**：详见 [`docs/high-anonymity-proxy.md`](docs/high-anonymity-proxy.md)，包括允许/丢弃的头部列表、主机名改写策略等。

## ForwardAuth 配置

代理本身通过 ForwardAuth 提供的请求头判断操作者身份，可通过环境变量/CLI 配置：

```bash
export FORWARD_AUTH_HEADER=Remote-Email
export FORWARD_AUTH_ADMIN_VALUE=xxx@example.com
export FORWARD_AUTH_NICKNAME_HEADER=Remote-Name
```

- `FORWARD_AUTH_HEADER` 指定哪一个请求头携带用户邮箱或 ID。
- 当该头的值等于 `FORWARD_AUTH_ADMIN_VALUE` 时，会授予管理员权限，从而允许访问 `/api/keys` 相关接口。
- `FORWARD_AUTH_NICKNAME_HEADER`（可选）会透传到前端，用于显示操作员昵称；缺省时可在 `ADMIN_MODE_NAME` 中设置固定昵称。
- 本地快速验证可以临时设置 `DEV_OPEN_ADMIN=true`，生产环境务必保持默认的安全策略。

## 前端控制台

- 构建产物位于 `web/dist`，可由后端直接托管或独立静态站点部署。
- 通过 React + TanStack Router 实现实时仪表盘：Key 列表、状态筛选、请求日志流式刷新。
- DaisyUI + Tailwind 提供深浅色主题，Iconify 提供图标，自带版本号展示（`scripts/write-version.mjs` 会把版本写入构建结果）。
- 开发期 `npm run dev` 会把 `/api`、`/mcp`、`/health` 请求代理到后端，减少 CORS 与鉴权配置成本。

## 开发与测试

- **Rust**：固定使用 1.91.0（见 `rust-toolchain.toml`）。
  - `cargo fmt` / `cargo clippy -- -D warnings` / `cargo test --locked --all-features`。
  - `cargo run -- --help` 查看完整 CLI。
- **前端**：Node 20 + pnpm/npm 均可，推荐 `npm ci`；`npm run build` 会串行执行 `tsc -b` 与 `vite build`。
- **Git Hooks**：运行 `lefthook install` 后，每次提交会自动执行 `cargo fmt`、`cargo clippy`、`npx dprint fmt` 与 `npx commitlint --edit`，确保遵循 Conventional Commits（英文）。
- **CI**：`.github/workflows/ci.yml` 包含 lint、测试、PR 构建、release 打包与 GHCR 推送，可据此了解默认流水线。

## 生产部署提示

1. 仅开放 `/mcp`、`/api/*`、静态资源；其余路径默认 404，若前面挂有 Nginx/Cloudflare，确保不要把 `/mcp` 之外的入口暴露到上游。
2. 结合 ForwardAuth 或其他零信任代理限制管理接口；普通用户不应看见真实 Key。
3. 若需更强匿名性，请按照 [`docs/high-anonymity-proxy.md`](docs/high-anonymity-proxy.md) 的头部清洗策略部署，并确认 `Origin/Referer` 已被改写。
4. 建议把 SQLite 放在持久卷或外部存储中，并定期导出 `request_logs` 以满足审计合规。

## 附加资料

- [`docs/high-anonymity-proxy.md`](docs/high-anonymity-proxy.md)：高匿名场景下的头部处理策略。
- `Dockerfile`：多阶段构建示例，可参考自定义镜像流程。
- `web/README`（如存在）：更细的前端说明。

## License

Distributed under the [MIT License](LICENSE)。在使用、复制或分发时请保留许可声明。
