# Admin：API Keys 入库后自动关闭与已入库隐藏（#2fd7v）

## 状态

- Status: 已完成
- Created: 2026-02-26
- Last: 2026-02-26

## 背景 / 问题陈述

- 在校验对话框中点击“入库”后，即使所有 key 都已入库，列表仍然保留原条目，用户需要手动关闭弹窗。
- 已成功入库的条目继续显示，导致用户误以为尚未处理完成，并可能重复触发导入。

## 目标 / 非目标

### Goals

- 点击入库后，已入库 key 从对话框列表中隐藏。
- 导入按钮计数与可点击状态基于“剩余未入库且可导入项”计算。
- 当列表因隐藏而清空时自动关闭对话框。

### Non-goals

- 不修改后端 `/api/keys/batch` 协议与状态语义。
- 不引入新的测试框架。

## 范围（Scope）

### In scope

- `web/src/AdminDashboard.tsx`：
  - 维护 `imported_api_keys` 集合
  - 派生可见 rows 与剩余可导入 keys
  - 自动关闭判定（列表清空才关闭）
- `web/src/components/ApiKeysValidationDialog.stories.tsx`：
  - 补充导入后无剩余与有剩余的展示场景

### Out of scope

- `src/server.rs` 与后端 API 无改动。
- 无数据库 schema 变更。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                            | 类型（Kind）    | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                      |
| --------------------------------------- | --------------- | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ---------------------------------- |
| `KeysValidationState.imported_api_keys` | TypeScript type | internal      | Modify         | None                     | frontend        | AdminDashboard      | 记录已入库 key，用于隐藏与自动关闭 |
| `ApiKeysValidationDialog` 输入 state    | React props     | internal      | Modify         | None                     | frontend        | AdminDashboard      | 上层传入“已过滤可见 rows”          |

## 验收标准（Acceptance Criteria）

- Given 入库结果全部为 `created/undeleted/existed`
  When 点击“入库 N 个可用 key”
  Then 列表中的对应项被隐藏，且弹窗自动关闭。
- Given 入库后仍有失败/不可用项
  When 导入完成
  Then 已入库项被隐藏，弹窗保持打开，剩余项可继续重试。
- Given 输入中存在 `duplicate_in_input` 行
  When 同 `api_key` 导入成功
  Then 该 key 对应重复行一起隐藏。
- Given 弹窗未自动关闭（仍有剩余）
  When 用户使用“关闭”按钮/右上角关闭
  Then 关闭行为与原先一致，无回归。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Frontend build: `cd web && bun run build`

### UI / Storybook

- 更新 `ApiKeysValidationDialog` stories，覆盖导入后空列表与剩余列表场景。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 增加 `imported_api_keys` 状态并隐藏已入库行
- [x] M2: 导入按钮计数/可点击切换为“剩余可导入项”
- [x] M3: 列表清空自动关闭逻辑落地
- [x] M4: Storybook 场景补充
- [x] M5: 前端构建验证

## 风险 / 开放问题 / 假设

- 风险：自动关闭触发与刷新请求并发时，需要避免对话框状态抖动。
- 假设：后端继续返回 `created/undeleted/existed/failed` 状态集合，不新增兼容分支。

## 变更记录（Change log）

- 2026-02-26: 创建规格并随实现同步落地。
