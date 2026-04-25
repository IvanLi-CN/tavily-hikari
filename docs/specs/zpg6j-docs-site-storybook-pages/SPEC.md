# Public docs-site + Storybook Pages assembly (#zpg6j)

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-19
- Last: 2026-04-25

## 背景 / 问题陈述

- 当前仓库有 README、`docs/*.md` 与成熟的 Storybook 面，但缺少一个面向外部操作者的正式公开文档站。
- Storybook 现在能承担组件/页面验收，却没有和公开文档建立稳定的 Pages 发布关系，本地运行时也缺少 docs-site 与 Storybook 的双向回链。
- 参考 `octo-rill` 的交付面拆分后，本仓库也需要一个清晰的 `docs-site + Storybook + app` 三面结构。

## 目标 / 非目标

### Goals

- 新建独立 `docs-site/`，使用 Rspress + Bun 承载公开文档站，并按 English 默认 / Simplified Chinese 本地化双语发布。
- 把 Storybook 保持为独立构建产物，最终挂载到 GitHub Pages 的 `/storybook/`，并通过 `/storybook.html` 与 `/storybook-guide.html` 提供入口。
- 将现有 README 与选定部署/接入文档提炼为首发公开页族：`Home`、`Quick Start`、`Configuration & Access`、`HTTP API Guide`、`Deployment & Anonymity`、`Development`、`Storybook Guide`。
- 补齐 Storybook 对 docs-site 的本地/静态回链，保证公开文档与 UI 验收面能互相跳转。
- 新增 GitHub Actions docs-pages workflow，在 `main` 上自动发布 GitHub Pages。

### Non-goals

- 不直接公开 `docs/specs/**` 或 `docs/plan/**`。
- 不修改后端 API 语义、SQLite 结构或运行时鉴权逻辑。
- 不把 docs-site 打回 Rust 二进制静态资源。
- 不引入自定义域名、外部搜索 SaaS，或单独做 Storybook a11y 体系升级。

## 范围（Scope）

### In scope

- `docs-site/**`
- `docs/specs/zpg6j-docs-site-storybook-pages/SPEC.md`
- `docs/specs/README.md`
- `web/.storybook/preview.tsx`
- 若干关键 Storybook docs 入口的 `.stories.tsx`
- `README.md`、`README.zh-CN.md`、`web/README.md`
- `.github/workflows/docs-pages.yml`
- `.github/scripts/assemble-pages-site.sh`
- CI-only stabilization in backend tests when required to keep the Pages fix mergeable.

### Out of scope

- `src/**` Rust runtime 行为；允许不影响运行时语义的测试稳定性修复
- 现有业务路由与 API contract
- 任何 GitHub Pages 以外的发布目标

## 公开接口 / 交付契约

- 新增 docs-site env contract：
  - `DOCS_BASE`：GitHub Pages 子路径 base
  - `DOCS_PORT`：本地 docs-site 端口，默认 `56007`
  - `VITE_STORYBOOK_DEV_ORIGIN`：docs-site 本地跳转 Storybook dev server 时的 origin
  - `VITE_DOCS_SITE_ORIGIN`：Storybook 本地跳回 docs-site 时的 origin
- 新增公开静态路径：
  - `/`
  - `/quick-start`
  - `/configuration-access`
  - `/http-api-guide`
  - `/deployment-anonymity`
  - `/development`
  - `/storybook.html`
  - `/storybook-guide.html`
  - `/storybook/index.html`
- 新增中文镜像路径：
  - `/zh/`
  - `/zh/quick-start`
  - `/zh/configuration-access`
  - `/zh/http-api-guide`
  - `/zh/deployment-anonymity`
  - `/zh/development`
  - `/zh/storybook.html`
  - `/zh/storybook-guide.html`

Storybook redirect behavior:

- Published docs builds use `DOCS_BASE` to send `/storybook.html` and `/zh/storybook.html` to the shared Storybook artifact at `${DOCS_BASE}storybook/index.html`.
- Local standalone docs dev builds keep the existing localhost handoff to the Storybook dev server only when `DOCS_BASE=/`.

## 验收标准（Acceptance Criteria）

- `bun install --cwd docs-site --frozen-lockfile` 与 `bun --cwd docs-site run build` 通过。
- `DOCS_BASE=/${repo}/ bun --cwd docs-site run build` 通过，构建产物内无坏链。
- `cd web && bun run build-storybook` 通过，静态 Storybook 被组装到 `/storybook/` 后仍可正确加载资源。
- 组装后的 Pages artifact 至少包含：
  - `index.html`
  - `storybook/index.html`
  - `storybook.html`
  - `storybook-guide.html`
- 英文与中文文档都覆盖首发页族，且语言切换不会出现死链。
- 发布产物中不直接暴露 `docs/specs/**` 或 `docs/plan/**` 原始入口。
- Pages artifact smoke checks fail if the English Storybook redirect can escape the repo Pages base or if the Chinese redirect points at a locale-nested Storybook path.
- Repository CI remains green for the redirect fix PR, including backend tests that exercise local port reuse behavior.
- Storybook redirect target construction does not throw for opaque origins such as direct `file://` artifact inspection.

## 实现里程碑

- [x] M1: spec 与 README 索引冻结
- [x] M2: `docs-site/` 骨架、双语页面与 Storybook 跳转页落地
- [x] M3: Storybook docs-site 回链与精选 stories 文案收口
- [x] M4: docs-pages workflow + Pages 组装脚本落地
- [x] M5: 构建验证、PR、checks 与 review-loop 收敛到 merge-ready

## 风险 / 假设

- 假设 GitHub Pages 环境与权限可在该仓库直接启用。
- 风险：Rspress 双语导航与 Storybook 本地回链若配置不当，最容易在本地 dev 和 GitHub Pages 子路径之间表现不一致。
- 风险：Storybook docs 深链 ID 若后续重命名，需要同步更新 docs-site 导览页。

## 变更记录

- 2026-03-19: 创建 spec，冻结公开 docs-site + Storybook Pages 化方案与首发页族。
- 2026-03-19: 完成 Rspress 双语 docs-site、Storybook 双向回链、Pages 组装 workflow，并通过构建与浏览器验收。
- 2026-04-25: 修复 GitHub Pages 子路径下英文 Storybook 入口跳出 repo base 的问题，并补充 Pages artifact smoke 检查。
- 2026-04-25: 同步记录为保持 PR merge-ready 所需的 backend local-port 测试稳定性修复，运行时行为不变。
- 2026-04-26: 补齐 Storybook redirect target 在 opaque origin 下的兼容要求。
