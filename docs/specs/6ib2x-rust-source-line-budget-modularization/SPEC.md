# Rust 源码行数预算与模块拆分（#6ib2x）

## 状态

- Status: 已完成
- Created: 2026-04-18
- Last: 2026-04-18

## 背景 / 问题陈述

- 当前仓库的 Rust 工程源码里存在多个超大文件，尤其是 `src/server/tests.rs`、`src/store/mod.rs`、`src/tests/mod.rs`、`src/tavily_proxy/mod.rs` 与 `src/forward_proxy.rs`，已经明显超出单文件易维护范围。
- 大文件让测试夹具、存储迁移、代理编排与 handler 逻辑边界模糊，后续修复更容易继续堆到同一入口，放大 review、冲突与回归风险。
- 本轮需要把超长 Rust 源码拆回清晰模块，并补一个可持续执行的预算门禁，避免类似文件再次无上限膨胀。

## 目标 / 非目标

### Goals

- 将当前超长 Rust 源码文件拆分到职责更清晰的子模块/分片文件。
- 保持现有运行时行为、测试名、公开导入路径与 server wiring 不回退。
- 新增自动化预算检查，确保 `src/**/*.rs` 与 `tests/**/*.rs` 后续不会再次突破约定上限。

### Non-goals

- 不在本轮修改 `web/**` 前端源码行数预算。
- 不新增产品功能、接口字段或数据库 schema 语义。
- 不借机改动已有业务行为、路由契约或测试覆盖目标。

## 范围（Scope）

### In scope

- `src/server/tests.rs` 与 `src/server/tests/**`
- `src/tests/mod.rs` 与 `src/tests/**`
- `src/store/mod.rs` 与 `src/store/*.rs`
- `src/tavily_proxy/mod.rs` 与 `src/tavily_proxy/*.rs`
- `src/forward_proxy.rs` 与 `src/forward_proxy/**`
- `src/server/proxy.rs` 与 `src/server/proxy/**`
- `src/server/handlers/admin_resources.rs` 与 `src/server/handlers/admin_resources/**`
- `tests/rust_source_line_budgets.rs`

### Out of scope

- `web/**`、Storybook、样式或视觉证据
- release / deploy / shared-testbox 行为
- 新的业务测试场景设计

## Solution Lookup

- Solution检索: 未命中
- Solution引用: none
- solution_disposition: none

## 验收标准（Acceptance Criteria）

- Given 仓库中的 Rust 工程源码
  When 统计 `src/**/*.rs` 与 `tests/**/*.rs` 的文件行数
  Then 每个文件都必须不超过 `3000` 行。

- Given 打开已知超长入口文件
  When 查看 `src/server/tests.rs`、`src/store/mod.rs`、`src/tests/mod.rs`、`src/tavily_proxy/mod.rs`、`src/forward_proxy.rs`
  Then 这些入口文件应降为薄门面或小型聚合文件，主要实现迁移到同目录拆分文件中。

- Given 现有 crate、bin 与 server 入口
  When 执行 `cargo fmt` 与 `cargo test --workspace --no-run`
  Then 必须编译通过，且新增拆分不应引入导入路径或模块边界错误。

- Given 后续再有人把 Rust 源码堆回超长文件
  When 执行 `cargo test --test rust_source_line_budgets`
  Then 预算测试必须失败，并指出超限文件路径与行数。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo test --workspace --no-run`
- `cargo test --test rust_source_line_budgets`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 盘点 Rust 超长文件并确定预算上限
- [x] M2: 将后端实现/测试入口拆分为薄门面 + 子模块文件
- [x] M3: 增加 Rust 源码行数预算测试并通过本地验证
- [x] M4: 快车道推进到 PR merge-ready

## 变更记录（Change log）

- 2026-04-18: 创建规格，冻结“只收口 Rust 工程源码行数预算，不扩到 `web/**`”的本轮边界。
- 2026-04-18: 将 `src/server/tests.rs`、`src/tests/mod.rs`、`src/store/mod.rs`、`src/tavily_proxy/mod.rs`、`src/forward_proxy.rs`、`src/server/proxy.rs` 与 `src/server/handlers/admin_resources.rs` 拆为薄入口 + 多个子文件。
- 2026-04-18: 新增 `tests/rust_source_line_budgets.rs`，对 `src/**/*.rs` 与 `tests/**/*.rs` 建立 3000 行预算门禁。
- 2026-04-18: 本地验证通过：`cargo fmt`、`cargo test --workspace --no-run`、`cargo test --test rust_source_line_budgets`。
- 2026-04-18: 在 review 收敛中修正 rebalance 本地 MCP 校验错误的 `failure_kind` 记录，保持 canonical failure kind 与 `fallback_reason` 同时可用，并补充对应回归测试。
