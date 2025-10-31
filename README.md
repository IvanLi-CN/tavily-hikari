# Tavily Hikari

Tavily Hikari 是一个轻量级的反向代理：它会把来自客户端的请求透传至官方 `https://mcp.tavily.com/mcp` 端点，同时对 Tavily API key 进行轮询、健康标记与旁路记录。

## 特性
- 多 key 轮询：SQLite 记录最近使用时间，确保 Tavily API key 均衡出站。
- 透传代理：对外保持与官方端点兼容的请求/响应，额外附加 `tavilyApiKey` 查询参数与 `Tavily-Api-Key` 请求头。
- 旁路审计：每次请求的 method/path/query、状态码、错误信息与响应体都会写入数据库，方便后续诊断配额情况。
- 健康标记：检测到状态码 432 时自动把对应 key 标记为“额度用尽”，UTC 月初再恢复。
- 简单部署：通过 CLI 指定监听地址、上游端点、数据库路径即可运行。

## 快速开始
```bash
cd tavily-hikari

# 1. 在 .env 中维护密钥，或导出 Tavily API 密钥（逗号分隔或重复传参皆可）
echo 'TAVILY_API_KEYS=key_a,key_b,key_c' >> .env
# export TAVILY_API_KEYS="key_a,key_b,key_c"

# 2. 启动反向代理
cargo run -- --bind 127.0.0.1 --port 8080
# 代理地址为 http://127.0.0.1:8080，与 Tavily MCP 的路径/方法保持一致
```

> 默认的数据库文件为工作目录下的 `tavily_proxy.db`；首次运行会自动建表并初始化密钥列表与请求日志表。

## CLI 选项
| Flag / Env | 说明 |
| --- | --- |
| `--keys` / `TAVILY_API_KEYS` | Tavily API key，支持逗号分隔或多次传入，必填。|
| `--upstream` / `TAVILY_UPSTREAM` | 上游 Tavily MCP 端点，默认 `https://mcp.tavily.com/mcp`。|
| `--bind` / `PROXY_BIND` | 监听地址，默认 `127.0.0.1`。|
| `--port` / `PROXY_PORT` | 监听端口，默认 `8787`。|
| `--db-path` / `PROXY_DB_PATH` | SQLite 文件路径，默认 `tavily_proxy.db`。|

## 审计与密钥生命周期
- **请求日志**：`request_logs` 表记录 key、method/path/query、状态码、错误信息以及完整响应体，用于离线分析配额问题。
- **额度用尽自动标记**：遇到状态码 432 会把 key 标记为禁用，直到下一个 UTC 月初自动清除。
- **均衡调度**：每次请求都会挑选最久未使用的 key；若所有 key 都被禁用，则按最早禁用时间重试。

## 开发
- 需要 Rust 1.84+（2024 edition）。
- 常用命令：
  - `cargo fmt`
  - `cargo check`
  - `cargo run -- --help`

希望这个代理能帮你更轻松地管理 Tavily API key 喵。
