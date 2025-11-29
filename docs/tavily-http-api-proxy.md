# Tavily HTTP API 中转设计（`/api/tavily/*`）

本文档描述在 Tavily Hikari 中新增一组 HTTP API 端点，用于为任意 **Tavily HTTP 客户端**（包括 Cherry Studio）提供带密钥池与配额控制的中转能力。

重点是：**客户端将 Hikari 当作 Tavily HTTP 服务来调用**，只需要更换 Base URL 与“API 密钥”，即可复用现有的 Tavily 请求格式与返回结构。

> 本文档只涵盖设计，不包含具体实现细节。实现阶段可以以此为基线做微调。

---

## 1. 背景与目标

现状：

- Hikari 已通过 `TavilyProxy` 实现了对 Tavily MCP 上游的代理与密钥调度（`/mcp` 路径）。
- 用户态流量通过 **访问令牌（`th-<id>-<secret>`）** 进入 `/mcp`，在 `TavilyProxy` 内部被映射到一组 Tavily API Key，并记录完整的请求日志与配额使用。
- 后台还通过 `TAVILY_USAGE_BASE` → `/usage` 与 Tavily Usage API 对接，用于同步 Key 配额。

需求：

- 希望 Hikari 也能为 **Tavily HTTP API** 提供同样的能力，使得：
  - Cherry Studio 这类“直接调用 Tavily HTTP API”的客户端，只需修改 Base URL & API Key，即可走 Hikari 的 Key 池与配额系统；
  - 未来其它服务也可以将 Hikari 作为 Tavily HTTP 代理，从而统一密钥管理与审计。

目标：

1. 在 `/api` 下新增一组 **Tavily HTTP façade 端点**：`/api/tavily/search` 为第一阶段，其余端点预留设计。
2. 对外保持 Tavily HTTP API 的请求/响应结构尽量一致，减少客户端改动。
3. 对内复用现有 `TavilyProxy`、KeyStore、配额与日志体系。
4. 确保不会在日志中泄露客户端访问令牌或 Tavily 官方 API key。

---

## 2. 端点总览

统一前缀：`/api/tavily/*`

计划分阶段实现：

### 2.1 第一阶段（Cherry Studio 立即可用）

| Method | Path                 | 说明                                  | 认证         |
| ------ | -------------------- | ------------------------------------- | ------------ |
| POST   | `/api/tavily/search` | Tavily `/search` 的代理与负载均衡入口 | Hikari Token |

### 2.2 预留后续端点（不在首阶段实现）

这些端点先在文档中预留语义，具体实现按需求推进：

| Method | Path                  | 对应 Tavily 端点 | 说明                        |
| ------ | --------------------- | ---------------- | --------------------------- |
| POST   | `/api/tavily/extract` | `POST /extract`  | 内容提取                    |
| POST   | `/api/tavily/crawl`   | `POST /crawl`    | 深度爬取                    |
| POST   | `/api/tavily/map`     | `POST /map`      | 站点结构映射                |
| GET    | `/api/tavily/usage`   | （自定义）       | 按 **token** 聚合的用量视图 |

> 说明：`/api/tavily/usage` 更偏向 Hikari 自己的 Usage API，而不是 Tavily 原生的 `/usage` 代理；它将依赖本地 `token_usage_stats` 等表。

---

## 3. 认证与鉴权设计

### 3.1 Hikari 访问令牌

所有 `/api/tavily/*` 端点统一要求使用 Hikari 的访问令牌：

- 标准形式：`th-<id>-<secret>`。
- 鉴权逻辑复用现有 `validate_access_token` 与 `check_token_quota`。

支持两种传递方式（为兼容不同客户端）：

1. **推荐**：HTTP Header

   ```http
   Authorization: Bearer th-<id>-<secret>
   ```

2. **兼容**：请求体字段（为 Cherry Studio 等设计）

   ```json
   {
     "api_key": "th-<id>-<secret>",
     "...": "..."
   }
   ```

解析顺序：

1. 优先读取 `Authorization: Bearer`；
2. 若无，再尝试从 JSON body 的字段 `api_key` 中解析；
3. 两者都没有则返回 `401 Unauthorized`。

### 3.2 配额检查与错误返回

1. 从 token 提取 `token_id`（`th-<id>-<secret>` 中的 `<id>`）。
2. 通过 `proxy.check_token_quota(token_id)` 获取配额 verdict：
   - 若 `allowed == false`：
     - 调用 `record_token_attempt` 写入一条 `result_status = "quota_exhausted"` 的 token 日志；
     - 返回 `429 Too Many Requests`，body 为简短 JSON：
       ```json
       {
         "error": "quota_exhausted",
         "message": "daily / hourly limit reached for this token"
       }
       ```
3. 若通过配额校验，则进入 Tavily 上游调用流程（见后文）。

### 3.3 开发模式

与 `/mcp` 一致：

- 若 `DEV_OPEN_ADMIN = true`，则允许在缺失 token 的情况下走特殊流程（例如使用固定 token id `"dev"`）；
- 文档需要明确：**生产环境严禁依赖 `DEV_OPEN_ADMIN`，仅供本地调试**。

---

## 4. `/api/tavily/search` 详细设计

### 4.1 请求格式

**Method**：`POST`\
**Path**：`/api/tavily/search`

请求体 JSON 与 Tavily HTTP `/search` 尽量保持一致（字段示例）：

```json
{
  "api_key": "th-<id>-<secret>",
  "query": "latest news about Rust 2024 edition",
  "topic": "general",
  "search_depth": "basic",
  "include_answer": false,
  "include_images": false,
  "include_raw_content": false,
  "max_results": 5,
  "include_domains": ["example.com"],
  "exclude_domains": ["twitter.com"]
}
```

行为约定：

- 除 `api_key` 外，其余字段按 Tavily 官方文档含义原样透传上游；
- `max_results` 等数值字段不在 Hikari 侧做业务校验，仅在明显非法时（如负数）返回 400；
- 后续如果 Tavily 新增字段，Hikari 可以统一视为“透传字段”，不做强约束。

### 4.2 响应格式

直接透传 Tavily `/search` 响应（成功时）：

```json
{
  "query": "latest news about ...",
  "results": [
    {
      "url": "https://...",
      "title": "Some article",
      "content": "Relevant snippet ...",
      "raw_content": "...",
      "score": "0.98"
    }
  ],
  "answer": "…",
  "images": ["https://..."],
  "follow_up_questions": ["…"],
  "response_time": "0.87"
}
```

错误时：

- 若 Tavily 返回 4xx/5xx，则原样透传状态码与 body（不包额外 envelope），同时在内部记录 `result_status = "error" / "quota_exhausted"`；
- 若 Hikari 自身出现内部错误（如数据库不可用），返回 `502 Bad Gateway` 或 `500 Internal Server Error`，body 为简短 JSON：
  ```json
  { "error": "proxy_error", "message": "upstream unavailable" }
  ```

### 4.3 内部调用流程

处理 `/api/tavily/search` 的 handler 逻辑（高层伪代码）：

1. 解析请求：
   - 读取 headers / body，获得访问令牌 `th-...`，以及 token_id；
   - 解析 JSON body，获取 `options`。
2. 配额检查：
   - 调 `check_token_quota(token_id)`，不允许时按 3.2 逻辑返回 429。
3. 调用 Tavily HTTP：
   - 使用 `acquire_key_for(Some(token_id))` 从 Tavily Key 池选一把 key（`lease.secret`）；
   - 构造上游 URL：`{usage_base}/search`，其中 `usage_base` 使用 CLI 的 `--usage-base` / `TAVILY_USAGE_BASE`（默认 `https://api.tavily.com`）；
   - 构造上游请求 body：
     - 以原始 `options` 为基础；
     - 移除其中的 `api_key` 字段（避免将访问令牌当成 Tavily key 透传上游）；
     - 插入 `api_key: <lease.secret>`，或根据 Tavily 要求改为 `Authorization: Bearer <secret>`；
   - 发送 HTTP 请求，获得 `status` 与 `body_bytes`。
4. 结果分析与日志：
   - 使用类似 `analyze_attempt(status, body_bytes)` 的逻辑为 HTTP 调用计算：
     - `status`（`OUTCOME_SUCCESS` / `OUTCOME_ERROR` / `OUTCOME_QUOTA_EXHAUSTED`）；
     - `tavily_status_code`（从 JSON 中解析 `status` 或 `structuredContent.status`）。
   - 调 `log_attempt` 写入 request_logs：
     - `result_status` 同上；
     - `tavily_status_code` 为结构化状态码；
     - `request_body` / `response_body` 中需要对敏感字段脱敏（见 6.1）。
   - 若 `OUTCOME_QUOTA_EXHAUSTED`，调用 `mark_quota_exhausted(lease.secret)`；否则调用 `restore_active_status(lease.secret)`。
5. Token 维度日志：
   - 调用 `record_token_attempt(token_id, ...)`：
     - `http_status` 使用 `status.as_u16()`；
     - `mcp_status` 字段可复用为 “上游结构化状态码”，虽然此处不是 MCP，但复用同一列；
     - `result_status` 使用上一步计算结果（`success` / `error` / `quota_exhausted`）；
     - `error_message` 仅在 Hikari 自身出错或 Tavily 返回严重错误时写入。
6. 返回响应：
   - 若 Tavily HTTP 调用成功，原样返回 `status` 与 `body_bytes`；
   - 若 Hikari 自身失败，则返回 502/500，并确保也写入 token 日志（`result_status = "error"`）。

---

## 5. 其它 Tavily HTTP 端点设计草案

本节仅给出未来可实现的方向，具体实施时可根据需求裁剪。

### 5.1 `/api/tavily/extract`

- 语义：代理 Tavily `POST /extract`，用于从给定 URL 提取内容。
- 请求体：
  - 与 Tavily 官方文档一致（例如 `urls`, `include_images`, `max_pages` 等），额外接受 `api_key` 作为 Hikari token。
- 内部流程与 `/search` 类似：
  - 通过 token 进行配额检查；
  - 使用 `acquire_key_for` 选择 Tavily key；
  - 调用 `{usage_base}/extract`，将 Tavily key 写入 `api_key` 字段；
  - 记录 request_logs 与 token_logs。

### 5.2 `/api/tavily/crawl` 与 `/api/tavily/map`

- 语义：对应 Tavily `POST /crawl` 与 `POST /map`。
- 设计模式完全与 `extract` 相同，仅 path 与请求体字段不同。
- 需要注意的是，这类调用可能持续时间较长、流量较大：
  - 建议在配额检查策略上保守一些（例如在 `TokenQuotaVerdict` 中设定单次调用的权重）。

### 5.3 `/api/tavily/usage`（Hikari 自定义）

- 目标：给调用方提供按 **访问令牌** 维度的用量统计，而不是暴露底层 Tavily key 的 `/usage`。
- 数据源：
  - `token_usage_stats` 与 `request_logs` / `token_logs` 的聚合结果；
  - 可以从现有用于用户总览页的查询复用逻辑。
- 响应示例：

  ```json
  {
    "token_id": "abc123",
    "daily_success": 120,
    "daily_error": 3,
    "monthly_success": 840,
    "monthly_quota_exhausted": 2
  }
  ```

---

## 6. 安全与隐私考虑

### 6.1 敏感字段脱敏策略

对于 `/api/tavily/*` 请求，日志记录需遵守以下原则：

- 不在任何持久化日志中保存：
  - 客户端访问令牌 `th-<id>-<secret>`；
  - Tavily 官方 API key；
  - 任何名为 `api_key` 的字段的原始值；
  - Authorization 头中的 `Bearer` 值。
- 对 `request_logs.request_body` 与 `token_logs.error_message`：
  - 在写入前对 JSON 进行重写，将 `api_key` 字段替换为固定占位符（例如 `"***redacted***"`）；
  - 如果未来支持通过 Header 传递 Tavily key，也要在 `sanitize_headers` 中确保这些头不会原样落盘。

### 6.2 Header 策略复用

可复用现有 `sanitize_headers_inner` / `should_forward_header` 的逻辑：

- 上游请求只保留允许的 headers，并对 `Host` / `Content-Length` 等进行重算；
- 用户的 UA、Referer 等只按现有白名单转发，避免泄露代理内部信息。

### 6.3 错误信息限制

对外暴露的错误信息应尽量避免包含：

- 数据库路径、表名等内部实现细节；
- Tavily 返回的完整错误栈（可以略写为简短 message，详细内容仅写入内部日志）。

---

## 7. 与现有系统的关系与兼容性

- 不修改任何现有 `/api/*` 路由与 `/mcp` 行为，新增 `/api/tavily/*` 属于纯增量功能。
- `TavilyProxy` 需要新增一组针对 HTTP Tavily 的方法：
  - 可以实现为通用 `proxy_tavily_http(path, options, auth_token_id)`，再由各 handler 封装；
  - 或为 `proxy_http_search` / `proxy_http_extract` 等具体方法。
- 现有的用户总览页、Token 详情页与管理后台：
  - 依赖的指标（`result_status` 计数、token_usage_stats）可以直接复用；
  - `/api/tavily/*` 产生的请求应自然计入这些统计，无需额外 UI 变更。

---

## 8. 面向文档与接入指南的约定

在用户总览页等面向终端用户的文档中，可以使用统一的配置说明：

- Base URL：`https://<你的 Hikari 域名>/api/tavily`
- API 密钥：在 Hikari 控制台为当前用户生成的 `th-<id>-<secret>` 访问令牌
- Tavily 客户端侧保持原有 Tavily HTTP 请求格式（仅更换 baseURL 与 api_key 来源）

Cherry Studio 的接入指南可基于本设计，重点强调：

- “搜索服务商”选择 Tavily；
- 将 `API 地址` 改为 Hikari 的 `/api/tavily`；
- 将“API 密钥”替换为 Hikari 提供的访问令牌（而非 Tavily 官方 key）。

### 8.1 官方 JavaScript SDK（@tavily/core）接入示例

对于直接使用 Tavily 官方 JavaScript SDK（`@tavily/core`）的客户端，可以通过配置 `apiBaseURL` 与 `apiKey` 将流量导向 Hikari：

```ts
import { tavily } from "@tavily/core";

const client = tavily({
  // Hikari 控制台生成的访问令牌（th-<id>-<secret>）
  apiKey: process.env.HIKARI_TAVILY_TOKEN!,
  // 指向 Hikari 的 Tavily HTTP façade 前缀
  apiBaseURL: "https://<你的 Hikari 域名>/api/tavily",
});

const result = await client.search("hello from Hikari proxy", {
  searchDepth: "basic",
  maxResults: 3,
});
```

对 SDK 而言：

- `apiBaseURL` 应设置为 `https://<host>/api/tavily`（SDK 内部会在此基础上拼接 `/search`）；
- `apiKey` 使用的是 Hikari 访问令牌，而不是 Tavily 官方 key；
  - 请求体保持 Tavily `/search` 的字段习惯（`search_depth`、`include_raw_content` 等），Hikari 会自动：
    - 从请求中剥离 `api_key`（访问令牌）；
    - 为上游 Tavily 注入池内选中的 Tavily key；
    - 在日志中对所有 `api_key` 字段进行脱敏。

本仓库提供了一个基于 `@tavily/core` 的端到端烟囱测试脚本：

- 路径：`tests/e2e/tavily_http_node.cjs`
- npm 脚本：`npm run test:tavily-http`
- 运行前需确保：
  - Hikari 后端已启动并监听 `http://127.0.0.1:58087`（`scripts/start-backend-dev.sh`）；
  - `TAVILY_USAGE_BASE` 指向本地/Mock Tavily HTTP 上游；
  - 导出 Hikari 访问令牌，例如：

    ```bash
    export HIKARI_TAVILY_TOKEN="th-<id>-<secret>"
    npm run test:tavily-http
    ```

这样可以在不访问 Tavily 生产环境的前提下，验证通过官方 SDK → Hikari `/api/tavily/search` → Mock Tavily HTTP 的完整调用链路。

### 8.2 Cherry Studio 设置示意界面（前端 HTML Mock）

为帮助终端用户快速理解 Cherry Studio 中的配置路径，前端需要在「用户总览页」的 Cherry Studio 指南区域，使用纯 HTML/CSS（React + Tailwind + DaisyUI）构建一个简化版的设置页示意界面。

该示意界面的设计要求：

- 只作为视觉引导，不具备真实交互能力；所有输入框、按钮、开关、滑条均应标记为静态/禁用（例如 `disabled` 或 `pointer-events-none`），不会触发任何 API 调用。
- 布局结构尽量贴近 Cherry Studio 的真实设置页，保持**相对位置关系**正确：
  - 左侧为竖直导航栏，包含若干菜单项，其中「网络搜索」处于高亮选中状态；
  - 右侧顶部为「网络搜索」信息卡片，展示「网络搜索」「搜索服务商」以及右侧的 provider 下拉按钮；
  - 右侧中部为 Tavily 配置大卡片，包含 logo + 标题行、API 密钥区块、API 地址区块；
  - Tavily 配置卡片下方为「常规设置」区域（搜索包含日期、搜索结果个数滑条等），可以用简化控件表示。
- **无关内容**（例如侧边栏图标、部分菜单项、复杂图标按钮等）可以统一用灰色色块/占位条代替，不需要还原具体图标或文案。

关键信息必须在示意界面中清晰凸显：

1. 搜索服务商：
   - 在右上方 provider 卡片中，显示「搜索服务商：Tavily (API 密钥)」（英文界面可为 `Tavily (API key)`）。
2. API 密钥输入框：
   - 标题：`API 密钥`；

- 输入框内部 placeholder 建议使用 Hikari 令牌示例：`th-<id>-<secret>`（例如 `th-abcd-1234567890ab`）；
- 使用红色边框模拟「未填写密钥」的错误状态；
- 右侧可以放置「显示/隐藏」「检测」两个占位按钮（图标可用色块表示），但保持位置接近真实界面；
- 在输入框下方用一行小字明确说明：应填写 Hikari 颁发的访问令牌（`th-<id>-<secret>`），而不是 Tavily 官方 API key。

3. API 地址输入框：
   - 标题：`API 地址`；
   - 输入框中显示 Hikari HTTP façade 的地址：`https://<你的 Hikari 域名>/api/tavily`；
   - 本地开发示意可在邻近位置补充小字：例如 `http://127.0.0.1:58087/api/tavily`；
   - 输入框为只读/禁用，用于提示配置值而非真正编辑。
4. 常规设置区域：
   - 至少包含「搜索包含日期」+ 一个开启状态的开关控件（红色/高亮）；
   - 「搜索结果个数」+ 一条滑条和若干数字刻度（1 / 5 / 20 / 50 / 100），仅作视觉引导。

实现细节建议：

- 将该示意界面实现为独立的 React 组件（例如 `CherryStudioMock`），仅在 Public 用户首页的 Cherry Studio 指南 tab 下展示。
- 使用现有的 Tailwind + DaisyUI 工具类构建布局和样式；整体风格与用户总览页其它卡片保持一致（圆角、浅灰背景、轻微阴影等）。
- 所有文字文案（标题、字段名、说明）应通过 `web/src/i18n.tsx` 的 i18n 管线管理，保证中英双语切换时示意界面同步更新。

示意界面对应的 UI 参考截图应保存到仓库的 `docs/assets` 目录，例如：

- 文件：`docs/assets/screenshot-5OeEJG55.png`

本设计文档中直接引用该截图如下：

![Cherry Studio Web Search 设置界面示意](assets/screenshot-5OeEJG55.png)

在其他文档中需要引用时，可使用相同的相对路径：

```md
![Cherry Studio Web Search 设置界面示意](assets/screenshot-5OeEJG55.png)
```

这样既能在文档中展示接入路径，又避免直接复制 Cherry Studio 的完整 UI 实现。
