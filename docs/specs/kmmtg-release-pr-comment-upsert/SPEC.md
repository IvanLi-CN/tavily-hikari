# Release：发布后回写 PR 评论（#kmmtg）

## 状态

- Status: 已完成
- Created: 2026-04-06
- Last: 2026-04-06

## 背景 / 问题陈述

- 当前 release workflow 能正确解析 `main` 提交对应的 PR，并成功发布 tag、GitHub Release 与 GHCR 镜像，但不会把发布结果回写到原 PR。
- `prepare` 阶段已经产出了 `pr_number` / `pr_url`，但这些信息只被写入 GitHub Actions step summary，没有后续消费。
- 现有 workflow 顶层权限仅为 `issues: read`，即便补脚本也无法对 PR issue thread 发评论。

## 目标 / 非目标

### Goals

- release workflow 成功发布后，自动在对应 PR 上创建或更新一条带 marker 的发布评论。
- 评论内容至少包含 release tag、release 链接、版本号、channel 与 GHCR tag 信息。
- workflow rerun 时必须幂等：若 bot marker 评论已存在，则更新而不是重复刷屏。
- 若评论步骤权限不足或出现临时 API 异常，保留 warning 并不阻断已完成的正式发布。

### Non-goals

- 不修改 release 版本计算、tag 生成、GitHub Release 正文或 GHCR manifest 行为。
- 不补发历史 PR 的发布评论。
- 不引入新的外部 Action 以外的复杂发布队列或快照机制。

## 范围（Scope）

### In scope

- `.github/workflows/release.yml`
- `README.md`
- `README.zh-CN.md`
- `docs/specs/README.md`
- `docs/specs/kmmtg-release-pr-comment-upsert/SPEC.md`

### Out of scope

- Rust/Web 业务代码
- 任何数据库、部署脚本或 101 rollout 流程

## 验收标准（Acceptance Criteria）

- Given 某次 stable release 成功结束且 `prepare` 已解析到唯一 PR
  When `github-release` job 收尾
  Then 对应 PR 必须存在一条带固定 marker 的 bot 评论，正文包含 `vX.Y.Z` release 链接、`stable` channel、版本号与 `latest` / `vX.Y.Z` GHCR tag。
- Given 某次 rc release 成功结束且 `prepare` 已解析到唯一 PR
  When `github-release` job 收尾
  Then 对应 PR 评论必须改写为 rc 版本信息，且 GHCR tag 只列出 `vX.Y.Z-rc.<sha7>`，不包含 `latest`。
- Given 同一提交重复 rerun release workflow
  When marker 评论已经存在
  Then workflow 必须更新该评论而不是创建第二条重复发布评论。
- Given PR 线程中已有同 marker 但并非 `github-actions[bot]` 所发的评论
  When release workflow 尝试回写
  Then workflow 只记录 warning 并跳过修改，避免覆盖人工内容。

## 非功能性验收 / 质量门槛（Quality Gates）

- `git diff --check`
- `bunx --bun prettier --check .github/workflows/release.yml README.md README.zh-CN.md docs/specs/README.md docs/specs/kmmtg-release-pr-comment-upsert/SPEC.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec，锁定“发布后 PR 评论”的行为契约与幂等要求
- [x] M2: 为 release workflow 补齐可写权限与 marker-based PR comment upsert
- [x] M3: README / README.zh-CN 同步 release 行为说明
- [x] M4: 完成本地格式/差异校验并准备普通流程收口

## 风险 / 假设

- 验证结果：GitHub `issues: write` 单独不足以让 `workflow_run` 的 release job 对 PR thread 回写评论；`github-release` job 还需要 `pull-requests: write`。
- 风险：如果仓库把 `GITHUB_TOKEN` 权限进一步收窄到 job 级别之外，评论步骤仍可能只产出 warning；但这不应回滚已发布的 release 资产。

## 进展记录

- 2026-04-06: 确认根因不是发布失败，而是 `release.yml` 从未实现 PR 评论步骤，且 workflow 权限只有 `issues: read`。
- 2026-04-06: 参考 `codex-vibe-monitor` 的 marker comment upsert 方案，为 `github-release` job 增加幂等 PR 发布评论逻辑，并同步 release 文档。
- 2026-04-06: 首次上线后通过真实 release 验证发现 `issues: write` 仍会对 PR thread 返回 403；补充 `pull-requests: write` 作为最终闭环修复。
