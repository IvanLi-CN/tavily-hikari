# Tavily Hikari

Tavily Hikari 是一个异步 Rust 项目，为 Tavily API 提供多密钥轮询与健康状态持久化。它既可作为库集成，也附带命令行客户端，帮助你在多个密钥之间均衡负载，并将禁用策略留给外部流程决策。

## 特性
- 多密钥轮询调度：使用 SQLite 记录最近使用时间，确保流量均匀散布。
- 请求全量审计：每次请求的查询、选项、返回状态与响应体都会写入 SQLite，方便后续分析。
- 健康状态持久化：数据库保留 `disabled_at` 字段，可由外部策略或人工标记禁用。
- 月初自动恢复：UTC 每月第一天零点起自动清除早于该时刻的禁用标记，便于配额重置后恢复。
- 最早禁用回退：所有密钥都被禁用时，按最早禁用时间重新尝试并刷新时间戳。
- CLI/环境变量配置：支持从参数或 `TAVILY_*` 环境变量注入密钥、端点、数据库路径等选项。

## 快速开始
```bash
cd tavily-hikari

# 1. 在 .env 中维护密钥，或导出 Tavily API 密钥（逗号分隔或重复传参皆可）
echo 'TAVILY_API_KEYS=key_a,key_b,key_c' >> .env
# export TAVILY_API_KEYS="key_a,key_b,key_c"

# 2. 运行一次性查询（query 子命令）
cargo run -- query --query "What is the latest news about lithium battery recycling?"

# 或显式覆盖数据库文件位置
cargo run -- query --db-path /tmp/tavily_keys.db --query "Rust embedded trends"

# 3. 启动远程 MCP Server（serve 子命令）
cargo run -- serve --bind 127.0.0.1 --port 8080
# 现在可让支持 MCP 的客户端连接 http://127.0.0.1:8080/mcp
```

> 默认的数据库文件为工作目录下的 `tavily_keys.db`；CLI/MCP Server 首次运行会自动建表并初始化密钥列表与请求日志表。

## CLI 选项
| Flag / Env | 说明 |
| --- | --- |
| `-k, --keys` / `TAVILY_API_KEYS` | Tavily API Key，支持逗号分隔或多次传入，必填。|
| `-q, --query` | Tavily 查询内容，必填。|
| `--endpoint` / `TAVILY_ENDPOINT` | Tavily 兼容接口地址，默认 `https://api.tavily.com/search`。|
| `--db-path` / `TAVILY_DB_PATH` | SQLite 文件路径，默认 `tavily_keys.db`。|
| `--search-depth` | `basic`/`advanced` 深度。|
| `--max-results` | 返回结果数量上限。|
| `--include-answer` | 返回整理后的回答。|
| `--include-images` | 返回图片链接。|
| `--include-raw-content` | 返回原始网页内容。|
| `--include-domains` | 逗号分隔的白名单域名。|
| `--exclude-domains` | 逗号分隔的黑名单域名。|

所有布尔开关在 CLI 中默认关闭，显式添加对应 flag 即等价于传 `true`。

## 审计与密钥生命周期
- **请求日志**：`request_logs` 表记录 API key、查询、序列化后的搜索选项、HTTP 状态码、错误信息以及完整响应体，可用于离线分析禁用策略。
- **额度用尽自动标记**：当 Tavily 返回 432（额度用尽）时，会记录响应并更新 `disabled_at`，让该密钥在下个月前暂停使用。
- **每月复位**：UTC 月初会自动清空早于该时刻的禁用记录，帮助你在 Tavily 配额重置后恢复密钥。
- **最早禁用回退**：若所有密钥都处于禁用状态，则选择禁用时间最早的密钥重试，并刷新 `last_used_at`。
- **均衡调度**：活跃密钥按 `last_used_at` 轮询，确保分布均匀；调度成功会刷新 `last_used_at`。

## MCP Server 功能
- **端点**：`POST /mcp` 接收 JSON-RPC 2.0 消息，`GET /mcp` 暴露 SSE 流（keep-alive + 后续通知）。
- **会话管理**：`initialize` 会返回 `Mcp-Session-Id`，客户端需在后续请求头携带以维持会话。
- **工具暴露**：通过 `tools/list` 可发现 `tavily.search` 工具，`tools/call` 可执行 Tavily 查询；结果以文本 + `structuredContent` JSON 返回。
- **错误处理**：额度用尽、网络错误等会通过工具结果的 `isError=true` 提示，同时仍把完整响应写入数据库用于排查。

## 作为库使用
```rust,no_run
use tavily_hikari::{TavilyBalancer, SearchOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let balancer = TavilyBalancer::new(["key_a", "key_b"], "tavily_keys.db").await?;
    let response = balancer
        .search("status of reusable rockets", &SearchOptions::default())
        .await?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
```

## 开发
- 需要 Rust 1.84+（2024 edition）。
- 常用命令：
  - `cargo fmt`
  - `cargo check`
  - `cargo run -- --help`

欢迎根据自身需求扩展指标上报、自动重试或外部监控喵。
