# 用户控制台公告演进历史（#aa7yu）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 公告能力独立成 topic spec，因为它同时跨管理端、用户控制台、后端持久化与浏览器本地状态。
- 用户关闭状态选择浏览器本地存储，满足“同一浏览器记忆公告 ID 与关闭时间”，避免引入跨设备已读同步和额外隐私状态。
- 已发布公告编辑时生成新公告 ID，并归档旧公告，确保内容更新后用户端不会被旧关闭记录吞掉。
- 管理端正文编辑使用 Milkdown Crepe，但只在进入创建/编辑视图时按需加载，避免公告列表路径承担编辑器依赖成本。
- Milkdown Crepe 的默认 Toolbar 与 LinkTooltip 在公告空状态会泄漏无标签浮层，因此公告编辑器关闭这些默认浮层，并用真实用户侧预览与保存并发布动作承载发布确认。
- 公告正文编辑器同时保留 Markdown、左右对比、所见即所得三种模式；任何预览态都使用 Milkdown 只读实例渲染，避免维护另一套 Markdown 预览实现。
- Storybook 静态构建使用轻量 Markdown 编辑器替身，真实 Milkdown 行为通过本地 demo mock 页面验证并沉淀视觉证据。

## Key Reasons / Replacements

- 弹窗公告用于强提醒，滚动公告用于低打扰提醒，两者共享同一公告模型但在用户端分别取最新发布项。
- 公告管理 API 使用既有 admin 判定，用户 API 使用既有 LinuxDo session 边界。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
