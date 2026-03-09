# Web：shadcn/ui 主通路收敛整改（#grzw4)

## 状态

- Status: 进行中（PR #110 checks green）
- Created: 2026-03-09
- Last: 2026-03-09

## 背景 / 问题陈述

- 当前仓库虽然已有 shadcn/ui primitives，但 `PublicHome`、`TokenDetail`、`ApiKeysValidationDialog`、`AdminDashboard`、`AdminShell` 仍让 `.btn`、`.input`、`.modal`、`.table` 与散写原生控件承担生产页面主交互骨架。
- 这让视觉和可访问性入口分散，后续新增页面也容易继续扩散 DaisyUI 兼容类依赖。

## 目标 / 非目标

### Goals

- 把上述五个 in-scope 文件的主交互收敛到现有 shadcn/ui primitives 或受控 wrapper。
- 新增少量共享 wrapper，统一 token secret、表格、分页、quota range 与 admin nav 的样式入口。
- 保持现有业务逻辑、接口协议、页面路由、交互语义与键盘可用性不回退。

### Non-goals

- 不改后端 API、鉴权逻辑、数据库结构。
- 不做品牌视觉重设计，不重写业务文案。
- 不强行替换仓库里没有对应 primitive 的控件；slider 仅包到 wrapper，不要求替换成不存在的 shadcn primitive。

## 范围（Scope）

### In scope

- `web/src/PublicHome.tsx`
- `web/src/pages/TokenDetail.tsx`
- `web/src/components/ApiKeysValidationDialog.tsx`
- `web/src/AdminDashboard.tsx`
- `web/src/admin/AdminShell.tsx`
- 必需的共享 wrapper、最小 CSS bridge、受影响的 stories 与 `docs/specs/README.md`

### Out of scope

- `web/src/pages/AdminLogin.tsx`
- `src/**` Rust 后端代码
- Storybook 独立重构

## 冻结约束（Frozen Constraints）

- 继承并冻结既有 spec 约束：`rqbqk-shadcn-ui-rebuild`、`3rb68-public-home-token-access-modal`、`kgakn-admin-api-keys-validation-dialog`、`m4n7x-admin-path-routing-modular-dashboard`、`pv69t-admin-user-quota-slider-stability`。
- `web/src/index.css` 中仍有生产消费者的 compat selectors 本轮只保留、不删除。
- `ApiKeysValidationDialog` 移动端继续使用 `Drawer`。
- `Quota slider` 继续使用原生 `input[type="range"]`，但业务页面只能通过 wrapper 接入。

## 接口 / 组件契约

- 新增共享组件：`TokenSecretField`、`AdminTableShell`、`AdminTablePagination`、`QuotaRangeField`、`AdminNavButton`。
- `ApiKeysValidationDialog` 改为 controlled open 契约：由 `open` 驱动显示，保留现有 `onClose` / `onRetryFailed` / `onRetryOne` / `onImportValid` 行为。
- `PublicHome`、`TokenDetail`、`AdminDashboard` 不再依赖 `HTMLDialogElement.showModal/close` 作为主路径。

## 验收标准（Acceptance Criteria）

- Given `PublicHome` 的 token 输入与 token access 弹层
  When 页面渲染与用户交互
  Then 主输入、显隐、复制、确认按钮与弹层全部来自 `Input` / `Button` / `Dialog`
  And token 持久化、复制反馈、密码管理器规避属性与 LinuxDo 登录行为不变。

- Given `TokenDetail`
  When 用户切换 period、输入起始时间、翻页、展开日志、轮换 token
  Then `Select` / `Input` / `Button` / `Table` / `Dialog` 成为主通路
  And 筛选逻辑、分页逻辑、日志展开、SSE live 状态不回退。

- Given `ApiKeysValidationDialog`
  When 在桌面与移动端分别打开
  Then 桌面端使用 `Dialog + Table`，移动端继续 `Drawer`
  And 批量校验、失败重试、导入、关闭与自动关闭行为保持不变。

- Given `AdminDashboard` 与 `AdminShell`
  When 管理员使用各主模块
  Then 主按钮、主要输入、文本域、主要弹层、可见主表格与侧边导航都走 shadcn primitives / wrappers
  And 业务页不再直接依赖 `.range`、`.btn`、`.input`、`.modal`、`.table` 作为主交互骨架。

## 非功能性验收 / 质量门槛

- `cd web && bun install --frozen-lockfile`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
- 浏览器手工验证：`/`、Token Detail、desktop/mobile API keys validation、AdminShell、Admin 各模块表格/筛选/弹层
- 键盘验收：Tab focus、ESC、backdrop click、Drawer/Dialog 关闭、分页/select 操作与 clipboard 反馈不低于当前状态

## 实现里程碑

- [x] M1: convergence spec + shared wrappers + CSS bridge
- [x] M2: P0 页面（`PublicHome` / `TokenDetail` / `ApiKeysValidationDialog`）迁移
- [x] M3: P1 页面（`AdminShell` / `AdminDashboard`）迁移
- [x] M4: build / storybook / browser 验证
- [x] M5: PR / checks / review-loop / spec-sync 收敛

## 变更记录

- 2026-03-09: 创建规格，冻结 shadcn 主通路收敛范围、约束与验收口径。
- 2026-03-09: 完成 shared wrappers、P0/P1 页面迁移，并通过 `bun run build`、`bun run build-storybook` 与浏览器 smoke。
- 2026-03-09: PR #110 已创建；review-loop 无 findings；GitHub checks 全绿；spec drift check 通过。
- 2026-03-09: 根据 Storybook 验收反馈，恢复多个交互控件的 `border: none` 样式桥，修复 AdminShell active marker 缩成 32px 的回归。
- 2026-03-09: 为 `TokenSecretField`、`AdminTableShell`、`AdminTablePagination`、`QuotaRangeField`、`AdminNavButton` 补充 direct Storybook stories。
