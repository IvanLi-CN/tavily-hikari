# 用户控制台管理员入口（#2uv3g）

## 状态

- Status: 已完成
- Created: 2026-03-07
- Last: 2026-03-07

## 背景 / 问题陈述

- 现状：首页 `/` 已有管理员入口；LinuxDo 登录成功后默认落到 `/console`；管理员若要进入后台，仍需手工改地址到 `/admin`。
- 问题：Forward Auth 管理员账号落到用户控制台后，没有明确后台入口，导致“已是管理员但看起来像普通用户”的体验割裂。

## 目标 / 非目标

### Goals

- 在用户控制台 `/console` 的共享页头增加管理员专属入口。
- 入口只对 `GET /api/profile` 返回 `isAdmin=true` 的会话展示。
- 点击入口后直接进入 `/admin`，不改变现有 `/console` hash 路由和控制台主体交互。

### Non-goals

- 不改变 LinuxDo 登录成功后的默认落点（仍为 `/console`）。
- 不调整 Rust 鉴权、Forward Auth、`/api/profile` 契约或管理员权限口径。
- 不为非管理员新增后台入口提示或引导。

## 范围（Scope）

### In scope

- `web/src/UserConsole.tsx`：在共享页头 actions 区域增加管理员入口。
- `web/src/UserConsole.stories.tsx`：补充管理员显示态，覆盖入口可见性。
- `docs/specs/README.md`：登记该工作项并同步状态。

### Out of scope

- `/` 首页 Hero 的管理员入口逻辑。
- `/auth/linuxdo/callback`、`/console`、`/admin` 的服务端重定向逻辑。
- 新增后端 capability API 或角色模型。

## 需求（Requirements）

### MUST

- 管理员进入 `/console` 后，页头出现单一、明确的“打开管理员面板” CTA。
- 非管理员进入 `/console` 时，该 CTA 不显示。
- CTA 文案优先复用现有国际化键 `public.adminButton`。
- CTA 点击目标固定为 `/admin`。

### SHOULD

- CTA 与现有页头按钮视觉层级一致，不挤压主题/语言切换控件。
- Storybook 提供可直接验证管理员显示态的页面级 story。

## 接口契约（Interfaces & Contracts）

- None

## 验收标准（Acceptance Criteria）

- Given 管理员会话访问 `/console`
  When 用户进入 dashboard、tokens 或 token detail 任一视图
  Then 页头都能看到管理员入口，且点击后进入 `/admin`。

- Given 非管理员会话访问 `/console`
  When 页面加载完成
  Then 不展示管理员入口，其余控制台内容与现有行为保持一致。

- Given 现有 LinuxDo 登录链路
  When 用户登录成功或已登录用户访问 `/`
  Then 默认落点仍为 `/console`，不改为 `/admin`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Frontend build: `cd web && bun run build`

### UI / Storybook

- Update `web/src/UserConsole.stories.tsx` with an admin-visible console scenario.

### Quality checks

- 浏览器实测 `/console` 管理员态可见、非管理员态隐藏。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 用户控制台页头增加管理员专属入口
- [x] M2: Storybook 覆盖管理员可见态与默认隐藏态
- [x] M3: 本地构建与浏览器验证完成

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：页头空间在窄屏下较紧，需要保持按钮不破坏现有换行与控件布局。
- 假设：`/api/profile.isAdmin` 已足够代表“允许进入 `/admin` 的管理员态”，无需新增更细粒度 capability。

## 变更记录（Change log）

- 2026-03-07: 完成 `/console` 管理员入口、Storybook 管理员态覆盖与本地构建/浏览器验证。
