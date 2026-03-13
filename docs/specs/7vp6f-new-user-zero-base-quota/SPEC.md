# 新用户基础额度归零，仅靠标签发放额度（#7vp6f）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-13
- Last: 2026-03-13

## 背景 / 问题陈述

- 当前账户级额度模型会在新用户首次创建或首次解析额度时，自动写入一份跟随 env 默认 tuple 的基础额度。
- 这让新用户即使没有任何标签，也天然具备可用额度，和当前“仅使用标签给新用户发额度”的运营口径冲突。
- 账户基线默认值与 token 默认额度仍共用同一套 helper；如果直接把现有 defaults 改成 0，会误伤未绑定 token 的现有语义，也会把历史 `inherits_defaults=1` 账户在重启时同步成 0。

## 目标 / 非目标

### Goals

- 让新创建或首次落库额度行的账户，基础额度默认变为 `0/0/0/0`。
- 保持“有效额度 = 账户基线 + 标签增量”的现有模型；新用户是否可用完全由标签决定。
- 保留未绑定 token 的默认额度、现有 LinuxDo 系统标签 seed / sync 逻辑，以及管理员手工编辑账户基线能力。
- 历史账户不做批量迁移，已有 `account_quota_limits` 行保持现状。

### Non-goals

- 修改线上历史账户数据。
- 调整未绑定 token 的 quota 逻辑或 env 变量语义。
- 修改 LinuxDo 系统标签的默认 delta 值或同步策略。
- 引入新的标签类型或新的账户注册入口。

## 范围（Scope）

### In scope

- `src/lib.rs` 中账户默认基线、账户额度 fallback、历史默认跟随同步语义。
- 用户侧 / 管理侧读取 `quotaBase`、`effectiveQuota` 时的新用户口径。
- Rust 回归测试、README 与规格文档同步。

### Out of scope

- 前端新增提示文案或运营引导。
- 服务器数据修复脚本、远端数据库操作。

## 接口契约（Interfaces & Contracts）

- [contracts/db.md](./contracts/db.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 新用户首次通过 LinuxDo 或其他 provider 创建本地账户
  When 该用户尚未绑定任何标签
  Then `quotaBase` 返回 `0/0/0/0`
  And `effectiveQuota` 返回 `0/0/0/0`。

- Given 新创建的 LinuxDo 用户在首登后被自动绑定 `linuxdo_l*` 标签
  When 读取用户详情或执行 quota 判定
  Then `effectiveQuota` 仅等于标签增量
  And 不再叠加旧的账户基础默认额度。

- Given 历史账户已经存在 `account_quota_limits` 行
  When 服务升级到本方案后的版本并重启
  Then 这些行保持原值
  And 历史 `inherits_defaults=1` 行仍继续跟随旧 token/env 默认 tuple 同步，而不是改成 0。

- Given 未绑定账户的 token
  When 读取 token detail 或执行 token 级 quota 判定
  Then 仍沿用现有 token 默认额度，不受本方案影响。

- Given 管理员调用 `PATCH /api/users/:id/quota`
  When 写入任意非零或零基线
  Then 账户基线按请求值落库
  And 后续 `effectiveQuota` 继续按“基线 + 标签增量”计算。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: 账户默认基线 helper 与 token 默认额度 helper 解耦
- [ ] M2: 新用户首次落库与缺失行 fallback 改为零基线
- [ ] M3: 历史 `inherits_defaults=1` 保持旧默认跟随语义
- [ ] M4: Rust 测试与 README / spec 同步完成

## 方案概述（Approach, high-level）

- 新增独立的“新账户默认基线” helper，固定返回 `0/0/0/0`，仅用于账户维度的首次落库与缺失行 fallback。
- 保留现有 token/env 默认额度 helper，继续服务未绑定 token、LinuxDo 系统标签默认 delta，以及历史默认跟随账户的启动期同步。
- 对历史 `account_quota_limits` 行不做迁移；仅通过“插入新行时写 0、读取缺失行时 fallback 0”改变未来账户行为。

## 风险 / 开放问题 / 假设

- 风险：若有依赖“新用户无标签也可直接使用”的现存测试或文档，需要同步修正，否则会出现验收口径冲突。
- 假设：当前产品口径接受“无标签新用户完全不可用”，不要求额外前端文案。

## 变更记录（Change log）

- 2026-03-13: 新建 follow-up spec，锁定“仅影响新用户、历史账户不迁移、未绑定 token 不受影响”的执行口径。
