# Quality Gates 与 `main` 默认分支保护收敛实现记录（#f4tw8）

## 当前状态

- [x] 新增 repo-local `.github/quality-gates.json`，声明 required checks、informational checks 与 `main` 目标保护状态。
- [x] 新增 `Quality Gates Contract` workflow 与 repo-local validator。
- [x] 将 `Docs Pages` 改为 always-trigger，并新增稳定的 `Docs Pages Gate` 聚合检查。
- [x] 将中英开发文档同步为当前 merge contract 与 owner-side audit 入口。
- [x] 将 live GitHub `main` branch protection 对齐到 contract，并用 repo-local audit 验证。

## 实现要点

- `Quality Gates Contract` 只做 repo-local contract 校验，不在 CI 内直接请求管理员级 GitHub API。
- `check_quality_gates.py` 同时承担三类职责：
  - 校验 `.github/quality-gates.json` 的 schema；
  - 校验 required/informational checks 与 workflow inventory 的 mapping；
  - 为 owner-side 操作提供 branch-protection payload 生成与 live GitHub drift audit。
- `Docs Pages Gate` 作为 docs surface 的唯一 required check；`build-docs`、`build-storybook`、`assemble-pages` 保持可见 leaf checks，方便定位失败来源。
- docs heavy jobs 只在命中 `docs-site/**`、`web/**`、README、根 `.bun-version`、`docs-pages` workflow 或其组装脚本变更时执行；无关 PR 走 success/no-op。

## 待完成 / Follow-up

- 复核现有 open PR 对新 required checks 的迁移影响，并按需要 refresh 分支以产出 `Quality Gates Contract` / `Docs Pages Gate`。
