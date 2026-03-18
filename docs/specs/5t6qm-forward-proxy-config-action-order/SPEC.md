# Forward Proxy 配置顶部动作顺序对调（#5t6qm）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-19
- Last: 2026-03-19

## 背景 / 问题陈述

- `/admin/proxy-settings` 配置区顶部目前按“添加节点 -> 手工节点数 -> 添加订阅 -> 订阅数”展示动作条。
- 当前顺序与主人希望的管理节奏不一致，使用时需要先越过手工节点入口才能找到更常用的订阅入口。
- 这次调整只涉及顶部动作条的信息顺序，不应连带改变弹窗逻辑、计数来源或下方两张配置卡片的布局。

## 目标 / 非目标

### Goals

- 将顶部动作条改为“添加订阅 -> 订阅数 -> 添加节点 -> 手工节点数”。
- 保持两个动作组内部的一一对应关系，避免按钮与计数错配。
- 补一条轻量回归断言，防止后续改版时把顺序无意改回去。

### Non-goals

- 不交换下方“订阅 URL / 手工代理 URL”卡片的左右位置。
- 不改动订阅/手工节点弹窗、保存流程、验证流程或任何 API 数据结构。
- 不为本次小改引入新的测试框架或额外 UI 组件。

## 范围（Scope）

### In scope

- `web/src/admin/ForwardProxySettingsModule.tsx`
  - 调整配置区顶部动作条 JSX 顺序。
- `web/src/admin/ForwardProxySettingsModule.render.test.ts`
  - 补充基于静态渲染输出的顺序断言。

### Out of scope

- `src/**` 后端逻辑、forward proxy 数据模型与接口契约。
- `web/src/admin/ForwardProxySettingsModule.stories.tsx` 之外的故事场景结构重排。

## 接口契约（Interfaces & Contracts）

- 无新增或变更的 HTTP API、Rust DTO、TypeScript 类型。
- UI 合同仅调整顶部动作条的视觉顺序，不改变按钮点击后的目标动作：
  - `添加订阅` 仍打开 subscription dialog。
  - `添加节点` 仍打开 manual dialog。

## 验收标准（Acceptance Criteria）

- Given 管理员打开 `/admin/proxy-settings`
  When 配置区顶部动作条渲染
  Then 顺序为 `添加订阅`、订阅数量、`添加节点`、手工节点数量。

- Given 页面已有订阅与手工节点数量
  When 顶部动作条显示数量文案
  Then 订阅按钮旁仍显示订阅数量，手工节点按钮旁仍显示手工节点数量。

- Given 管理员点击顶部动作条按钮
  When 点击 `添加订阅` 或 `添加节点`
  Then 仍分别打开原本对应的订阅弹窗与手工节点弹窗。

- Given 后续有人调整配置区布局
  When 运行 Bun 测试
  Then 静态渲染断言能发现动作条顺序回退。

## 测试与证据

- `cd web && bun test src/admin/ForwardProxySettingsModule.render.test.ts src/admin/ForwardProxySettingsModule.test.ts`
- `cd web && bun run build`
- Storybook `Admin/ForwardProxySettingsModule` 的 `Default` 场景用于人工复核顶部动作条顺序与按钮行为。

## 里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结顶部动作条顺序调整 spec
- [x] M2: 调整顶部动作条 JSX 顺序并保留原有动作绑定
- [x] M3: 补齐顺序回归断言、构建验证、Storybook 复核、PR/review-loop/merge 收口
