# Web：响应式双设备与 Storybook 断点验收收敛（#bcpru）

## 状态

- Status: 已完成（6/6）
- Created: 2026-03-04
- Last: 2026-03-06

## 背景 / 问题陈述

- 前端当前存在多套断点阈值（含 430/560/640/752/760/768/860 等），页面行为在边界切换时难以预期。
- 主路径表格在窄宽度下可读性不足，部分场景仍依赖横向滚动。
- Storybook 尚未提供统一断点视口预设，断点回归成本较高。

## 目标 / 非目标

### Goals

- 统一断点体系：设备模式 `small<=767 / normal>=768`，主内容紧凑模式 `content<=920`，Admin 结构折叠阈值 `<=1100`。
- 主路径表格在 small/compact 模式可切换到列表视图，普通模式保留表格信息密度。
- Storybook 提供可一键切换的断点视口集合，并覆盖边界切换验收。
- 对外验收报告不展示缝位数值明细，仅输出边界切换测试结论。

### Non-goals

- 不变更后端 API、数据库与鉴权协议。
- 不新增业务功能。
- 不引入新的前端框架或视觉回归平台。

## 范围（Scope）

### In scope

- `web/src/lib/responsive.ts` 响应式常量、类型与 hooks。
- `web/src/index.css` 断点收敛与小屏样式统一。
- `web/src/UserConsole.tsx`、`web/src/PublicHome.tsx`、`web/src/pages/TokenDetail.tsx`。
- `web/src/admin/AdminShell.tsx`、`web/src/AdminDashboard.tsx`。
- Storybook 相关文件：`web/.storybook/preview.tsx` 与相关 `*.stories.tsx`。

### Out of scope

- Rust 服务端代码。
- 非主路径业务模块的新增功能。

## 接口契约（Interfaces & Contracts）

- 新增前端内部常量与类型：
  - `VIEWPORT_SMALL_MAX = 767`
  - `CONTENT_COMPACT_MAX = 920`
  - `ADMIN_SIDEBAR_STACK_MAX = 1100`
  - `ViewportMode = 'small' | 'normal'`
  - `ContentMode = 'compact' | 'normal'`
- 新增 hooks：
  - `useViewportMode()`
  - `useContentMode(ref)`
  - `useResponsiveModes(ref)`
- Storybook `preview` 增加视口预设 options（用于断点切换验收）。

## 验收标准（Acceptance Criteria）

- 设备模式与主内容模式判定一致且可复现。
- 主路径表格在 small/compact 模式显示为列表，关键操作可见、可点击。
- Admin 侧栏在折叠与双栏切换时布局稳定。
- Storybook 可直接切换断点视口，相关 stories 覆盖目标状态。
- 对外验收清单仅展示“边界切换测试通过/失败”，不展示缝位数值。

## 非功能性验收 / 质量门槛

- `cd web && bun run build`
- `cd web && bun run build-storybook`

## 验收截图（移动端，iPhone 14 portrait 390px）

- User Detail（主内容卡片布局与间距）

  ![User Detail - iPhone 14 portrait 390px](./assets/mobile/user-detail-iphone14-390.png)

- Dashboard（左侧汉堡导航抽屉）

  ![Dashboard Nav Drawer - iPhone 14 portrait 390px](./assets/mobile/dashboard-nav-drawer-iphone14-390.png)

- API Keys Validation Dialog（底部全屏抽屉；底部操作单行、Import 徽标计数）

  ![API Keys Validation Drawer - iPhone 14 portrait 390px](./assets/mobile/apikey-validation-drawer-iphone14-390.png)

## 实现里程碑（Milestones）

- [x] M1: 响应式契约与 hooks 落地
- [x] M2: CSS 断点收敛与基础样式统一
- [x] M3: 主路径页面 small/compact 列表化
- [x] M4: Storybook 断点视口预设 + stories 补充
- [x] M5: 构建验证与边界切换回归
- [x] M6: review-loop 收敛与 PR 交付
