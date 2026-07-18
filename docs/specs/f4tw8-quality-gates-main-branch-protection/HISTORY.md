# Quality Gates 与 `main` 默认分支保护收敛演进历史（#f4tw8）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-07-18：确认仓库当时已有 `CI Pipeline`、`PR Label Gate` 与 `Docs Pages`，但 GitHub `main` 同时不存在 branch protection 与 ruleset。
- 2026-07-18：锁定 `branch protection` 而非 `ruleset` 作为本轮执行面，原因是当前只保护单一默认分支，且 repo-local contract / audit helper 可以直接围绕 branch protection 建模。
- 2026-07-18：锁定 `main` 为全员 `PR-only`，required checks 启用 strict 模式，对 admins 生效，并同时要求 signed commits。
- 2026-07-18：确认 docs / Storybook surface 需要纳入 required checks，但不能继续依赖 workflow-level `paths:`；因此改为 always-trigger workflow + stable `Docs Pages Gate` + conditional no-op。
- 2026-07-18：确认 `build-docs`、`build-storybook`、`assemble-pages` 保留可见 leaf checks，但 branch protection 只要求 `Docs Pages Gate`，避免 required context 随 leaf 拆分而膨胀。
- 2026-07-18：使用 repo-local contract 生成的 payload 同步 live GitHub `main` branch protection，并立即通过 owner-side `gh api` audit 验证 required checks、strict、admin enforcement 与 required signatures 已对齐。

## Key Reasons / Replacements

- 本主题替代的是“GitHub UI 里临时点出来的默认分支设置 + 零散 spec 口径”，而不是替代现有 CI / release workflow 本身。
- `Quality Gates Contract` 被单独拆成新 workflow，而不是塞进 `CI Pipeline`，是为了给 branch protection 提供稳定、可单独审计的 repo-local contract check。
- `Docs Pages Gate` 的核心价值是稳定 required context，而不是减少 docs build 次数；no-op 只是防止无关 PR 因状态缺失被误阻塞。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
