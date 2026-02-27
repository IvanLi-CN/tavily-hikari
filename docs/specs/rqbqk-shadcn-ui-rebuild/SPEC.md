# Web：shadcn/ui 全量重构与双主题品牌化（#rqbqk）

## 状态

- Status: 已完成
- Created: 2026-02-27
- Last: 2026-02-27

## 背景 / 问题陈述

- 现有前端依赖 DaisyUI 组件语义类，样式体系与组件体系耦合较深，难以持续演进。
- 现有主题仅浅色主路径，缺少可持久化的主题模式切换与统一 token 约束。
- 品牌视觉与 Tavily 官网配色存在偏差，页面间视觉一致性不足。

## 目标 / 非目标

### Goals

- 迁移到 shadcn/ui 基础设施（Radix + CVA + Tailwind token）。
- 完成交互语义等价重构（Public/Admin/Login/Token Detail）。
- 落地 Light/Dark 双主题、`system` 默认跟随与本地持久化。
- 对齐 Tavily 品牌色并统一状态色（success/warning/error/info）。

### Non-goals

- 不改动后端 API、数据模型或鉴权协议。
- 不增加新的业务功能与流程变更。
- 不引入新的前端框架或状态管理库。

## 范围（Scope）

### In scope

- `web/package.json`、`web/tailwind.config.cjs`、`web/components.json`
- `web/src/index.css` 全局 token 与兼容组件层
- `web/src/theme.tsx`、`web/src/components/ThemeToggle.tsx`
- `web/src/components/ui/*` shadcn 基础组件
- `web/src/components/StatusBadge.tsx`、`web/src/components/LanguageSwitcher.tsx`
- `web/src/PublicHome.tsx`、`web/src/AdminDashboard.tsx`、`web/src/pages/AdminLogin.tsx`、`web/src/pages/TokenDetail.tsx`
- Storybook 装饰器与关联 stories

### Out of scope

- Rust 服务端实现与接口行为
- 文案体系重写（沿用现有 i18n key）

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）               | 类型（Kind）    | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                      |
| -------------------------- | --------------- | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ---------------------------------- |
| `ThemeMode`                | TypeScript type | internal      | New            | None                     | frontend        | all pages           | `light \| dark \| system`          |
| `ResolvedTheme`            | TypeScript type | internal      | New            | None                     | frontend        | all pages           | `light \| dark`                    |
| `tavily-hikari-theme-mode` | localStorage    | internal      | New            | None                     | frontend        | theme provider      | 主题模式持久化                     |
| `StatusBadge`              | React component | internal      | Modify         | None                     | frontend        | Public/Admin pages  | tone 语义保持不变，内部改为 shadcn |
| `LanguageSwitcher`         | React component | internal      | Modify         | None                     | frontend        | Public/Admin/Login  | 迁移至 Radix DropdownMenu          |

## 验收标准（Acceptance Criteria）

- Given 用户在任意页面切换主题
  When 选择 Light / Dark / System
  Then 页面立即生效并在刷新后保持模式。
- Given 现有 Token/API Key/Logs/Dialogs 交互
  When 完成重构后回归
  Then 行为语义与结果保持一致。
- Given 前端构建与 Storybook 构建
  When 执行构建命令
  Then 均可通过且不依赖 DaisyUI runtime/plugin。
- Given 多语言与现有路由
  When 页面访问 `/` `/login` `/admin` 与 hash 子路由
  Then 文案、导航、功能不回归。

## 非功能性验收 / 质量门槛（Quality Gates）

### Build & Storybook

- `cd web && bun run build`
- `cd web && bun run build-storybook`

### UI / A11y

- 375 / 768 / 1024 / 1440 断点无水平滚动。
- 键盘焦点可见，`prefers-reduced-motion` 下动画降级。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: shadcn 基础设施与主题 token 落地
- [x] M2: 共享组件（StatusBadge/LanguageSwitcher）迁移
- [x] M3: Public/Login 页面重构
- [x] M4: Admin/Token Detail 页面重构
- [x] M5: Storybook 与文档同步
- [x] M6: 构建验证与 review-loop 收敛
