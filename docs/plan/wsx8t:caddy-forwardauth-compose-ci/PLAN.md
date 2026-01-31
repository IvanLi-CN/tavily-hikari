# Caddy 作为网关的 ForwardAuth + Docker Compose 示例与 CI 验证（#wsx8t）

## 状态

- Status: 已完成
- Created: 2026-01-30
- Last: 2026-01-30

## 背景 / 问题陈述

- 当前仓库已实现基于 ForwardAuth 请求头的管理员鉴权（`FORWARD_AUTH_HEADER` / `FORWARD_AUTH_ADMIN_VALUE` / `FORWARD_AUTH_NICKNAME_HEADER`），但缺少“带网关”的可运行部署示例。
- 仓库内现有 `docker-compose.yml` 仅提供最小化运行，不包含任何网关/ForwardAuth 层；容易出现“文档口径/配置样例/实际实现”漂移。
- 当前 CI（`.github/workflows/ci.yml`）只做 Rust 侧 lint + tests，不验证“网关 + ForwardAuth + Compose”这一运行形态。

## 目标 / 非目标

### Goals

- 提供一个“Caddy 作为网关”的 Docker Compose 示例，演示 ForwardAuth 注入身份头，并反向代理到 Hikari。
- 用户访问示例入口时需要“输入口令”完成鉴权（浏览器 Basic Auth challenge），未鉴权请求不得访问受保护资源；但 `/health` 公开可访问。
- 示例默认不触达 Tavily 生产上游（满足仓库安全约束）。
- 在 CI 中增加可重复的 smoke test：用 `docker compose` 启动示例并验证关键路径可用（至少包含鉴权边界）。

### Non-goals

- 不绑定特定 IdP/零信任方案（Authentik/Authelia/oauth2-proxy/Cloudflare Access 等），示例以可替换的 mock/auth service 作为演示。
- 不修改后端 ForwardAuth 判定逻辑与 HTTP API 契约（本计划关注部署示例与 CI 验证）。
- 不在本计划内引入 Kubernetes/Helm 等更复杂部署形态。

## 范围（Scope）

### In scope

- 新增示例目录（建议：`examples/forwardauth-caddy/`）：
  - `docker-compose.yml`：Caddy 网关 + Hikari +（可选）auth mock +（可选）upstream mock。
  - `Caddyfile`：`forward_auth` + `reverse_proxy`，并包含“入站头清理 + 从 auth 响应复制头”的安全示例。
  - `README.md`：如何启动、如何替换为真实 IdP、如何验证。
- CI 增加一个 compose-smoke job（或等价）：
  - 启动 compose。
  - 通过 `curl` 验证 health 与管理员接口的鉴权行为。
  - 明确禁止访问 Tavily 生产上游（通过配置与测试路径共同保证）。
- 文档更新：在 `README.md` / `README.zh-CN.md` 增加“Caddy + ForwardAuth 示例”入口与注意事项。

### Out of scope

- “把 Caddy 作为唯一鉴权源”（例如使用第三方插件实现 OAuth/OIDC）不在范围内；本计划聚焦 forward-auth pattern。
- 为生产部署提供完整的 TLS、证书自动化与多域名路由策略（可在后续计划补充）。

## 需求（Requirements）

### MUST

- 示例必须使用 Caddy 作为网关并可通过 Docker Compose 一键启动。
- 示例必须提供“需要输入口令”的鉴权能力（Basic Auth challenge），并用于保护 Hikari 的对外入口（除 `/health` 外的入口默认都需要鉴权）。
- 示例鉴权无需区分 admin/user 角色：单一口令即可访问受保护资源（示例口径下所有“已鉴权用户”都视为管理员）。
- 必须演示 ForwardAuth 的最小安全要点：
  - 网关在进入鉴权前清理潜在伪造的身份头（例如 `Remote-Email` / `Remote-Name`）。
  - 仅从鉴权服务响应中复制允许的头部到后续 `reverse_proxy` 请求（`copy_headers`）。
- 必须与后端现有 ForwardAuth 判定逻辑一致：
  - `FORWARD_AUTH_HEADER` 指定哪个请求头承载“用户标识”（例：`Remote-Email`）。
  - 当该头的值等于 `FORWARD_AUTH_ADMIN_VALUE` 时视为管理员。
- CI 必须执行该示例的 smoke test，且测试用例在默认配置下不触达 Tavily 生产上游。

## 接口契约（Interfaces & Contracts）

None（本计划不新增/修改/删除后端对外接口，仅新增部署示例与 CI 验证路径。）

## 验收标准（Acceptance Criteria）

- Given 使用示例的默认配置启动 `docker compose up -d`
  When 通过 Caddy 访问 `/health`
  Then 返回 `200` 且 body 为 `ok`
- Given 未提供口令（未鉴权）
  When 通过 Caddy 访问任一受保护资源（例如 `/` 或 `GET /api/debug/forward-auth`）
  Then 返回 `401` 且带 `WWW-Authenticate`（提示输入口令）
- Given 提供正确口令完成鉴权，且通过 ForwardAuth 注入的 `FORWARD_AUTH_HEADER` 值等于 `FORWARD_AUTH_ADMIN_VALUE`
  When 通过 Caddy 访问 `GET /api/debug/forward-auth`
  Then 返回 `200` 且响应中 `is_admin=true`
- Given CI 运行在 PR 与 main push 上
  When 执行 compose-smoke job
  Then job 在固定时间窗内通过，并且 smoke test 不依赖外网 Tavily 生产上游

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认 CI smoke test 仅验证“鉴权边界 + /health”，不覆盖会触达上游的真实代理路径（避免误触生产与提升稳定性）。
- 已确认示例中使用的身份头名（默认：`Remote-Email` / `Remote-Name`）与管理员值样例。
- 已确认 `/health` 公开访问，且示例鉴权不区分 admin/user（单一口令）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- CI: 新增 compose-smoke job（使用 `docker compose` + `curl`），验证鉴权与健康检查。
- 保持现有 `cargo fmt` / `clippy` / `cargo test` 通过。

### Quality checks

- 不新增新的 lint/test 工具链；复用 GitHub Actions 现有能力与系统自带 `docker compose`。

## 文档更新（Docs to Update）

- `README.md`: 增加 Caddy 示例入口与“不要触达 Tavily 生产上游”的提示。
- `README.zh-CN.md`: 同步增加上述入口与提示。

## 计划资产（Plan assets）

- None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [x] M1: 新增 `examples/forwardauth-caddy/`（compose + Caddyfile + README）并可本地启动验证
- [x] M2: CI 增加 compose-smoke job，覆盖 health + admin 鉴权边界
- [x] M3: 更新 `README.md` / `README.zh-CN.md` 链接到示例并明确安全约束

## 方案概述（Approach, high-level）

- 网关：Caddy 负责 `forward_auth` 与 `reverse_proxy`，并对 `/health` 做匿名放行；其余入口默认受保护。示例使用一个可替换的 `auth-mock` 服务来提供“口令鉴权”（对未鉴权返回 `401 + WWW-Authenticate`）并返回身份头，供 Caddy `copy_headers` 注入到上游请求。
- 后端：Hikari 仅通过 `FORWARD_AUTH_HEADER` 与 `FORWARD_AUTH_ADMIN_VALUE` 判断管理员；示例对齐该判定方式，避免出现两套不一致的“管理员口径”。
- CI：以 curl 验证鉴权边界为主，不覆盖真实代理请求（需要 upstream mock）以保持稳定与避免误触生产。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - 若示例未清理入站身份头，存在头部伪造风险（必须在示例中显式处理）。
  - 若 CI smoke test 覆盖代理路径但缺少稳定 upstream mock，可能引入不稳定与误触生产风险。
- 假设（需主人确认）：
  - 默认身份头使用 `Remote-Email`（管理员值为 `admin@example.com`），昵称头使用 `Remote-Name`。

## 变更记录（Change log）

- 2026-01-30: 创建计划（待设计）
- 2026-01-30: 冻结口径：`/health` 公开；鉴权不区分 admin/user；CI smoke 只测鉴权边界与 health
- 2026-01-30: 实现完成：示例目录 + CI compose smoke + README 入口
- 2026-01-31: 修复 CI smoke：改用 `/api/debug/is-admin` 断言，并在 CI 中构建 PR checkout 的 Docker image

## 参考（References）

- `.github/workflows/ci.yml`（现有 CI：Rust lint/tests；暂无 compose smoke）
- `README.md` / `README.zh-CN.md`（ForwardAuth 文档口径）
- `src/server.rs`（ForwardAuth 判定：`FORWARD_AUTH_HEADER` 的值 == `FORWARD_AUTH_ADMIN_VALUE`）
