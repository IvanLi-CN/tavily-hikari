# Web：共享 shadcn Storybook docs 收口与 CTA / wrapper 清理（#a9gkk)

## 状态

- Status: 已完成
- Created: 2026-03-10
- Last: 2026-03-10

## 背景 / 问题陈述

- `main` 已吸收大部分生产页面的 shadcn 收敛改动，当前剩余缺口主要集中在共享层 Storybook docs 与少量尾部 wrapper 清理。
- 目前 `web/.storybook/main.ts` 还未启用 `@storybook/addon-docs`，`web/src/components/ui/` 下多数组件仍缺少 direct story，`Select` 的 meta/docs 信息也不足以支撑 docs page 验收。
- `PublicHomeHeroCard` 仍保留少量原生 CTA 实现；与此同时，历史上的 `QuotaRangeInput` wrapper 已不在当前代码路径中，但需要把“持续删除态”作为本轮验收的一部分固定下来。

## 目标 / 非目标

### Goals

- 在 Storybook 10 中启用 `@storybook/addon-docs`，并让共享 shadcn primitives 拥有可读、可构建的 docs 页面。
- 为 `Button`、`Dialog`、`Drawer`、`Input`、`Table`、`Textarea` 补 direct stories，优化 `Select` docs 描述，并新增 `AdminShell` 独立 story 入口。
- 将 `PublicHomeHeroCard` 残留 CTA 统一收口到 `Button` / `Button asChild`，保持现有文案、跳转与点击语义不变。
- 固化 `QuotaRangeInput` orphan wrapper 已删除且无引用的状态。

### Non-goals

- 不重做 `PublicHome.tsx`、`TokenDetail.tsx`、`AdminDashboard.tsx` 的主页面迁移。
- 不改后端 API、路由、业务接口、数据库结构或鉴权逻辑。
- 不新增新的通用业务 wrapper，除非为已有共享组件补最小必要支持。

## 范围（Scope）

### In scope

- `web/.storybook/main.ts`
- `web/package.json` 与 `web/bun.lock`
- `web/src/components/ui/*.stories.tsx`（仅本轮列出的 primitives）
- `web/src/admin/AdminShell.stories.tsx`
- `web/src/components/PublicHomeHeroCard.tsx`
- `web/src/index.css` 中仅 hero CTA 相关样式桥
- `docs/specs/README.md`

### Out of scope

- `web/src/PublicHome.tsx`
- `web/src/pages/TokenDetail.tsx`
- `web/src/AdminDashboard.tsx`
- `web/src/components/ApiKeysValidationDialog.tsx`
- `src/**` Rust 后端代码

## 冻结约束（Frozen Constraints）

- Storybook docs 采用 Storybook 10 官方方式：安装并在 `main.ts` 中启用 `@storybook/addon-docs`，不在 `preview.tsx` 全局开启 autodocs。
- `AdminShell` docs 页面必须由独立 `AdminShell.stories.tsx` 暴露；旧的 `AdminLayout` story 入口不得继续保留。
- `PublicHomeHeroCardProps` 不新增、不删减，CTA 收口仅限渲染 primitive 与最小必要样式调整。
- `QuotaRangeInput.tsx` 与 `QuotaRangeInput.stories.tsx` 若当前不存在，则本轮只做持续删除态校验，不迁移到新位置。

## 接口 / 组件契约

- `UI/Button`、`UI/Dialog`、`UI/Drawer`、`UI/Input`、`UI/Select`、`UI/Table`、`UI/Textarea` 统一补 `tags: ['autodocs']` 与 `parameters.docs.description.component`。
- `UI/Select` 的 meta 必须显式声明 `component` 与 `subcomponents`，docs 文案说明 trigger/content/item 的组合关系。
- `Admin/AdminShell` 复用现有 shell fixture，单独承担 `AdminShell` 响应式壳层文档入口。
- `PublicHomeHeroCard` 的 LinuxDo、token access、admin action 三类 CTA 都通过 `Button` 体系渲染；链接分支使用 `Button asChild`。

## 验收标准（Acceptance Criteria）

- Given Storybook 构建共享 primitives
  When 执行 `cd web && bun run build-storybook`
  Then `Button`、`Dialog`、`Drawer`、`Input`、`Select`、`Table`、`Textarea`、`AdminShell` stories 都可成功构建
  And docs page 具备 component-level 描述。

- Given `PublicHomeHeroCard`
  When 首屏渲染 CTA 区域
  Then 不再直接保留原生 CTA 按钮实现
  And LinuxDo 登录、token access、admin action 仍保留既有跳转、aria-label 与点击行为。

- Given 仓库中的 quota range wrapper 状态
  When 搜索 `QuotaRangeInput`
  Then `web/src` 下无该组件与 story 的引用或残留文件。

## 非功能性验收 / 质量门槛

- `cd web && bun install --frozen-lockfile`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
- 浏览器验收：本地 Storybook 中打开并确认 `UI/Button`、`UI/Dialog`、`UI/Drawer`、`UI/Input`、`UI/Select`、`UI/Table`、`UI/Textarea`、`Admin/AdminShell` docs 页面可访问。

## 实现里程碑

- [x] M1: follow-up spec 与 README 索引落地
- [x] M2: Storybook docs addon 与共享 primitives stories 落地
- [x] M3: `PublicHomeHeroCard` CTA primitive 收口 + orphan wrapper 持续删除态校验
- [x] M4: build / storybook / browser 验证
- [ ] M5: PR / checks / review-loop / spec-sync 收敛

## 变更记录

- 2026-03-10: 创建规格，冻结本轮共享 Storybook docs 收口、Hero CTA 收口与 orphan wrapper 清理边界。
- 2026-03-10: 已完成 addon-docs 接入、shared stories、AdminShell docs、Hero CTA primitive 收口，并在 PR #113 上完成 checks / review-loop 收敛，进入合并收尾。
