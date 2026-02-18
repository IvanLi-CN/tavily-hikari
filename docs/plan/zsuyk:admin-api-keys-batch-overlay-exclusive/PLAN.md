# Admin / API Keys 批量录入浮层互斥显示（zsuyk）

## 背景

`/admin` 页面中，API Keys 面板头部提供「单行输入 + 添加按钮」用于新增一条 Tavily API key，并在 hover /
focus 时展开为悬浮卡片（overlay）支持多行批量粘贴。

当前实现中，overlay（展开态 ②）出现时，折叠控件组（折叠态 ①）仍同时可见，造成两个区域同屏暴露，违背
“② 是 ① 的展开态”这一交互约束。

## 目标

- 互斥显示：当 overlay（②）展开时，折叠控件组（①）在视觉上完全隐藏，且不可交互/不可聚焦。
- 焦点体验：当由 click / 键盘聚焦触发展开时，textarea（②）自动获得焦点，便于直接粘贴/输入。
- 保持现有语义：不改变 overlay 的定位/翻转/外点关闭/Esc 关闭/提交后收起等行为。

## 非目标

- 不调整 API Keys 面板其它布局与样式（除为隐藏 ① 的最小 CSS）。
- 不引入新的前端测试框架（当前仓库未配置 Vitest/RTL）。

## 范围（In / Out）

### In scope

- 前端：
  - `web/src/AdminDashboard.tsx`：API Keys 面板头部批量录入控件（①/②）互斥显示 + focus 行为。
  - `web/src/index.css`：新增最小 CSS 以在展开态隐藏 ①（不能使用 `display: none` 以避免锚点抖动）。

### Out of scope

- 后端接口、批量录入解析/提交逻辑（保持现状）。

## 验收标准（Given / When / Then）

### 互斥显示

- Given `/admin` 页面已加载，API Keys 面板处于折叠态（①可见）
  - When 鼠标移入新增控件组区域
  - Then 展开 overlay（②可见），且 ① 不可见（同屏不同时暴露）

### 可用性与焦点

- Given 折叠态（①可见）
  - When 点击 ① 或通过键盘 Tab 聚焦到 ①
  - Then 展开 overlay（②可见）并自动将焦点置于 textarea（②）

### 收起

- Given 展开态（②可见）
  - When 点击 overlay 外部或按 `Esc`
  - Then 立即收起 overlay（②不可见），并恢复 ① 可见

## 测试计划

- 手动回归：
  - `/admin` → API Keys：hover/click/tab 触发展开时 ①/② 互斥、focus 正常、Esc/外点收起正常。
- 自动化最小验证：
  - `cd web && npm run build`

## 实现里程碑

- [x] 修复 ①/② 互斥显示（展开态隐藏 ① 且不可交互/不可聚焦）
- [x] 完成焦点策略（仅 click/tab 聚焦触发展开时 focus 到 ②）
- [x] 完成最小验证（`web npm run build`）并记录测试结果到 PR

## 风险与备注

- 风险较低：仅影响 Admin 页面局部前端交互。
