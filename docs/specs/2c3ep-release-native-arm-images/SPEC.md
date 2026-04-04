# Release：原生 ARM 镜像发布与双架构 manifest（#2c3ep）

## 状态

- Status: 已完成
- Created: 2026-04-04
- Last: 2026-04-04

## 背景 / 问题陈述

- 当前 release workflow 只会把 GHCR 镜像发布为 `linux/amd64`，因为 `.github/workflows/release.yml` 将发布平台固定成了 `linux/amd64`。
- `Dockerfile` 已支持 `TARGETARCH=arm64` 的运行时依赖下载，但发布链路仍未产出 `linux/arm64` manifest，ARM 主机拉取 `latest` / `vX.Y.Z` 时会命中 `no matching manifest`。
- 现有 release job 虽然配置了 QEMU，但这只能支持跨架构构建；本次需要的是“原生 ARM runner 上构建 ARM 镜像”，避免继续依赖 x64 + QEMU 的发布路径。

## 目标 / 非目标

### Goals

- stable release 发布后，`latest` 与 `vX.Y.Z` 都要成为包含 `linux/amd64` + `linux/arm64` 的 multi-arch manifest。
- rc release 发布后，`vX.Y.Z-rc.<sha7>` 也要成为同样的双架构 manifest，但不更新 `latest`。
- `linux/arm64` 镜像必须运行在 GitHub-hosted `ubuntu-24.04-arm` 上原生构建与 smoke，不能退回 QEMU 跨架构发布。
- 任一架构 smoke 失败时，阻断最终 manifest 发布与 GitHub Release，避免对外产生半套正式 tag。
- 保持 release rerun 的版本/tag 幂等行为，不改动已有 semver 计算与 release intent 语义。

### Non-goals

- 不引入 self-hosted ARM runner 或新的远端基础设施。
- 不修改 `ci.yml` 的常规 PR build 策略。
- 不改动 `Dockerfile` 的业务行为、数据库、HTTP API 或前端代码。
- 不调整 GitHub Release 资产内容或历史镜像治理策略。

## 范围（Scope）

### In scope

- `.github/workflows/release.yml`
- `docs/specs/README.md`
- `docs/specs/2c3ep-release-native-arm-images/SPEC.md`

### Out of scope

- 任何 Rust 业务逻辑、Web 页面和运行时配置
- 部署到 101 或其他环境的后续 rollout
- 非 release 工作流的架构策略

## 验收标准（Acceptance Criteria）

- Given release channel 为 stable
  When release workflow 成功结束
  Then `ghcr.io/<repo>:latest` 与 `ghcr.io/<repo>:vX.Y.Z` 都必须解析出 `linux/amd64` 和 `linux/arm64` 两个 manifest 条目。
- Given release channel 为 rc
  When release workflow 成功结束
  Then `ghcr.io/<repo>:vX.Y.Z-rc.<sha7>` 必须解析出 `linux/amd64` 和 `linux/arm64` 两个 manifest 条目，且 `latest` 不发生变更。
- Given `linux/amd64` 或 `linux/arm64` 的 native smoke gate 失败
  When workflow 进入 manifest 汇总前
  Then 最终的 multi-arch tag 与 GitHub Release 都不得执行。
- Given release job 需要发布 `linux/arm64`
  When workflow 调度对应架构 build job
  Then 该 job 必须运行在 `ubuntu-24.04-arm`，且 release 发布路径中不得再依赖 `docker/setup-qemu-action`。
- Given 同一提交重复 rerun release workflow
  When HEAD 已存在对应 channel tag
  Then 仍复用原有 tag/version，不引入额外 tag 或版本回退。

## 非功能性验收 / 质量门槛（Quality Gates）

- `git diff --check`
- `bunx --bun prettier --check .github/workflows/release.yml docs/specs/README.md docs/specs/2c3ep-release-native-arm-images/SPEC.md`
- 通过 workflow 级 smoke 验证：两个 native build job 都能构建本地 smoke image 并跑通 MCP 账单验烟。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec，锁定 native ARM runner、manifest 聚合与 release 契约
- [x] M2: 将 release workflow 重构为 `amd64` / `arm64` 原生构建与独立 smoke
- [x] M3: 新增 manifest 聚合与最终 tag 校验，阻断半发布
- [x] M4: 完成本地验证、PR、checks 与 review-loop 收敛到 merge-ready

## 风险 / 假设

- 假设：仓库当前可用 GitHub-hosted `ubuntu-24.04-arm` runner；若配额或计划限制导致不可用，本次修复会被 runner 可用性阻断，而不是回退到 QEMU。
- 风险：multi-arch manifest 汇总依赖两个原生架构 job 都成功推送 digest；若 GHCR 或 artifact 交接异常，release 会 fail-fast。

## 进展记录

- 2026-04-04: 新建 spec，锁定 issue #182 的真实修复范围为“发布双架构镜像，并强制 ARM 走原生 runner 构建”。
- 2026-04-04: `release.yml` 已重构为 `docker-native` + `docker-manifest`，`arm64` 改由 `ubuntu-24.04-arm` 原生构建并在两架构 smoke 通过后汇总正式 tag。
- 2026-04-04: PR #207 已进入 merge-ready；CI checks 通过，`codex review --base origin/main` 无待修阻塞项。
