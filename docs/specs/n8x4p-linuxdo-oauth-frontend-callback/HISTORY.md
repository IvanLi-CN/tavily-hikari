# History

## Decision Trace

- 2026-06-26: 决定直接把 LinuxDo redirect URI 切到 `/console/oauth/linuxdo/callback`，不保留长期双栈 callback 收口。
- 2026-06-26: 决定由前端 callback 页通过 `POST /auth/linuxdo/finalize` 提交 `code/state`，以便在 `/console` 壳内统一承接连接中、成功、失败、超时与取消态。
- 2026-06-26: 决定失败与超时只提供 fresh restart + home CTA，不自动复用旧授权码重试。
- 2026-06-26: 决定保留 `/registration-paused` 专页分流，并把旧 `GET /auth/linuxdo/callback` 改成显式诊断入口。

## Key Reasons

- OAuth callback 的主要问题不是后端能否完成登录，而是用户在异常路径下缺少稳定、可理解、可恢复的反馈界面。
- `/console` 已经具备 path 路由与壳层能力，把 callback 收口进来可以复用既有视觉体系、i18n、reduced-motion 与 Storybook 验收面。
- `finalize` 把“OAuth provider 返回浏览器”与“本地完成会话建立”拆开后，前端才能控制超时、状态 copy、CTA 与 redirect 节点，而不需要继续暴露裸 HTTP 错误页。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
