# Tavily Hikari

Tavily Hikari 是一个轻量级的反向代理：它会把来自客户端的请求透传至官方 `https://mcp.tavily.com/mcp` 端点，同时对 Tavily API key 进行轮询、健康标记与旁路记录。

## 特性

- 多 key 轮询：SQLite 记录最近使用时间，确保 Tavily API key 均衡出站。
- 短 ID 主键：为每个 key 生成 4 位 nanoid 作为主键，对外展示安全短 ID，真实 API key 仅管理员可取回。
- 透传代理：对外保持与官方端点兼容的请求/响应，额外附加 `tavilyApiKey` 查询参数与 `Tavily-Api-Key` 请求头。
- 旁路审计：每次请求的 method/path/query、状态码、错误信息与响应体都会写入数据库，方便后续诊断配额情况。
- 健康标记：检测到状态码 432 时自动把对应 key 标记为“额度用尽”，UTC 月初再恢复。
- 简单部署：通过 CLI 指定监听地址、上游端点、数据库路径即可运行。
- Web 控制台：构建 `web/dist` 后可直接挂载单页应用，实时查看 key 状态与最近代理请求。
- 仅透传 `/mcp` 路径：除 `/mcp` 与静态资源外，其余请求在本地响应 404，避免意外直连上游。

## 快速开始

```bash
cd tavily-hikari

# 1. （可选）在 .env 中维护密钥，或导出 Tavily API 密钥（逗号分隔或重复传参皆可）
echo 'TAVILY_API_KEYS=key_a,key_b,key_c' >> .env
# export TAVILY_API_KEYS="key_a,key_b,key_c"

# 2. 启动反向代理（开发期建议使用高位端口）
cargo run -- --bind 127.0.0.1 --port 58087
# 代理地址为 http://127.0.0.1:58087，与 Tavily MCP 的路径/方法保持一致
```

> 默认的数据库文件为工作目录下的 `tavily_proxy.db`；首次运行会自动建表并初始化密钥列表与请求日志表。

## CLI 选项

| Flag / Env                        | 说明                                                                                                     |
| --------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `--keys` / `TAVILY_API_KEYS`      | （可选）启动时同步到数据库的 Tavily API key，支持逗号分隔或多次传入；未提供时沿用数据库/管理界面的配置。 |
| `--upstream` / `TAVILY_UPSTREAM`  | 上游 Tavily MCP 端点，默认 `https://mcp.tavily.com/mcp`。                                                |
| `--bind` / `PROXY_BIND`           | 监听地址，默认 `127.0.0.1`。                                                                             |
| `--port` / `PROXY_PORT`           | 监听端口，默认 `8787`（开发期示例使用高位端口如 `58087`）。                                              |
| `--db-path` / `PROXY_DB_PATH`     | SQLite 文件路径，默认 `tavily_proxy.db`。                                                                |
| `--static-dir` / `WEB_STATIC_DIR` | Web 静态资源目录；若未显式指定且存在 `web/dist` 则会自动挂载。                                           |

## Web API

- `GET /api/summary`：返回整体成功/失败次数、活跃 key 数以及最近活跃时间。
- `GET /api/keys`：列出每个 key 的状态、调用次数与成功/失败统计（以 4 位 `id` 标识）。
- `GET /api/logs?limit=50`：按时间倒序返回最近的代理请求记录（默认 50 条）。
- `GET /api/keys/:id/secret`：管理员专用接口，返回指定短 ID 对应的真实 API key。
- `POST /api/keys`：管理员专用接口，新增或“反删除”一个 API key（JSON: `{ "api_key": "..." }`）。
- `DELETE /api/keys/:id`：管理员专用接口，软删除指定短 ID 的 API key（可通过重新添加同一密钥反向恢复）。
- `GET /health`：健康检查端点。

> 管理员身份通过 ForwardAuth 配置的请求头判断，只有管理员请求才能访问 `/api/keys/:id/secret`，前端页面也仅在管理员会话下展示“复制原始 API key”图标按钮。

### 关于 `TAVILY_API_KEYS` 同步语义

显式提供 `TAVILY_API_KEYS`（或 `--keys`）时，代理会把内容与数据库中的 `api_keys` 表保持同步；如果未提供，该步骤会被跳过，数据库里已有的密钥集合将保持不变，完全可以只依赖 Web 管理界面来新增/删除密钥。

同步语义如下：

- 在列表中的密钥：
  - 若数据库不存在则新增为 `active`；
  - 若之前被“软删除”（`deleted`）则恢复为 `active`；
  - 若状态为其它（如 `exhausted`），保持原状态不改动。
- 不在列表中的密钥：不会被硬删除，而是标记为 `deleted`（软删除）。

这样可以安全地通过环境变量维护基线集合，同时保留历史统计与日志；需要恢复时只需把密钥放回 `TAVILY_API_KEYS` 或在管理界面重新添加即可。

### 复制真实 API key 示例

```bash
# 获取 key 列表，注意客户端仅能看到 4 位短 ID
curl -s http://127.0.0.1:58087/api/keys | jq .

# 以管理员身份请求真实 key（需要满足 ForwardAuth 头部要求）
curl -s http://127.0.0.1:58087/api/keys/<id>/secret \
  -H "X-Forwarded-User: admin@example.com" \
  -H "X-Forwarded-Admin: true"

# 响应内容为 JSON，示例：{"api_key":"dummy-key-123"}
```

> Web 控制台中，“复制原始 API key” 的图标按钮同样位于可点击区域内，悬浮即可看到提示文字；无管理员权限的用户不会看到该图标。

> 只有 `/mcp` 与 `/mcp/*` 会被透传至 Tavily upstream，其余路径仍由本地服务处理或返回 404。

## 审计与密钥生命周期

- **请求日志**：`request_logs` 表记录 key、method/path/query、状态码、错误信息以及完整响应体，用于离线分析配额问题。日志使用 `api_key_id`（4 位短 ID）与 key 关联。
- **额度用尽自动标记**：遇到状态码 432 会把 key 标记为禁用，直到下一个 UTC 月初自动清除。
- **均衡调度**：每次请求都会挑选最久未使用的 key；若所有 key 都被禁用，则按最早禁用时间重试。

## 开发

- 需要 Rust 1.91+（2024 edition，`rust-toolchain.toml` 固定为 1.91.0）。
- 常用命令：
  - `cargo fmt`
  - `cargo check`
  - `cargo run -- --help`
- Web 前端位于 `web/`：
  - `cd web && npm install`
  - `npm run dev` 在本地调试（http://127.0.0.1:55173；已在 Vite 配置中固定高位端口并代理到后端）
  - `npm run build` 生成 `web/dist`，代理启动时可自动加载该 SPA
  - 已配置 Vite 代理：`/api`、`/mcp`、`/health` → `http://127.0.0.1:58087`

## Git Hooks

- 首次克隆后运行 `npm install` 安装 commitlint / dprint 依赖。
- 运行 `lefthook install` 安装预设的 Git hooks。
- 提交时会自动执行：
  - `cargo fmt` / `cargo clippy -- -D warnings`；
  - `npx dprint fmt` 用于格式化 Markdown 变更；
  - `npx commitlint --edit` 校验提交信息，需遵循 Conventional Commits 且使用英文。

如果缺少 `lefthook` 可通过 `brew install lefthook` 或参考官方安装指南。

希望这个代理能帮你更轻松地管理 Tavily API key 喵。
