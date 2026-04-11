# jmdsq · Release 失败 Telegram 告警接入

## Summary
- 为 `Release` 工作流补一个 repo-local notifier wrapper，统一复用共享 Telegram 告警 workflow。
- 为 release 目标 SHA 增加显式日志标记，确保失败告警能定位真实 release head。
- 接入后通过 `workflow_dispatch` smoke test 验证 Telegram 通知链路。

## Scope
- 新增 `.github/workflows/notify-release-failure.yml`。
- 更新 `.github/workflows/release.yml` 输出 `RELEASE_REQUESTED_SHA` / `RELEASE_TARGET_SHA` 标记。
- 保持现有发布逻辑与 artifact 行为不变。

## Acceptance
- `workflow_run` 在 `Release` 失败时触发 Telegram 告警。
- `workflow_dispatch` 可手动发送 smoke test 通知。
- 告警首行必须是 Emoji + 状态 + 项目名。
- 失败告警优先携带真实 release target SHA，而不是仅回退到 workflow 头 SHA。
