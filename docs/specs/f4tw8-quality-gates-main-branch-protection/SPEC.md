# Quality Gates 与 `main` 默认分支保护收敛（#f4tw8）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 仓库已经有 `CI Pipeline`、`PR Label Gate`、`Docs Pages` 与 release workflow，但 GitHub 远端 `main` 在 `2026-07-18` 之前仍处于未保护状态：没有 branch protection，也没有 ruleset。
- 现状把“哪些检查真的构成 merge contract”分散在 workflow 与历史 spec 里，导致 reviewer 能看到很多 green checks，却无法从 repo-local source of truth 快速回答“哪些必须过、哪些只是辅助 leaf checks”。
- `Docs Pages` 之前依赖 workflow-level `paths:` 过滤决定是否触发；一旦把 docs / Storybook surface 升为 required check，这种触发方式会在纯后端 PR 上制造“缺少状态检查”的假阻塞。
- 该主题需要把 `main` 的 merge gate 写成长期可审计 contract，避免以后再退回“GitHub UI 手工点过、repo 里没人知道精确规则”的状态。

## 目标 / 非目标

### Goals

- 为仓库新增 repo-local `.github/quality-gates.json`，明确默认分支保护、signed commits、required checks 与 informational checks。
- 新增稳定命名的 `Quality Gates Contract` check，并保证 `Docs Pages Gate` 在所有 PR 上都稳定产出：相关改动 full run，无关改动 success/no-op。
- 将 GitHub `main` 保护对齐到 contract：全员 `PR-only`、strict required checks、signed commits、禁 force-push、禁 branch deletion、对 admins 生效。
- 在人类项目文档中写清楚“当前 merge contract 是什么、怎样做 owner-side drift audit”。

### Non-goals

- 不新增 `review-policy.yml`、required approving review count、`CODEOWNERS`、merge queue 或 actor-conditional bypass。
- 不改变 release label taxonomy、release workflow 语义、merge strategy 可用性，或把本主题扩展成整个发布系统改造。
- 不要求 GitHub Actions 在 CI 内直接读取管理员级 branch protection 状态；live audit 保持 owner-side/CLI 路径。

## 范围（Scope）

### In scope

- `.github/quality-gates.json`
  - repo-local required checks / informational checks / branch-protection contract。
- `.github/workflows/quality-gates.yml`
  - 稳定的 `Quality Gates Contract` check。
- `.github/workflows/docs-pages.yml`
  - always-trigger + `Docs Pages Gate` 聚合检查 + conditional no-op。
- `.github/scripts/**`
  - repo-local contract validator、Docs Pages 变更探测与 owner-side GitHub audit helper。
- `docs/specs/f4tw8-quality-gates-main-branch-protection/**`
  - 记录本主题 contract、实现状态与关键决策。
- `docs-site/docs/{zh,en}/development.md`
  - 将 CI / merge contract 与 owner-side audit 写入当前真相文档。

### Out of scope

- Rust / Web 产品逻辑、API、数据库 schema 或 release payload 本身的变更。
- 引入 ruleset 与 branch protection 并行维护双真相。
- 将现有 open PR 自动批量 rebase/refresh 到新 required checks；这些 PR 后续是否跟进更新，不在本主题自动处理。

## 需求（Requirements）

### MUST

- repo-local contract 必须声明 `main` 的 branch protection 目标状态，并把 required checks 锁定为：
  - `Quality Gates Contract`
  - `Release intent label gate`
  - `Worktree Bootstrap Smoke`
  - `Web Assets`
  - `Lint & Checks`
  - `Backend Shard Plan`
  - `Backend Tests`
  - `Frontend Checks`
  - `Compose Smoke (ForwardAuth + Caddy)`
  - `Build (Release)`
  - `Docs Pages Gate`
- `build-docs`、`build-storybook`、`assemble-pages` 必须继续保留为可见 leaf checks，但只作为 informational surface，不直接作为 branch protection required checks。
- `Docs Pages Gate` 必须在所有 PR 到 `main` 时稳定出现；当本次变更与 docs-site / Storybook / docs-pages assemble 无关时，该 gate 必须 success/no-op，而不是直接缺失。
- `Quality Gates Contract` 必须能在本地与 CI 内验证 `.github/quality-gates.json` 是否仍与当前 workflow inventory 一致。
- owner-side audit 必须能在不依赖私人 `STYLE_PLAYBOOK_HOME` 的前提下，对 live GitHub branch protection 与 required signatures 做 drift 检查。

### SHOULD

- contract validator 的 GitHub audit 路径应直接复用 `gh api`，让维护者可以在本机一条命令完成 repo-local + live state 双校验。
- 文档应明确指出 `Docs Pages Gate` 是 docs surface 的唯一 required check，而 `build-docs` / `build-storybook` / `assemble-pages` 只是 leaf evidence。
- spec / docs 应记录“切换到新 required checks 后，旧 open PR 在刷新 workflow 之前可能被阻塞”的预期影响。

### COULD

- 维护者可以使用 repo-local validator 生成 branch-protection payload，再用 `gh api` 同步远端，减少手写 JSON 漂移。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- repo-local contract：
  - `.github/quality-gates.json` 作为 source of truth，记录 branch protection 目标状态与 workflow-backed checks。
  - `python3 .github/scripts/check_quality_gates.py` 验证 contract schema、workflow inventory、workflow/job 名称与 required/informational mapping 是否一致。
- PR gate：
  - `Quality Gates Contract` workflow 在 `pull_request` / `push main` / `workflow_dispatch` 上运行，失败即说明 repo-local contract 漂移。
- `Docs Pages` workflow 在所有 `pull_request` / `push main` 上触发，但通过变更探测决定是否真的跑 docs-site / Storybook heavy jobs；根 `.bun-version` 变化也必须命中这条检测面。
  - `Docs Pages Gate` 总是出现；相关改动时要求 `build-docs`、`build-storybook`、`assemble-pages` 全部 success，无关改动时直接 success/no-op。
- owner-side live audit：
  - `python3 .github/scripts/check_quality_gates.py --github-live IvanLi-CN/tavily-hikari --github-branch main`
    必须同时检查 local contract、branch protection required checks / strict / admin enforcement / allow-force-push / allow-deletion，以及 required signatures。

### Edge cases / errors

- 若新增 required check 但未同步 `expected_pr_workflows`，`Quality Gates Contract` 必须失败，而不是允许 branch protection 与 workflow 名脱钩。
- 若 `Docs Pages` 的相关路径探测脚本、assemble 脚本或 workflow 改动后导致 docs leaf jobs 漂移，`Docs Pages Gate` 必须给出失败，而不是继续静默 no-op。
- 若 GitHub 远端仍返回 `404 Branch not protected`、required signatures 未开启、或 required contexts 与 contract 不一致，owner-side live audit 必须失败。
- 若新规则落地后历史 open PR 没有刷新到包含 `Quality Gates Contract` / `Docs Pages Gate` 的 workflow 版本，GitHub 会把这些 PR 视为缺少 required checks；这是预期迁移成本，而不是本 spec 允许绕过的例外。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                      | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner）                          | 使用方（Consumers）                  | 备注（Notes）                                                     |
| --------------------------------- | ------------ | ------------- | -------------- | ------------------------ | ---------------------------------------- | ------------------------------------ | ----------------------------------------------------------------- |
| repo-local quality gates contract | config/json  | internal      | New            | `./SPEC.md`              | `.github/quality-gates.json`             | Maintainers, CI                      | source of truth for required checks and branch-protection targets |
| Quality Gates Contract            | workflow/job | internal      | New            | `./SPEC.md`              | `.github/workflows/quality-gates.yml`    | GitHub Actions PR / push runs        | validates contract ↔ workflow inventory alignment                 |
| Docs Pages Gate                   | workflow/job | internal      | Modify         | `./SPEC.md`              | `.github/workflows/docs-pages.yml`       | GitHub Actions PR / push runs        | always-trigger required check with conditional no-op              |
| owner-side GitHub drift audit     | cli          | maintainer    | New            | `./SPEC.md`              | `.github/scripts/check_quality_gates.py` | Maintainers with `gh` authentication | validates live branch protection + required signatures            |

### 契约文档（按 Kind 拆分）

- `./SPEC.md`

## 验收标准（Acceptance Criteria）

- Given `.github/quality-gates.json` 已写入 repo
  When 运行 `python3 .github/scripts/check_quality_gates.py`
  Then schema、workflow inventory、required/informational checks 与 `expected_pr_workflows` 均通过校验。

- Given 任意纯后端 PR 到 `main`
  When `Docs Pages` workflow 运行
  Then `Docs Pages Gate` 必须出现并 success/no-op，且不会因为 leaf checks 缺失而卡住 merge。

- Given 任意涉及 `docs-site/**`、`web/**`、README 或 `docs-pages` 相关脚本/workflow 的 PR
  When `build-docs`、`build-storybook` 或 `assemble-pages` 任一失败
  Then `Docs Pages Gate` 必须失败，阻止该 PR 被视为 merge-ready。

- Given live GitHub `main` 已按本主题同步 branch protection
  When 运行 `python3 .github/scripts/check_quality_gates.py --github-live IvanLi-CN/tavily-hikari --github-branch main`
  Then branch protection 不得再是 `404`，required checks / strict / admins / force-push / deletion / required signatures 必须与 contract 一致。

## 验收清单（Acceptance checklist）

- [ ] repo-local `.github/quality-gates.json` 已存在并声明 required / informational checks。
- [ ] `Quality Gates Contract` workflow 已存在并在 PR/main 上运行。
- [ ] `Docs Pages Gate` 已取代 workflow-level `paths:` 缺失问题，成为稳定 required check。
- [ ] docs-site 中英开发文档已写明 merge contract 与 owner-side audit。
- [ ] GitHub `main` branch protection 已对齐 contract，且 live audit 可通过。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `python3 .github/scripts/check_quality_gates.py`
- `python3 .github/scripts/check_quality_gates.py --emit-branch-protection-payload > /tmp/tavily-hikari-branch-protection.json`
- `python3 .github/scripts/check_quality_gates.py --github-live IvanLi-CN/tavily-hikari --github-branch main`
- `cd docs-site && bun run build`

### UI / Storybook (if applicable)

- None

### Quality checks

- `bunx --bun prettier --check .github/quality-gates.json .github/scripts/check_quality_gates.py .github/scripts/detect_docs_pages_changes.py .github/workflows/docs-pages.yml .github/workflows/quality-gates.yml docs-site/docs/zh/development.md docs-site/docs/en/development.md docs/specs/README.md docs/specs/f4tw8-quality-gates-main-branch-protection/SPEC.md docs/specs/f4tw8-quality-gates-main-branch-protection/IMPLEMENTATION.md docs/specs/f4tw8-quality-gates-main-branch-protection/HISTORY.md`

## Visual Evidence

- None

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：一旦 `main` 开始要求 `Quality Gates Contract` 与 `Docs Pages Gate`，当前已有的 open PR 在刷新到新 workflow 版本前会被 GitHub 视为缺少 required checks；这是预期的迁移窗口。
- 风险：GitHub branch protection API 的 `require pull request before merging` 仍需要通过 `required_pull_request_reviews` 物化；若 GitHub API 未来调整该表达方式，repo-local validator 与同步命令需要一并更新。
- 假设：仓库继续使用 `main` 作为唯一默认分支，且本轮不引入 ruleset。
- 假设：docs-site / Storybook 仍由 `Docs Pages` workflow 统一构建与组装，不拆成新的 required workflow。

## 参考（References）

- `.github/quality-gates.json`
- `.github/workflows/ci.yml`
- `.github/workflows/docs-pages.yml`
- `.github/workflows/label-gate.yml`
- `.github/workflows/quality-gates.yml`
- `docs/specs/3grrf-ci-backend-test-split/SPEC.md`
