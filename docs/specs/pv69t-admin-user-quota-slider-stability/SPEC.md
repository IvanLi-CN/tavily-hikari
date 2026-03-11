# Admin 用户配额滑块稳定化与指数档位收敛（#pv69t）

## 状态

- Status: 已完成
- Created: 2026-03-06
- Last: 2026-03-07

## 背景 / 问题陈述

- Admin 用户详情页的 4 个配额滑块当前会用实时草稿值推导滑块上限，导致拖动过程中 `max` 跟着变化。
- 当当前限额较小或为非整档值时，滑块轻微移动就可能触发大幅跳值，交互不可控。
- 右侧数字输入与滑块目前共享线性范围，无法兼顾“快速粗调”和“整数精调”。

## 目标 / 非目标

### Goals

- 将 4 个配额滑块的上限改为基于“服务端最近一次快照”稳定计算，而不是基于实时草稿动态扩容。
- 为滑块引入分段整数档位，拖动时吸附到合理整数档。
- 保留数字输入框的任意正整数精调能力，支持千分符展示/解析，且不改变保存接口与后端配额语义。
- 让 Admin 实页与 Storybook 共用同一套配额滑块算法，避免逻辑漂移。

### Non-goals

- 不修改 Rust 后端、数据库 schema、`/api/users/:id/quota` 协议与校验语义。
- 不新增 rail 上的文字刻度、打点或额外视觉组件。
- 不把这套逻辑抽象为站内通用 slider 组件。

## 范围（Scope）

### In scope

- `web/src/admin/quotaSlider.ts`：
  - 维护字段默认基线：`1000 / 1000 / 10000 / 100000`
  - 计算稳定上限 `max(defaultBaseline, fetchedInitialLimit, fetchedUsed, 1)`
  - 生成分段整数档位（`10-100` 每 `10` 一档，其后按 `120/150/200/250/300/400/500/600/800/1000 × 10^n` 扩展）、查找最近档位、完成 index/value 映射
  - 生成基于档位位置的轨道比例，确保 thumb 与颜色条对齐
- `web/src/AdminDashboard.tsx`：
  - 在用户详情加载与保存后刷新时固化每字段的服务端快照种子
  - 将 range 输入改为按档位 index 驱动
  - 保持数字输入可编辑任意正整数，超范围值不自动扩容
- `web/src/admin/AdminPages.stories.tsx`：
  - 复用共享 helper
  - 保留 `262`、`1022` 等非整档初值示例，覆盖首屏落位回归

### Out of scope

- 其他管理页与用户控制台页面无改动。
- 后端配额计算、环境变量默认值与 reset 逻辑无改动。

## 接口契约（Interfaces & Contracts）

### Public / external interfaces

- `PATCH /api/users/:id/quota` payload 保持不变：
  - `hourlyAnyLimit: number`
  - `hourlyLimit: number`
  - `dailyLimit: number`
  - `monthlyLimit: number`

### Internal interfaces

- 新增 `web/src/admin/quotaSlider.ts` 共享 helper，供 Admin 实页与 Storybook 调用：
  - 默认基线查找
  - 稳定上限解析
  - 档位集合生成（`10-100` 每 `10` 一档，其后使用整数档位原型 `100/120/150/200/250/300/400/500/600/800/1000` 按十进制扩展，并并入 `initialLimit` / `used`）
  - 草稿值到滑块 index/value 的映射
  - 基于档位位置的轨道比例裁剪

## 验收标准（Acceptance Criteria）

- Given 用户详情刚加载完成
  When 服务端返回某字段 `used=134`、`initialLimit=262`
  Then 该字段稳定上限固定为 `max(defaultBaseline, 262, 134, 1)`，且首屏显示值仍为 `262`。
- Given 用户正在拖动滑块
  When 草稿值发生变化
  Then 同一轮拖动期间稳定上限与档位集合不发生变化。
- Given 用户拖动滑块
  When 选择任一档位
  Then 写入值必须为档位集合中的整数，不得出现线性连续值。
- Given 稳定上限不小于 `100`
  When 渲染默认档位
  Then `10-100` 区间必须按 `10` 递增，之后再切换到整数“nice number”档位。
- Given 滑块展示任一档位值
  When 颜色条根据当前值渲染
  Then 颜色条末端必须与 thumb 使用同一套档位位置映射，不得出现明显脱节。
- Given 用户在输入框手填 `777` 或大于稳定上限的正整数
  When 尚未再次拖动滑块
  Then 输入值保持原样、允许保存；超过稳定上限时轨道/滑块做末端饱和，未超过时轨道按档位位置插值显示且不自动扩容。
- Given 输入框展示任意配额值
  When 用户输入 `25000`、`25,000` 或类似纯数字文本
  Then 界面必须格式化显示为带千分符的整数文本，并在保存与滑块计算时解析回相同整数；逗号/空白可被容忍，其他非法字符不得被误解析成不同数字。
- Given Storybook 的 `UserDetail` 页面
  When 展示 `262` 与 `1022` 等非整档初值
  Then thumb 能准确落位，不能在加载时被自动改写为附近整档。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Frontend build: `cd web && bun run build`

### UI / Storybook

- 更新 `UserDetail` story 并在 Storybook 中验证 4 个滑块的首屏与拖动表现。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增共享 quota slider helper，并固定 4 个字段默认基线
- [x] M2: Admin 用户详情页改为“稳定上限 + 档位 index”驱动
- [x] M3: 输入框保留任意正整数精调，超范围值不自动扩容
- [x] M4: Storybook 共用 helper，并补充非整档初值回归场景
- [x] M5: build + 浏览器验收完成

## 风险 / 开放问题 / 假设

- 风险：手填超范围值时，滑块 thumb 需要明确采用“末端饱和”而非隐式改值，避免视觉与保存值不一致的误解。
- 假设：服务端返回的用户详情字段继续完整提供 `used` 与 `limit`，无需额外 API 字段。

## Visual Evidence (PR)

- source_type: storybook_canvas
  story_id_or_title: admin-pages--user-detail
  state: default
  target_program: mock-only
  capture_scope: browser-viewport
  sensitive_exclusion: N/A
  submission_gate: approved
  evidence_note: 聚焦用户详情页的配额设置面板，确认每行采用“滑块优先占宽 + 固定宽数字输入列”的共享生产组件布局，避免输入框反向挤压滑块可用宽度。
  image:
  ![Admin 用户详情配额布局验收截图](./assets/admin-user-quota-layout-proof.png)

## 变更记录（Change log）

- 2026-03-06: 创建 follow-up spec，收敛稳定上限与指数档位滑块方案。
- 2026-03-06: 实现稳定上限 + 指数档位滑块；已完成 `bun run build`、`bun run build-storybook`，并在 Storybook 与真实 admin 页面完成交互验证。
- 2026-03-06: 创建 PR #99，进入快车道 checks / review-loop 收敛阶段。
- 2026-03-07: 修正配额滑块颜色条与 thumb 脱节问题，轨道改为按指数档位位置插值，确保 Storybook 与真实 admin 页面视觉对齐。
- 2026-03-07: 将前段档位收敛为整数梯度：`10-100` 每 `10` 一档，之后切换为整数 nice-number 档位，避免前段过细。
- 2026-03-07: 配额输入框改为千分符格式化展示与解析，并提升字号/字重以增强可读性。
- 2026-03-10: 补充用户详情配额区域验收截图到 spec 资产，固化“滑块优先占宽、输入框可容纳 `1,000,000`”的视觉证据。
