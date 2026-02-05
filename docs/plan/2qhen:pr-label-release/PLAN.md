# Plan: PR Label Release (GitHub Actions)

## Background

当前仓库在 `main` 上的发版（打 tag + GitHub Release + 推送镜像）是“push to main 就发版”的模式，
版本来源主要依赖 `Cargo.toml`（再做 patch 自增避免 tag 冲突）。这会导致：

- PR 合并后是否发版、以及 semver bump（major/minor/patch）不够确定（更多靠人工改版本文件）。
- `docs-only` / `chore` 之类的合并也会触发发版，噪声较高。

本计划引入“PR intent label 作为唯一事实来源”的发版方案：PR 的 `type:*` 标签决定是否发版与 bump 级别。

## Goals

- 引入互斥的 PR intent labels，并在 PR 上强制“必须且只能有一个 intent label”。
- `main` 通过 CI 后，根据合并 PR 的 intent label 决定：
  - `type:patch|minor|major`：发版（创建 semver tag + GitHub Release + 推送镜像）
  - `type:docs|skip`：不发版
- 版本号从 git tags 推导（fallback 到 `Cargo.toml`），并按 intent bump 生成下一版；同时保证 rerun 的幂等性（同一提交只会使用同一 tag）。
- 安全默认：若 commit 无法映射到“恰好一个 PR”，则保守跳过发版（不打 tag / 不推镜像）。

## Non-goals

- 不在本计划内修改 GitHub Branch Protection（需要 repo 管理权限与人工确认）。
- 不改动 Tavily 上游/生产访问策略（与本计划无关）。

## Scope

### In

- 新增：
  - `.github/workflows/label-gate.yml`：PR label 校验（fail-fast）。
  - `.github/workflows/release.yml`：workflow_run 在 main CI 成功后执行发版链路。
  - `.github/scripts/release-intent.sh`：通过 GitHub API 将 SHA 映射到 PR 并读取 labels。
- 更新：
  - `.github/scripts/compute-version.sh`：支持 `BUMP_LEVEL=major|minor|patch`，并从 tags 计算下一版本（保留 fallback 逻辑）。
  - `.github/workflows/ci.yml`：移除 push-to-main 自动发版逻辑（改由 `release.yml` 承担）。
- 文档：
  - 在 README 或 `docs/` 中说明 labels 集合与发版行为（简要）。

### Out

- 不在仓库内自动创建/同步 GitHub Labels（可选后续增强）。

## Acceptance Criteria

- PR 打开/同步/打标签时，`label-gate` 会强制：
  - 允许的 intent labels 只有：`type:docs|skip|patch|minor|major`
  - 只要出现未知 `type:*` label 或 0/2+ 个 intent labels，CI 失败。
  - 允许的 channel labels 只有：`channel:stable|channel:rc`
  - 只要出现未知 `channel:*` label 或 0/2+ 个 channel labels，CI 失败。
- 合并到 `main` 后：
  - 若该提交对应 PR 的 intent label 为 `type:patch|minor|major`，且 main CI 通过：
    - `channel:stable`：
      - 生成且只生成一个 semver tag（格式 `vX.Y.Z`）
      - 创建/更新 GitHub Release（非 prerelease）
      - 推送 ghcr 镜像：`latest` 与 `vX.Y.Z`
    - `channel:rc`：
      - 生成 prerelease tag（格式 `vX.Y.Z-rc.<sha7>`）
      - 发布 prerelease GitHub Release
      - 推送 ghcr 镜像：仅 `vX.Y.Z-rc.<sha7>`（不更新 `latest`）
  - 若 intent label 为 `type:docs|skip`：
    - 不创建 tag，不创建 Release，不推送镜像（只跑 CI）。
- 若 commit 无法解析到“恰好一个 PR”，则 `release.yml` 保守跳过发版并给出可读 reason。

## Testing / Verification

- 本地：
  - `bash -n` 校验新增/修改的 shell scripts。
  - 解析 YAML（确保语法正确）。
- GitHub：
  - PR 上验证 label-gate 行为（缺 label / 多 label / 正常 label）。
  - merge 后验证 release workflow 行为（skip 与 release 两条路径）。

## Milestones

- [x] PR label gate workflow + 允许的 labels 集合落地
- [x] release workflow（workflow_run）+ release-intent/compute-version 脚本落地
- [x] 文档补齐（labels 说明 + 发版行为）

## Risks / Open Questions

- Label 集合需要在 GitHub 仓库侧实际存在，否则只能靠人工添加；可后续补一个“手动同步 labels”的 workflow。
