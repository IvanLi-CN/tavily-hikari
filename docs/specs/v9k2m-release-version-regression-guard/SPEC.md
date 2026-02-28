# Release：版本回退防护与 `latest` 稳定修复（#v9k2m）

## 状态

- Status: 已完成
- Created: 2026-02-28
- Last: 2026-02-28

## 背景 / 问题陈述

- Release 流程在 `v0.3.3` 之后错误地产生并发布了 `v0.2.53`，出现稳定版本号回退。
- GHCR `latest` 会跟随最新 stable release，被 `v0.2.53` 覆盖，形成“latest 降级”。
- 根因是 semver tag 匹配正则写法错误，导致版本计算退回到 `Cargo.toml` 基线（`0.2.0`）再 bump。

## 目标 / 非目标

### Goals

- 修复版本计算逻辑，确保能正确识别已有 `vX.Y.Z` 标签。
- 在 release prepare 阶段新增 stable 单调递增保护，阻断 `new_version <= latest_stable`。
- 保持 rerun 幂等：若 HEAD 已有合法 channel tag，复用原 tag，不触发误拦截。

### Non-goals

- 不删除历史 `v0.2.53` 的 release 或 tag。
- 不修改运行时业务逻辑、API、数据库或前端协议。
- 不清理历史镜像 tag。

## 范围（Scope）

### In scope

- `.github/scripts/compute-version.sh`
- `.github/workflows/release.yml`
- `docs/specs/README.md`
- `docs/specs/v9k2m-release-version-regression-guard/SPEC.md`

### Out of scope

- 任何 Rust 业务代码与 Web 页面逻辑
- Release 资产策略与历史版本治理策略

## 验收标准（Acceptance Criteria）

- Given 仓库已有稳定版本 `v0.3.3`
  When 执行 `BUMP_LEVEL=patch bash .github/scripts/compute-version.sh --print-version`
  Then 输出必须为 `0.3.4`（或更高且严格大于现有最高 stable）。
- Given release channel 为 stable 且当前最高 stable 为 `0.3.3`
  When 计算结果为 `0.3.3` 或更低
  Then release prepare 必须 fail-fast，且不进入 Docker push 阶段。
- Given 同一提交重复 rerun release job
  When HEAD 已存在合法稳定 tag
  Then 流程复用 existing tag，不因单调保护而失败。

## 非功能性验收 / 质量门槛（Quality Gates）

- `bash -n .github/scripts/compute-version.sh`
- `BUMP_LEVEL=patch bash .github/scripts/compute-version.sh --print-version`
- `BUMP_LEVEL=minor bash .github/scripts/compute-version.sh --print-version`
- `BUMP_LEVEL=major bash .github/scripts/compute-version.sh --print-version`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 修复 semver tag 正则，恢复正确基线检测
- [x] M2: 增加 stable 单调递增 guard（仅新 tag 计算路径生效）
- [x] M3: 完成本地脚本验证与语法检查
- [x] M4: 快车道交付（PR + checks + review-loop + merge + release 验证）
