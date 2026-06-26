# Implementation

## Current Coverage

- LinuxDo OAuth 的正式浏览器落点已迁移到 `/console/oauth/linuxdo/callback`，前端在 `/console` 壳内承接 `code/state/error` query 并驱动 callback 状态机。
- 后端新增 `POST /auth/linuxdo/finalize`，复用既有 OAuth state 消费、userinfo 拉取、用户 upsert、token 绑定与用户会话建立逻辑。
- 旧 `GET /auth/linuxdo/callback` 已降级为诊断入口，不再承担正常登录收口。
- callback 视图已覆盖 connecting、success、provider denied、invalid request、invalid state、inactive user、timeout、upstream failure、server error 与 unsupported provider。
- `registration_paused` 继续定向到 `/registration-paused`，不与通用失败卡片混用。
- Storybook 已补 callback state gallery，前后端测试已切到 finalize contract。

## Validation

- `cargo test`
- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`

## References

- `./SPEC.md`
- `./HISTORY.md`
