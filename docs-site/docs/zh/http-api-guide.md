# HTTP API 指南

## 这页解决什么问题

当你的客户端不会讲 MCP，而是直接调用 Tavily HTTP API 时，就看这一页。

典型场景包括：

- Cherry Studio 这类直接配置 Base URL + API key 的客户端
- 自己写的脚本、Webhook 或后端服务
- 想继续沿用 Tavily HTTP 请求格式，但不想把官方 Tavily key 暴露给终端

如果你要接的是 MCP 客户端，请改看 `/mcp`，而不是 `/api/tavily/*`。

## 当前可用的 Tavily facade 端点

| Method | Path                               | 说明                             | 认证         |
| ------ | ---------------------------------- | -------------------------------- | ------------ |
| `POST` | `/api/tavily/search`               | Tavily `/search` 代理入口        | Hikari token |
| `POST` | `/api/tavily/extract`              | Tavily `/extract` 代理入口       | Hikari token |
| `POST` | `/api/tavily/crawl`                | Tavily `/crawl` 代理入口         | Hikari token |
| `POST` | `/api/tavily/map`                  | Tavily `/map` 代理入口           | Hikari token |
| `POST` | `/api/tavily/research`             | Tavily `/research` 代理入口      | Hikari token |
| `GET`  | `/api/tavily/research/:request_id` | 获取 research 任务结果           | Hikari token |
| `GET`  | `/api/tavily/usage`                | 当前 token 的按日/月聚合使用情况 | Hikari token |

## 认证方式

推荐方式是：

```http
Authorization: Bearer th-<id>-<secret>
```

对于 `POST /api/tavily/*` 这类 JSON 请求，也兼容把 token 放进 body：

```json
{
  "api_key": "th-<id>-<secret>",
  "query": "rust async runtime"
}
```

补充约定：

- Header 和 body 都带 token 时，优先取 `Authorization: Bearer ...`
- `GET /api/tavily/research/:request_id` 与 `GET /api/tavily/usage` 这种 GET 端点，应使用
  `Authorization` 头，不用 body
- 开启 `DEV_OPEN_ADMIN=true` 的本地模式下，缺 token 也能走开发回退路径；生产环境不要依赖这个行为

## Search 请求示例

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Authorization: Bearer th-<id>-<secret>" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust async runtime",
    "topic": "general",
    "search_depth": "basic",
    "max_results": 3
  }'
```

如果客户端只能把 token 放进 body，也可以这样写：

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Content-Type: application/json" \
  -d '{
    "api_key": "th-<id>-<secret>",
    "query": "rust async runtime",
    "topic": "general"
  }'
```

## 其他 Tavily HTTP 端点

除 `/search` 外，当前还支持：

- `/api/tavily/extract`
- `/api/tavily/crawl`
- `/api/tavily/map`
- `/api/tavily/research`

这些端点都沿用同一套规则：

- 客户端请求体尽量保持 Tavily HTTP 的字段习惯
- Hikari 会把客户端的 `api_key` 去掉，再换成真实的上游 Tavily key
- 配额、审计、密钥池路由都发生在 Hikari 内部

Research 结果读取示例：

```bash
curl -H "Authorization: Bearer th-<id>-<secret>" \
  http://127.0.0.1:58087/api/tavily/research/<request_id>
```

## Cherry Studio 接入

1. 在用户控制台创建 Tavily Hikari access token。
2. 在 Cherry Studio 中选择 **Tavily (API key)**。
3. 将 API URL 设置为 `https://<your-host>/api/tavily`。
4. 将 `th-<id>-<secret>` 作为 API key 使用。

不要把 Tavily 官方 API key 直接填入 Cherry Studio；通过 Hikari 才能复用密钥池、额度和审计能力。

本地开发时，API URL 通常就是：

`http://127.0.0.1:58087/api/tavily`

## CLI + Agent Skills 接入

如果你想继续使用官方 Tavily CLI 工作流，但让流量通过 Hikari，可以安装 `tvly-hikari`：

```bash
curl -fsSL "https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh" | bash -s -- \
  --base-url "https://<your-host>" \
  --token "th-<id>-<secret>"
```

wrapper 会透传命令给官方 `tvly`，同时设置：

```bash
TAVILY_API_BASE_URL=https://<your-host>/api/tavily
TAVILY_API_KEY=th-<id>-<secret>
```

可选安装 Agent Skills：

```bash
npx skills add https://github.com/IvanLi-CN/tavily-hikari --global
```

这里的 `TAVILY_API_KEY` 在下游语义里故意使用 Hikari token。Hikari 会在额度与审计检查后，在
内部替换成真实上游 Tavily key。

## 常见错误与返回

- `401 Unauthorized`
  - 缺少 token
  - token 无效
  - token 已禁用
- `429 Too Many Requests`
  - 该 token 已达到小时请求数上限
  - 该 token 的额度不足以继续消费
- `400 Bad Request`
  - 请求体不是合法 JSON
  - 像 `max_results` 这样的字段明显非法
- `502 Bad Gateway`
  - Hikari 到 Tavily 上游失败
  - 当前没有可用的上游 key

成功时，Hikari 会尽量直接返回 Tavily 上游的响应体，而不是再包一层自定义 envelope。

## `/mcp` 和 `/api/tavily/*` 的区别

- `/mcp`
  - 给 Codex CLI、Cursor、Claude Desktop 这类 MCP 客户端使用
  - 入口是标准 MCP HTTP 传输
- `/api/tavily/*`
  - 给 Cherry Studio、脚本、自定义后端服务使用
  - 入口是 Tavily HTTP 风格的 JSON API

两者底层共用：

- Hikari access token
- Tavily key 池
- 配额判断
- 请求审计

## 相关接口

| Method | Path                  | 说明                     |
| ------ | --------------------- | ------------------------ |
| `GET`  | `/health`             | 存活探针                 |
| `GET`  | `/api/summary`        | 公共汇总指标             |
| `GET`  | `/api/user/token`     | 获取当前用户绑定的 token |
| `GET`  | `/api/user/dashboard` | 用户控制台汇总数据       |
| `POST` | `/api/user/logout`    | 用户登出                 |
| `GET`  | `/api/keys`           | 管理员查看上游 key 列表  |
| `POST` | `/api/keys`           | 管理员新增或恢复上游 key |

这些管理员接口走的是管理员鉴权，不是 Hikari token。

## 什么时候看 Storybook

如果你要验收控制台状态、表格空态、管理员流程或 dialog 交互，而不是对接客户端，
请直接打开 [Storybook](/zh/storybook.html)。

如果你对接时遇到 `401`、`429`、`502` 或 token 传递问题，再看 [FAQ 与排障](/zh/faq)。
