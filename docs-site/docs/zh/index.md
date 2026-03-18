# Tavily Hikari 文档

Tavily Hikari 是一个面向 Tavily 流量的 Rust + Axum 代理层。它负责多上游 key 轮转、SQLite
审计落盘、管理员与用户访问控制，并附带 React + Vite 运维控制台。

这个公开文档站只覆盖以下对外内容：

- 产品能力与本地接入
- 部署、鉴权与访问模式
- HTTP API 接入说明
- 开发与 Storybook 验收入口

它**不会**直接发布 `docs/specs/**` 或 `docs/plan/**` 里的内部执行规格。

## 文档地图

1. 需要快速跑起来时，先看[快速开始](/zh/quick-start)。
2. 需要理解 CLI、ForwardAuth、内置管理员登录、Linux DO OAuth 时，看[配置与访问](/zh/configuration-access)。
3. 需要给 Cherry Studio 等客户端接入时，看[HTTP API 指南](/zh/http-api-guide)。
4. 需要生产部署与高匿名说明时，看[部署与高匿名](/zh/deployment-anonymity)。
5. 需要核对页面状态和组件表现时，直接打开 [Storybook](/zh/storybook.html) 或 [Storybook 导览](/zh/storybook-guide.html)。

## 产品形态

- 后端：Rust 2024、Axum、SQLx、Tokio、Clap
- 数据层：SQLite（`api_keys`、`request_logs`、用户/会话状态）
- 前端：React 18、TanStack Router、Tailwind CSS、shadcn/ui、Vite 5
- 对外交付面：运行时 Web、Storybook、公开文档站

## 接下来该看哪里

- 想先跑实例：[快速开始](/zh/quick-start)
- 想看鉴权/访问模型：[配置与访问](/zh/configuration-access)
- 想对接客户端：[HTTP API 指南](/zh/http-api-guide)
- 想从文档跳到 UI 验收面：[Storybook 导览](/zh/storybook-guide.html)
