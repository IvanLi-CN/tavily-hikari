# 用户控制台 Token 重置演进历史（#r8tkn）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 用户侧 Token 重置独立成新规范，因为 `2m7yv` 只覆盖用户控制台 Token 明文显示，且明确排除了轮换能力。
- PR #284 创建后与 `origin/main` 同步，保留 main 上 clay redesign 相关规范索引，同时追加 `r8tkn` 条目。
- PR-stage review 指出禁用但仍绑定的 Token 不应由用户侧 rotate 重新披露 secret，因此 rotate 改为先通过用户侧 secret 可见性检查。
- PR Frontend Checks 暴露 source budget 超限，重置对话框和 Token list 操作组已拆出 runtime。
- PR-stage review 指出禁用 Token 不应展示必然失败的 reset 操作，列表操作组改为仅 enabled Token 展示重置入口。

## Key Reasons / Replacements

- “重置 Token”定义为重新生成 secret 并保持 `tokenId` 不变，避免破坏用户绑定与历史用量归属。
- 用户侧 API 独立于管理员 API，鉴权边界以 LinuxDo session 与 Token 归属为准。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
