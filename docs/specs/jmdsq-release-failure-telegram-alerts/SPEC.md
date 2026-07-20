# jmdsq · Release 失败 Telegram 告警接入

## Summary

- 为 `Release` 工作流补一个 repo-local notifier wrapper，统一复用共享 Telegram 告警 workflow。
- 为 `main` 上 `CI Pipeline` 的 post-merge 失败补 release-blocker 告警，只在对应 merge commit 解析出 releasable intent 时通知。
- 为 release 目标 SHA 增加显式日志标记，确保失败告警能定位真实 release head。
- 接入后通过 `workflow_dispatch` smoke test 验证 Telegram 通知链路。
- notifier 会在首次 attempt 命中已知瞬时 Docker / BuildKit / Docker Hub 拉取故障时，自动 `rerun failed jobs` 1 次并抑制首次 Telegram 告警；只有重跑后仍失败或不符合瞬时外部故障特征时才告警。

## Scope

- 新增 `.github/workflows/notify-release-failure.yml`，同时监听 `Release` 与 `CI Pipeline` 的 `workflow_run`。
- 新增 `.github/scripts/release_failure_notifier.py` 与对应本地 Python 单测。
- 更新 `.github/workflows/release.yml` 输出 `RELEASE_REQUESTED_SHA` / `RELEASE_TARGET_SHA` 标记。
- 保持现有 release 版本/标签语义不变，但允许对瞬时 Docker 外部故障做一次 failed-jobs 自愈，并对 `CI Pipeline` 失败复用同一套 release-intent 解析规则。

## Acceptance

- `workflow_run` 在 `Release` 失败时触发 repo-local triage；若不是首次瞬时 Docker 外部故障，自然进入 Telegram 告警。
- `workflow_run` 在 `main` 的 `CI Pipeline` 失败时，也必须触发 repo-local triage；若 merge commit 解析出的 labels 满足 `type:patch|minor|major` 且 channel 合法，则直接发送 Telegram，哪怕后续 `Release` 只会变成 `skipped`。
- `workflow_run` 在 `main` 的 `CI Pipeline` 失败但 merge commit 对应 `type:docs|type:skip`、label 缺失/歧义、或 release-intent API 解析失败时，必须抑制 owner-facing Telegram 告警。
- `workflow_dispatch` 可手动发送 smoke test 通知。
- 告警首行必须是 Emoji + 状态 + 项目名。
- 失败告警优先携带真实 release target SHA，而不是仅回退到 workflow 头 SHA。
- 首次瞬时 Docker / BuildKit / Docker Hub 拉取故障必须自动重跑 1 次 failed jobs，且当前 attempt 不发送 owner-facing Telegram 告警。
- 重跑后的后续 attempt 若仍失败，告警上下文必须能说明“已做过一次自动自愈后仍失败”。
