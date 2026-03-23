# Request Logs 外键安全迁移与稳定版恢复发布（#oqb0p）

## 状态

- Status: 进行中（快车道）
- Created: 2026-03-23
- Last: 2026-03-23

## 背景 / 问题陈述

- `0.29.1` 在启动阶段执行 `request_logs` schema 迁移时，会通过 `request_logs_new` 全量复制后直接 `DROP TABLE request_logs` 再 rename 新表。
- 生产 SQLite 已存在 `auth_token_logs.request_log_id -> request_logs(id)` 与 `api_key_maintenance_records.request_log_id -> request_logs(id)` 引用，因此在 SQLite 外键开启时直接 drop 父表会触发 `FOREIGN KEY constraint failed`，容器反复重启，服务不可用。
- 101 已临时回退到 `0.28.15` 恢复服务，但 hotfix 仍需修复迁移本身并重新走 stable release，再把 101 升回新版本。

## 目标 / 非目标

### Goals

- 修复 `request_logs` 的启动迁移，使旧库在存在 request-log 子表外键引用时仍能安全升级。
- 保持 `request_logs.id` 稳定，确保所有 `request_log_id` 引用在迁移后仍可解析。
- 覆盖两条现有重建路径：
  - legacy `api_key` 列清理迁移
  - `api_key_id TEXT NOT NULL -> nullable` 迁移
- 补齐生产形态回归测试，确保 `0.29.1` 这类启动崩溃在本地可复现并被修复。
- 以 stable fast-flow 完成 PR、合并、release、101 升级与线上验收。

### Non-goals

- 不改变 HTTP API / MCP 对外行为。
- 不直接手改线上 SQLite 数据。
- 不额外引入一次性数据修复脚本。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `oqb0p-request-logs-fk-safe-migration` 索引。
- `src/store/mod.rs`
  - 抽出 request-log table-swap helper。
  - 在同一 SQLite 连接上执行外键安全的 `request_logs` 重建流程。
  - 在 table-swap 结束后强制跑 `foreign_key_check` 并阻断坏迁移。
- `src/server/tests.rs`
  - 新增生产形态迁移测试，覆盖 request-log 子表引用与两条 request-log 重建分支。
- 101 `ai` stack 部署资料
  - 待 stable release 成功后，把 `/home/ivan/srv/ai/docker-compose.yml` 与 `/home/ivan/srv/ai/tavily-hikari.md` 更新到新 digest。

### Out of scope

- 变更任何 request log / token log 的业务字段含义。
- 调整 release workflow 本身的发布策略。
- 扩大到其它机器或环境的部署改造。

## 验收标准（Acceptance Criteria）

- Given 旧库需要重建 `request_logs`
  When `auth_token_logs.request_log_id` 或 `api_key_maintenance_records.request_log_id` 已有引用
  Then 服务启动迁移成功，不再出现 `FOREIGN KEY constraint failed`。
- Given 迁移完成
  When 查询历史 `request_logs.id`
  Then 原有 `request_log_id` 引用仍全部有效，且 `foreign_key_check` 无返回行。
- Given 线上生产继续使用 immutable digest
  When stable release workflow 成功发布新 digest
  Then 101 更新到该 digest 后，容器健康、容器内 `/health`、外网 `/health` 与 `/api/version` 均返回新版本。
- Given 近期的 `/mcp/*` 本地拒绝 hotfix 已落地
  When 运行全量回归
  Then `/mcp/*` 本地拒绝、null-key request log 与对应 token-log 行为不回退。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- PR CI 全绿
- release workflow 成功产出 stable tag、GHCR digest 与 GitHub Release
- 101 升级后通过容器内/外双层健康验收

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: 以规格锁定 request-log 外键安全迁移策略与 101 升级收工条件
- [ ] M2: 抽出同连接 table-swap helper，完成外键安全的 `request_logs` 重建
- [ ] M3: 补齐生产形态迁移测试并通过本地质量门
- [ ] M4: 完成 PR、合并、stable release、101 部署与线上验收

## 风险 / 假设

- 风险：SQLite 的 `PRAGMA foreign_keys` 是连接级状态，若 helper 没有固定在单连接上执行，仍可能出现不可复现的迁移失败。
- 风险：若 table-swap 中途失败并留下 `request_logs_new` 残留，下次重试也可能继续卡住，因此 helper 必须显式清理残留临时表。
- 假设：生产部署继续遵循“release workflow 产出 immutable digest，再手动同步 101 compose/card”的策略，不切回浮动 tag。
- 假设：release channel 使用 stable，对应 PR 需要 `type:patch` 与 `channel:stable`。

## 参考（References）

- `src/store/mod.rs`
- `src/server/tests.rs`
- `/home/ivan/srv/ai/docker-compose.yml`
- `/home/ivan/srv/ai/tavily-hikari.md`
