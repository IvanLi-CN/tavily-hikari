# 用户控制台 Token 重置（#r8tkn）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 用户控制台已经允许用户查看和复制自己绑定的 Token，但只有管理员端具备重新生成 Token secret 的能力。
- 用户一旦怀疑 Token 泄露，需要不经管理员介入即可让旧完整 Token 失效并拿到新 Token。
- 既有 `2m7yv` 规范只覆盖 Token 明文显示，明确排除了轮换能力，因此用户侧写操作需要独立规范承载。

## 目标 / 非目标

### Goals

- 在用户控制台首页 Token 列表提供“重置”操作。
- 重置时保持 4 位 `tokenId` 不变，只重新生成 secret；旧完整 Token 立即失效。
- 重置操作必须要求用户确认；成功后展示新完整 Token，并尽量自动复制。
- 用户侧 API 必须校验 LinuxDo session、OAuth 启用状态和 Token 归属。
- Storybook 与自动化测试覆盖桌面/移动列表入口及重置成功路径。

### Non-goals

- 不清空 Token 用量、配额、日志或统计聚合。
- 不创建新 `tokenId`，不改变用户绑定、note、enabled 状态或管理员端轮换语义。
- 不触达生产 Tavily upstream；验收使用 mock、Storybook 或本地后端。

## 范围（Scope）

### In scope

- `POST /api/user/tokens/:id/secret/rotate`
- 用户控制台首页 Token 列表桌面表格和移动卡片操作区。
- 用户控制台 API client、i18n 文案、Storybook mock、相关测试与视觉证据。

### Out of scope

- 管理员端 Token detail 的既有 rotate 行为。
- Token 用量重算、配额窗口重置或历史日志修剪。
- LinuxDo OAuth 登录流程本身。

## 需求（Requirements）

### MUST

- 未启用 LinuxDo OAuth 时，用户侧 rotate API 返回 `404`。
- 未登录用户调用 rotate API 返回 `401`。
- 已登录用户只能 rotate 自己绑定的 Token；非本人 Token 返回 `404`。
- 已登录用户不能 rotate 已被禁用的绑定 Token；禁用 Token 返回 `404`。
- 成功 rotate 返回 `{ token: "th-<id>-<new-secret>" }`，且 `<id>` 与原 Token ID 一致。
- 用户确认前不得调用 rotate API；取消确认不改变任何 Token 状态。
- rotate 成功后前端必须清理旧 secret 缓存，后续复制/详情页展示使用新完整 Token。
- 禁用 Token 仍出现在列表时，前端不得展示可点击的重置入口。

### SHOULD

- 成功后自动复制新 Token；复制被浏览器阻止时展示只读文本框并选中文本。
- 列表操作按钮在桌面和移动布局中都不挤压配额/统计内容。

### COULD

- 后续可把用户侧 rotate 成功事件接入更细的审计日志，但本轮不要求。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户在 `/console` Token 列表点击“重置”。
- 页面显示确认对话框，说明旧完整 Token 会立即失效。
- 用户确认后，前端调用 `POST /api/user/tokens/:id/secret/rotate`。
- 后端验证用户 session 与 Token 归属，调用现有 secret rotate 存储能力。
- 前端拿到新完整 Token 后清理该 Token 的旧 secret 缓存，展示结果对话框并尝试复制。

### Edge cases / errors

- rotate 请求失败时，确认对话框保持打开并展示局部错误。
- rotate 返回 `401` 时，复用用户控制台既有登出/重新登录处理路径。
- 新 Token 自动复制失败时，结果对话框必须提供手动复制路径。
- 如果当前用户同时停留在该 Token 的详情上下文，已显示的明文 Token 必须更新为新值。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                         | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                   |
| ------------------------------------ | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ----------------------------------------------- |
| `/api/user/tokens/:id/secret/rotate` | HTTP API     | external      | New            | None                     | server          | UserConsole         | 用户 session + Token 归属校验；返回新完整 Token |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given 未登录用户
  When 调用 `POST /api/user/tokens/:id/secret/rotate`
  Then 返回 `401`。

- Given 已登录用户请求非本人 Token
  When 调用 `POST /api/user/tokens/:id/secret/rotate`
  Then 返回 `404`。

- Given 已登录用户请求本人 Token
  When rotate 成功
  Then 返回的新完整 Token 保留相同 `tokenId`，旧完整 Token 不再是当前 secret。

- Given 用户在 Token 列表点击“重置”
  When 还未确认
  Then 不发起 rotate 请求。

- Given rotate 成功
  When 用户再次复制同一 Token
  Then 复制值为新完整 Token，而不是旧缓存。

## 验收清单（Acceptance checklist）

- [ ] 核心路径的长期行为已被明确描述。
- [ ] 关键边界/错误场景已被覆盖。
- [ ] 涉及的接口/契约已写清楚或明确为 `None`。
- [ ] 相关验收条件已经可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Backend targeted test: `cargo test user_profile_and_user_token_reflect_linuxdo_session`
- Frontend targeted tests: `cd web && bun test src/UserConsole.stories.test.ts`

### UI / Storybook (if applicable)

- Update `User Console/UserConsole` mock so the Token list reset action can be exercised without real upstream calls.
- Capture Storybook visual evidence for the Token list with the reset action visible.

### Quality checks

- `cd web && bun run build`
- `cargo test` targeted or broader validation as time permits.

## Visual Evidence

source_type=storybook_canvas
story_id_or_title: `User Console/UserConsole / Console Home Tokens Focus`
state: reset confirmation
target_program: mock-only
capture_scope: browser-viewport
sensitive_exclusion: N/A
submission_gate: approved
evidence_note: 证明 Token 列表桌面操作区展示 Reset 按钮，并在确认前不会直接重置。

PR: include
![User console token reset confirmation](./assets/user-console-token-reset-dialog.png)

source_type=storybook_canvas
story_id_or_title: `User Console/UserConsole / Console Home Tokens Focus`
state: reset result
target_program: mock-only
capture_scope: browser-viewport
sensitive_exclusion: N/A
submission_gate: approved
evidence_note: 证明重置成功后展示新完整 Token，并提供复制/手动复制结果界面。

PR: include
![User console token reset result](./assets/user-console-token-reset-result.png)

## Related PRs

- None
