# 账户级配额迁移与登录后用户控制台（#45squ）

## 状态

- Status: 进行中（fast-track）
- Created: 2026-03-02
- Last: 2026-03-02

## 背景 / 问题陈述

- 当前配额判定与展示主要基于 access token 维度，无法沉淀到账户层。
- LinuxDo 登录后仍停留在公共首页，缺少“我的控制台”入口与账户视角指标。
- 用户端需要最小可用的控制台：账户仪表盘 + token 管理（当前每账户仅 1 个 token，只读）。

## 目标 / 非目标

### Goals

- 新增账户级配额模型（业务配额 hour/day/month + 任意请求 hourly-any）。
- 对历史绑定账户执行一次性回填：把 token 维度已用量/限额迁移到账户维度。
- 已绑定账户 token 请求改为账户级配额判定；未绑定 token 继续 token 级判定。
- 登录后统一进入 `/console`，并在访问 `/` 时对已登录用户自动跳转。
- 新增用户控制台页面：`#/dashboard` 与 `#/tokens` + `#/tokens/:id`。

### Non-goals

- 用户侧 token 轮换/禁用/删除（保持只读）。
- 多账户、多角色 RBAC。
- 未绑定 token 强制纳入账户级配额。

## 范围（Scope）

### In scope

- `src/lib.rs`
  - 账户级配额表、回填迁移、判定逻辑。
  - 用户控制台所需查询能力（dashboard/tokens/detail/secret/logs）。
- `src/server.rs`
  - 新增 `/api/user/dashboard`、`/api/user/tokens*`。
  - 登录后 `/console` 路由与重定向策略。
- `web/`
  - 新入口 `console.html` + `console-main.tsx`。
  - 用户控制台页面（仪表盘、token 管理、token 详情）。

### Out of scope

- 管理员 `/admin` 的功能改造。
- 生产配额参数配置中心化改造。

## 接口契约（Interfaces & Contracts）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 账户已绑定 token 且 token 维度已有配额数据
  When 服务启动并执行一次性回填
  Then 账户级配额已用量与 token 级历史口径一致（允许窗口边界误差 ±1 bucket）。

- Given 已绑定账户 token 达到账户小时/日/月上限
  When 调用 `/mcp` 或 `/api/tavily/*`
  Then 返回 429 且按账户级限额判定。

- Given 未绑定 token
  When 调用 `/mcp` 或 `/api/tavily/*`
  Then 继续走原 token 级判定。

- Given OAuth 登录成功
  When 回调结束
  Then 跳转 `/console`；且已登录用户访问 `/` 自动跳转 `/console`。

- Given 进入 `/console#/dashboard`
  Then 能看到账户维度用量与限额（hourly-any/hour/day/month）。

- Given 进入 `/console#/tokens`
  Then 能看到 token 列表列（token id、统计与限额、复制、详情入口）。

## 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run build`

## 里程碑

- [x] M1: Spec 与 contracts 建立
- [x] M2: 账户级配额存储与回填迁移落地
- [x] M3: 用户控制台 API 与路由落地
- [x] M4: 前端 `/console` 仪表盘 + token 管理落地
- [ ] M5: fast-track 交付（push + PR + checks + review-loop）

## 变更记录

- 2026-03-02: 初始化规格，冻结范围、接口与验收口径。
