# Web：shadcn/ui 主通路收敛收尾（#grzw4)

## 状态

- Status: 已完成
- Created: 2026-03-09
- Last: 2026-03-11

## 背景 / 问题陈述

- `main` 已吸收此前大部分 shadcn/ui 页面迁移与共享 wrapper 落地，当前分支相对主干只剩少量收尾差异。
- 剩余尾差集中在 `web/.storybook/preview.tsx` 的 autodocs 保留点，以及历史 orphan wrapper `QuotaRangeInput` 的持续删除态确认。
- 本轮需要把规格收缩到当前真实剩余工作，避免继续描述已被 `main` 吸收的大规模页面迁移。

## 目标 / 非目标

### Goals

- 保持 `web/.storybook/preview.tsx` 作为 Storybook 全局 autodocs 配置入口，明确保留 `tags: ['autodocs']`。
- 固化 `web/src/admin/QuotaRangeInput.tsx` 与 `web/src/admin/QuotaRangeInput.stories.tsx` 已删除且无代码层引用的状态。
- 用前端构建结果验证本轮尾差收尾不引入行为回退。

### Non-goals

- 不重做 `PublicHome.tsx`、`TokenDetail.tsx`、`AdminDashboard.tsx`、`ApiKeysValidationDialog.tsx` 的 shadcn 页面迁移。
- 不新增新的 wrapper、stories 覆盖面或 UI 组件契约。
- 不修改 Rust 后端、业务逻辑、接口、路由或权限行为。

## 范围（Scope）

### In scope

- `docs/specs/grzw4-web-shadcn-mainline-convergence/SPEC.md`
- `web/.storybook/preview.tsx`
- `web/src/admin/QuotaRangeInput.tsx`
- `web/src/admin/QuotaRangeInput.stories.tsx`

### Out of scope

- `web/src/PublicHome.tsx`
- `web/src/pages/TokenDetail.tsx`
- `web/src/AdminDashboard.tsx`
- `web/src/components/ApiKeysValidationDialog.tsx`
- `src/**` Rust 后端代码

## 冻结约束（Frozen Constraints）

- `web/.storybook/preview.tsx` 现有 viewport、i18n 与 theme decorators 保持不变；本轮只允许补齐并保留全局 autodocs 标签。
- `QuotaRangeInput` 仅做持续删除态收尾：若文件已不存在，不得恢复、迁移或用新 wrapper 替代。
- 不扩展 Storybook story 数量，不修改页面级业务实现。

## 接口 / 组件契约

- `web/.storybook/preview.tsx` 的 `Preview` 默认导出必须包含 `tags: ['autodocs']`。
- `QuotaRangeInput` 不再作为生产组件或 Storybook story 暴露；代码层引用数必须为零。
- 本轮不新增或变更任何公共组件 props、业务接口与页面路由。

## 验收标准（Acceptance Criteria）

- Given `web/src/admin/QuotaRangeInput.tsx` 与 `web/src/admin/QuotaRangeInput.stories.tsx`
  When 检查仓库代码路径
  Then 两个文件均不存在
  And `web/src` 与 Storybook 入口不存在 `QuotaRangeInput` 生产引用或 story 引用。

- Given `web/.storybook/preview.tsx`
  When 检查 Storybook 全局配置
  Then `tags: ['autodocs']` 保持存在
  And 现有 viewport、语言切换与主题装饰器逻辑不回退。

- Given 前端构建校验
  When 执行 `cd web && bun run build` 与 `cd web && bun run build-storybook`
  Then 两条命令都成功通过。

## 非功能性验收 / 质量门槛

- `cd web && bun run build`
- `cd web && bun run build-storybook`
- `rg -n "QuotaRangeInput" web/.storybook web/src`

## 实现里程碑

- [x] M1: 收缩 spec 到当前真实尾差范围
- [x] M2: 保留 preview 全局 autodocs 配置并确认 orphan wrapper 持续删除态
- [x] M3: 完成 build / storybook / 引用检索验证
- [x] M4: PR / checks / review-loop 收敛

## 变更记录

- 2026-03-09: 创建规格，覆盖主通路 shadcn 收敛与共享 wrapper/page 迁移。
- 2026-03-10: PR #110 合并到 `main` 后，主页面迁移已被主干吸收。
- 2026-03-11: 将规格收缩为当前真实尾差收尾，仅保留 preview autodocs、`QuotaRangeInput` 持续删除态与构建验证。
- 2026-03-11: PR #116 创建后完成本轮 review-loop 收敛，规格状态与里程碑同步为收尾完成。
