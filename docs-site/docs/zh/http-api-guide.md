# HTTP API 指南

## 公开探针

| Method | Path           | 说明         |
| ------ | -------------- | ------------ |
| `GET`  | `/health`      | 存活探针     |
| `GET`  | `/api/summary` | 公共汇总指标 |

## 管理员接口

| Method   | Path                   | 说明                 |
| -------- | ---------------------- | -------------------- |
| `GET`    | `/api/keys`            | 列出 key、状态、计数 |
| `GET`    | `/api/logs?page=1`     | 分页请求日志         |
| `POST`   | `/api/keys`            | 新增或恢复 key       |
| `DELETE` | `/api/keys/:id`        | 软删除 key           |
| `GET`    | `/api/keys/:id/secret` | 查看真实 Tavily key  |

## Hikari token 接口

| Method | Path                 | 说明                                               |
| ------ | -------------------- | -------------------------------------------------- |
| `POST` | `/api/tavily/search` | 给 Cherry Studio 等客户端使用的 Tavily HTTP facade |
| `GET`  | `/api/user/token`    | 获取当前用户绑定的 access token                    |
| `POST` | `/api/user/logout`   | 用户登出                                           |

## Cherry Studio 接入

1. 在用户控制台创建 Tavily Hikari access token。
2. 在 Cherry Studio 中选择 **Tavily (API key)**。
3. 将 API URL 设置为 `https://<your-host>/api/tavily`。
4. 将 `th-<id>-<secret>` 作为 API key 使用。

不要把 Tavily 官方 API key 直接填入 Cherry Studio；通过 Hikari 才能复用密钥池、额度和审计能力。

## HTTP facade 约定

- 面向客户端的字段尽量保持与 Tavily HTTP 习惯一致
- 认证永远基于 Hikari token，而不是上游 Tavily key
- 配额判断与路由发生在 Hikari 内部，再决定如何访问上游

## 什么时候看 Storybook

如果你要验收控制台状态、表格空态、管理员流程或 dialog 交互，而不是对接客户端，
请直接打开 [Storybook](/zh/storybook.html) 或 [Storybook 导览](/zh/storybook-guide.html)。
