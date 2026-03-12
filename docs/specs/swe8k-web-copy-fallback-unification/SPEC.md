# Web 全站复制兼容统一收口（#swe8k）

## 状态

- Status: 进行中（快车道）
- Created: 2026-03-12
- Last: 2026-03-12

## 背景 / 问题陈述

- 当前前端复制链路不一致：`/console`、`PublicHome`、`AdminDashboard`、`TokenDetail` 分别使用原生 `navigator.clipboard.writeText` 或各自的临时 fallback。
- 少部分用户会在浏览器、嵌入式 WebView 或非标准 secure-context 约束下遇到复制失败，尤其是用户控制台 token 复制按钮会直接显示“复制失败”。
- 部分入口只有复制按钮能拿到原文，自动复制失败后当前 UI 不会给出可恢复的原文容器，存在一次性 secret 丢失风险。

## 目标 / 非目标

### Goals

- 引入一套共享 clipboard helper，统一采用 `Clipboard API -> execCommand('copy')` 的顺序降级，并把结果显式返回给上层 UI。
- 引入一套共享“手动复制气泡”组件，专门覆盖“原文不在旁边可见、只能靠复制按钮拿到原文”的入口。
- 统一所有生产态复制入口：用户控制台、PublicHome、admin token/API key 复制、token share link、token regenerate/create 等场景都走同一兼容策略。
- 当复制失败且原文本来不可见时，必须展示只读编辑框、自动全选、允许用户立即手动复制；当原文本来可见时，至少保证现有字段/对话框可以直接手动复制。

### Non-goals

- 不修改任何后端 API、鉴权、token 格式或业务配额逻辑。
- 不新增前端测试框架；保持 Bun test + build + 浏览器验收。
- 不把 Storybook stories 作为本次兼容修复的主测试载体。

## 范围（Scope）

### In scope

- `web/src/lib/clipboard.ts`
  - 共享文本复制 helper
  - 共享只读文本自动全选 helper
- `web/src/components/ManualCopyBubble.tsx`
  - anchored bubble + viewport clamp + readonly field + auto select all
- 生产入口统一改造
  - `web/src/UserConsole.tsx`
  - `web/src/PublicHome.tsx`
  - `web/src/AdminDashboard.tsx`
  - `web/src/pages/TokenDetail.tsx`
- `web/src/index.css`
  - 手动复制气泡与只读编辑框样式
- 前端回归测试与相关 spec 同步

### Out of scope

- `src/**` Rust 服务端无改动
- 视觉系统重构、全站 Tooltip/Popover 基础设施重写
- Storybook 基建清理与额外 stories 大规模重做

## 需求（Requirements）

### MUST

- 所有生产复制入口统一走共享 helper。
- `Clipboard API` 失败后自动尝试 `document.execCommand('copy')` fallback。
- 对“按钮独占原文”的入口，若页面已提前 cache 到 secret，则应优先在同步点击栈里执行 legacy fallback，避免在异步请求后丢失 user activation。
- 当两条路径都失败且原文本来不可见时，必须弹出 anchored bubble，展示原文与只读编辑框。
- bubble 中的编辑框必须默认全选，且在点击/聚焦时再次全选；字段样式要允许 `user-select: all`。
- 可能只展示一次 secret 的流程（例如 create token / regenerate token）在复制失败后不得丢失原文。
- 已经直接展示原文的入口不重复弹 bubble，但必须保证用户可以直接从字段/对话框手动复制。

### SHOULD

- bubble 支持桌面与窄屏，避免被视口裁切。
- bubble 支持 `Escape` 与外部点击关闭。
- 复制失败后的 UI 反馈应优先就地恢复，不依赖全局错误 banner。

### COULD

- 在 helper 返回结果中保留成功路径/失败原因，方便后续 telemetry 或 UI diagnostics 复用。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户在 `/console#/tokens` 或 `/console#/tokens/:id` 点击复制完整 token：
  - 先尝试 `navigator.clipboard.writeText`
  - 若失败则尝试 `execCommand('copy')`
  - 若仍失败，则在触发按钮旁弹出手动复制气泡，显示完整 token 与只读输入框。
- 用户在 admin token/API key 列表点击复制 raw secret / share link：
  - 统一走共享 helper
  - 最终失败时在对应按钮旁弹出气泡，显示原文并允许手动复制。
- 用户在 PublicHome 的 access token 输入区点击复制：
  - 统一走共享 helper
  - 因原文已经在输入框里，失败时只显示错误态，不额外弹 bubble。
- admin create token / regenerate token 等一次性 secret 场景：
  - 自动复制失败后仍要把完整 secret 保留在当前界面可见容器中
  - 用户无需重新请求 secret 就能手动复制。

### Edge cases / errors

- `Clipboard API` 不可用、reject、或 `document` 不满足 fallback 条件时，helper 返回失败而不是抛出未处理异常。
- bubble 定位若靠近视口边缘，组件必须自动向内收敛位置。
- 多行内容（如批量分享链接）保持 textarea 只读手动复制；既有文本框可见时不额外创建 bubble。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）              | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）                                     | 备注（Notes）                  |
| ------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------------------------------------------- | ------------------------------ |
| `copyText()`              | internal     | internal      | New            | None                     | web             | PublicHome / UserConsole / AdminDashboard / TokenDetail | 统一文本复制 helper            |
| `selectAllReadonlyText()` | internal     | internal      | New            | None                     | web             | ManualCopyBubble / readonly secret fields               | 统一自动全选行为               |
| `ManualCopyBubble`        | internal     | internal      | New            | None                     | web             | UserConsole / AdminDashboard                            | 原文不可见入口的手动复制恢复层 |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given 浏览器支持 `navigator.clipboard.writeText`
  When 用户在任一生产复制入口点击复制
  Then 文本成功写入剪贴板，UI 按现有成功态更新。

- Given `navigator.clipboard.writeText` 被浏览器/环境拒绝
  When 同一入口继续执行 fallback
  Then 系统自动尝试 `document.execCommand('copy')`
  And 只要 fallback 成功，UI 仍按复制成功处理。

- Given 两条复制路径都失败且原文本来不可见
  When 用户点击复制 raw secret / full token / share link
  Then 触发按钮旁弹出手动复制气泡
  And 气泡内展示原文与只读编辑框
  And 编辑框打开即自动全选，点击/聚焦时再次全选。

- Given 原文本来已经显示在当前字段或对话框里
  When 自动复制或手动复制按钮失败
  Then 页面不强制额外弹 bubble
  And 用户仍可以直接从当前只读字段或对话框手动复制原文。

- Given create token / regenerate token 返回一次性 secret
  When 自动复制失败
  Then 原文不会丢失
  And 用户在关闭当前恢复容器前始终可以手动复制完整 secret。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认本次只改前端复制行为，不触碰后端接口
- 已确认“原文可见 vs 原文不可见”两类 UX 分流规则
- 已确认快车道允许直接推进到 push + PR + checks + review-loop

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `cd web && bun test ./src/lib/clipboard.test.ts`
- Integration tests: 如需要，补充最小 React/Bun 级组件验证，不引入新测试框架
- E2E tests (if applicable): Chrome 手工冒烟验证复制失败恢复分支

### UI / Storybook (if applicable)

- Stories to add/update: 非必需；如现有 story 因接口变化受影响，仅做最小适配
- Visual regression baseline changes (if any): None required for merge gate

### Quality checks

- `cd web && bun run build`
- `cd web && bun test`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增 spec 索引并同步状态
- `docs/specs/swe8k-web-copy-fallback-unification/SPEC.md`: 跟踪实现与 review-loop 收敛

## 计划资产（Plan assets）

- Directory: `docs/specs/swe8k-web-copy-fallback-unification/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.

## Visual Evidence (PR)

- 2026-03-12: Chrome DevTools 手工冒烟
  - `http://127.0.0.1:55174/#a1b2`：可见原文入口在双路径复制失败后不重复弹 bubble，但会自动 reveal 并选中当前 token，便于立即手动复制。
  - `http://127.0.0.1:55174/console#/tokens`：列表复制失败后弹出手动复制气泡，输入框默认聚焦且全选；同一按钮下次复制成功后旧气泡会自动关闭。
  - `http://127.0.0.1:55174/console#/tokens/a1b2`：窄屏下 detail 复制失败后 bubble 仍保持在视口内，点击/聚焦会再次全选。
  - `http://127.0.0.1:58089/admin/tokens`：新建 token、复制完整 token、复制分享链接在失败时均弹出手动复制气泡。
  - `http://127.0.0.1:58089/admin/tokens/m87I`：rotate secret 失败时复用现有对话框，展示只读 textarea 并自动全选，不额外弹 bubble。
  - `http://127.0.0.1:58089/admin/tokens`：列表初次加载仅请求 `summary/profile/tokens/...` 等基础数据，不再自动拉取 `/api/tokens/:id/secret` 或 `/api/keys/:id/secret`；hover 到复制按钮后才按需触发 secret 请求。
  - `http://127.0.0.1:58089/admin/tokens/m87I -> /admin/tokens`：rotate 后返回列表，强制复制失败时手动复制气泡展示的分享链接已经带上最新 token `th-m87I-210OK5iEU2T5lM7X1nVZYwJ5`，未读到旧 cache。

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: Spec 建立并冻结复制兼容范围与验收口径
- [x] M2: 共享 clipboard helper + ManualCopyBubble 落地
- [x] M3: 全站生产复制入口统一接入并补齐一次性 secret 恢复容器
- [ ] M4: Bun 测试、build、浏览器验收已完成；PR / checks / review-loop 收口中

## 方案概述（Approach, high-level）

- 把复制兼容性下沉到共享 helper，避免每个页面各自判断浏览器能力。
- 把“最终失败后的手动复制恢复”下沉到共享气泡组件，统一定位、关闭行为和只读编辑框交互。
- 页面层只负责声明：当前入口是否已有可见原文，以及失败时应该展示的原文内容与锚点。
- 对隐藏 secret 的按钮独占入口，仅保留 hover/focus 触发的按需 warm-up，把 legacy fallback 尽量保留在同步点击栈里；禁止在列表初次加载时批量预取 secret。
- admin token rotate 成功后必须同步更新父级 secret cache，避免返回列表后复制/分享仍命中旧值。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：`execCommand('copy')` 已废弃，仅能作为兼容 fallback，长期不能依赖它覆盖所有环境。
- 风险：不同 WebView 对 focus/selection 行为可能有细微差异，需要浏览器实测确认。
- 假设：当前前端入口都能在触发时拿到按钮锚点或可持续显示原文的容器。

## 变更记录（Change log）

- 2026-03-12: 初始化规格，冻结复制兼容统一 helper、手动复制气泡与一次性 secret 恢复口径。
- 2026-03-12: 落地 `copyText()` / `selectAllReadonlyText()` / `ManualCopyBubble`，统一接入 PublicHome、UserConsole、AdminDashboard、TokenDetail，并补齐 Bun 测试、build 与浏览器冒烟。
- 2026-03-12: 修复 `ManualCopyBubble` 首次打开时因定位前短路渲染而不显示的问题，改为先挂载再定位，未完成定位前仅隐藏且禁用指针事件。
- 2026-03-12: 为 `execCommand` fallback 补充 iOS / iPadOS 选区兼容分支，并在 PublicHome 复制失败时自动 reveal 当前 token，确保“原文可见入口”仍可手动复制。
- 2026-03-12: 补齐复制 review 收口：同步优先的 legacy fallback 选项、UserConsole/Admin secret cache 预热、复制成功时自动关闭旧气泡，以及 PublicHome / rotated token 失败后的自动重新选中。
- 2026-03-12: 收紧 secret 生命周期：移除 UserConsole/Admin 列表初载时的全量 secret 预取，仅保留 hover/focus 按需 warm-up；同时在 admin rotate token 后回写父级 secret cache，避免后续复制/分享读到旧 token。

## 参考（References）

- MDN Clipboard API / `writeText()` secure-context & transient-activation constraints
- MDN `document.execCommand('copy')` deprecated fallback guidance
