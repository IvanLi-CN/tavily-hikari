# Upstream/Base URL 路径前缀兼容增强（#5y2g4）

## 状态

- Status: 已完成（merge-ready）
- Created: 2026-04-04
- Last: 2026-04-04

## 背景 / 问题陈述

- 当前 `TAVILY_UPSTREAM` 与 `TAVILY_USAGE_BASE` 都按“裸 origin + 覆盖 path”解释，运行时会直接 `set_path(...)` 到 `/mcp`、`/search`、`/research`、`/usage` 等固定路径。
- 当上游地址本身携带 path prefix，尤其是 Resin 这类把 token / platform / target host 编进路径的 reverse-proxy URL 时，现有实现会把 prefix 整段覆盖掉，导致上游命中错误路径。
- 默认官方 Tavily 地址与本地 mock 地址都能工作，但“自建 reverse-proxy URL”这一类兼容性目前不足，用户只能退回内置 forward proxy 方案。

## 目标 / 非目标

### Goals

- 让 `TAVILY_UPSTREAM` 支持“完整 MCP 端点 URL”语义：配置值自带 path 时，`/mcp` 必须命中该 path 本身，而不是被重写成字面 `/mcp`。
- 让 `TAVILY_USAGE_BASE` 支持“带 path prefix 的 HTTP/usage 基地址”语义：`/search`、`/extract`、`/crawl`、`/map`、`/research`、`/research/{id}`、`/usage` 都必须在 prefix 后继续追加。
- 明确 trailing slash 归一化规则，避免双斜杠或 `/mcp/mcp` 这类重复路径。
- 保持无 path 配置零行为变化，不新增 env var 或 feature flag。

### Non-goals

- 不重新开放 `/mcp/*` 子路径透传。
- 不为 Resin 增加 `X-Resin-Account`、`platform.account`、动态 header 注入等专属 sticky 语义。
- 不改变现有 bearer / `api_key` 注入、quota sync、usage diff、request logs、token logs 与 key health 语义。

## 范围（Scope）

### In scope

- `src/lib.rs`
  - 新增共享 URL path 组合 helper，统一封装 path prefix 归一化。
- `src/tavily_proxy/mod.rs`
  - 替换 MCP、HTTP façade、research result、usage/quota probe 等调用点的直接 `set_path(...)`。
- `tests/server_http_contract.rs`
  - 新增 path-prefixed upstream/base URL 的外部契约测试夹具。
- `src/server/tests.rs`
  - 新增 research result、usage probe 等内部回归场景。
- `README.md`
- `README.zh-CN.md`
- `docs/tavily-http-api-proxy.md`
- `docs-site/docs/en/configuration-access.md`
- `docs-site/docs/zh/configuration-access.md`
- `docs/specs/README.md`

### Out of scope

- 任何新的 MCP ingress 形态或 `/mcp/*` 子路径支持。
- forward proxy / sticky routing 的产品语义扩展。
- 与本次 path 组合无关的 API、数据库或 UI 行为调整。

## 验收标准（Acceptance Criteria）

- Given `TAVILY_UPSTREAM=https://host/prefix/mcp` 或 `http://127.0.0.1:2260/token/Tavily/https/mcp.tavily.com/mcp`
  When 客户端请求 `/mcp`
  Then 上游实际命中的 path 必须是配置 URL 自带的端点 path，不能退化成 `/mcp` 或 `/mcp/mcp`。
- Given `TAVILY_USAGE_BASE=https://host/prefix/api.tavily.com`
  When 客户端调用 `/api/tavily/search`、`/api/tavily/extract`、`/api/tavily/crawl`、`/api/tavily/map`、`/api/tavily/research`、`/api/tavily/research/{id}` 或 `/api/tavily/usage`
  Then 上游 path 必须统一命中 `prefix` 下的对应相对路径。
- Given `request_id` 包含需要编码的字符
  When `/api/tavily/research/{request_id}` 转发到上游
  Then 编码行为必须保持现状，且 path prefix 不得丢失或出现 double-encoding。
- Given 默认官方 URL 或现有 mock upstream/base URL
  When 运行既有 contract / server tests
  Then 语义与结果必须保持不变。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --test server_http_contract`
- `cargo test server::tests::tavily_http_research_result`
- `cargo test server::tests::tavily_http_usage`
- 必要时全量 `cargo test`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 `TAVILY_UPSTREAM` / `TAVILY_USAGE_BASE` 的 path-prefix 兼容契约与文档口径
- [x] M2: 落共享 URL helper，并替换 MCP / HTTP façade / research result / usage probe 的 path 构造逻辑
- [x] M3: 补齐 prefixed path、trailing slash、encoded request id、无 path 不回归等测试
- [x] M4: 完成 README / docs-site / 设计文档同步、review-loop 收敛与 merge-ready 收口

## 风险 / 假设

- 风险：仓库外若存在依赖“覆盖式 path 行为”的非常规部署脚本，本次增强可能改变它们的上游命中方式。
- 假设：`TAVILY_UPSTREAM` 继续代表单一 MCP 根端点；若用户要接带 prefix 的 reverse-proxy，配置值会显式包含最终 MCP path。
- 假设：`TAVILY_USAGE_BASE` 的 path prefix 只负责路径拼接，不携带额外的 Resin 协议语义。

## 进展记录

- 2026-04-04: 新建 spec，锁定修复目标为“增强 path-prefix URL 兼容性”，不引入新的 env var 或 Resin 专属语义。
- 2026-04-04: 共享 URL path helper、MCP/HTTP/usage probe 路径拼接改造、契约测试、server 回归测试和文档同步已完成；待进入 PR review-loop 与 merge-ready 收口。
- 2026-04-04: PR #208 已创建并补齐 `type:patch` + `channel:stable` 标签；本地验证、PR checks 与 review-loop 已收敛到 merge-ready。

## 参考（References）

- `src/tavily_proxy/mod.rs`
- `tests/server_http_contract.rs`
- `src/server/tests.rs`
- Issue #183
