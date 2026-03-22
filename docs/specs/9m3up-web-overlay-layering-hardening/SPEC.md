# Web 全站浮层防遮挡收口（#9m3up）

## 状态

- Status: 部分完成（3/5）
- Created: 2026-03-22
- Last: 2026-03-22

## 背景 / 问题陈述

- 前端当前同时存在 Radix portal 浮层、DaisyUI 风格 `.tooltip::after` / `dropdown-content`，以及若干局部 `position: absolute` 气泡。
- 表格滚动壳、`overflow` 容器、sticky 区块与 modal/drawer 叠加后，局部布局树里的气泡会被截断或被更高层内容盖住。
- 单点追加 `z-index` 只能修个别页面，不能从根上杜绝新浮层继续落在错误的层级模型里。

## 目标 / 非目标

### Goals

- 为 `web/` 建立统一的浮层层级 tokens、共享 `Tooltip` primitive、共享 anchored floating 定位 hook。
- 迁移生产态遗留 `.tooltip[data-tip]`、局部 `absolute` 气泡、DaisyUI mobile guide dropdown。
- 让 tooltip / popover / dropdown / modal 在滚动表格、裁剪容器和对话框里保持稳定可见。
- 增加静态 guard、Storybook proof 与测试，阻止旧模式回流。

### Non-goals

- 不重做视觉风格或主题系统。
- 不改动后端 API、DTO、数据库或业务逻辑。
- 不重写已是 Radix portal 的 `Dialog` / `DropdownMenu` / `Select` 交互语义。

## 范围（Scope）

### In scope

- `docs/specs/9m3up-web-overlay-layering-hardening/**`
  - 记录 legacy overlay inventory、替换合同与收敛证据。
- `web/package.json`
  - 补 `@radix-ui/react-tooltip` 直依赖。
- `web/src/components/ui/tooltip.tsx`
  - 共享 Tooltip primitive。
- `web/src/lib/useAnchoredFloatingLayer.ts`
  - 共享 anchored floating 定位 hook。
- `web/src/index.css`
  - 统一 layer tokens 与遗留 overlay 样式清理。
- 生产调用点
  - `web/src/AdminDashboard.tsx`
  - `web/src/pages/TokenDetail.tsx`
  - `web/src/components/JobKeyLink.tsx`
  - `web/src/components/ConnectivityChecksPanel.tsx`
  - `web/src/components/ApiKeysValidationDialog.tsx`
  - `web/src/PublicHome.tsx`
  - `web/src/UserConsole.tsx`
  - `web/src/components/ManualCopyBubble.tsx`
  - `web/src/admin/ForwardProxyEgressControl.tsx`
  - `web/src/admin/ForwardProxySettingsModule.tsx`
- Storybook / tests
  - 相关 stories、Bun tests、legacy pattern source-scan。

### Out of scope

- Rust 服务端实现。
- 生产上游联调与非 mock 截图流程。

## 实现合同（Implementation Contract）

- 生产代码不得再使用 `data-tip`、`.tooltip::after`、`.dropdown-content` 来实现可见浮层。
- 所有文本提示统一走共享 `Tooltip` primitive：
  - 默认渲染到 `document.body`。
  - 默认开启 collision handling 与 viewport padding。
  - 样式统一使用共享 layer token，而不是页面局部 `z-index`。
- 所有富内容锚定气泡统一走共享 anchored floating hook：
  - 通过 `createPortal(..., document.body)` 渲染。
  - 自动监听 anchor / bubble 自身尺寸变化、窗口 resize 与祖先 scroll。
  - 自动在首选方向与反向 fallback 间切换，并向视口内收敛。
- `Dialog`、`Drawer`、`DropdownMenu`、`Select` 必须改用统一 layer token，保证 modal 与 floating content 的相对顺序稳定。
- `PublicHome` 与 `UserConsole` 的 mobile guide 菜单必须改为 Radix `DropdownMenu`。
- 新增源码 guard：
  - 失败条件至少包含生产态 `data-tip`、生产态 `dropdown-content`、遗留 `.tooltip[data-tip]` 样式回流。

## 验收标准（Acceptance Criteria）

- Given `/admin/users` 或对应 Storybook `Admin/Pages -- UsersUsage`
  When 鼠标悬停或键盘 focus 表头提示
  Then tooltip 在表格滚动壳外完整显示，不被 header / table wrapper 裁剪。

- Given `/admin/jobs` 或 `Admin/Components/JobKeyLink -- BubbleProof`
  When jobs key 分组气泡打开
  Then 气泡渲染在 portal 层，不依赖局部伪元素。

- Given `/console#/tokens/:id` 或 `User Console/Fragments/Connectivity Checks -- State Gallery`
  When probe bubble 可见
  Then bubble 不再依赖按钮局部 absolute 定位，且在窄视口内仍保持可见。

- Given PublicHome / UserConsole 的 mobile guide 菜单
  When 菜单打开
  Then 菜单内容通过 portal 浮在容器之上，不再依赖 `dropdown-content`。

- Given 运行源码 guard
  When 生产代码重新引入 legacy overlay pattern
  Then Bun test 直接失败并指出命中文件。

## 质量门槛（Quality Gates）

- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
- Chrome DevTools 浏览器验收：Storybook + leased local preview

## Visual Evidence (PR)

- Storybook `Admin/Pages -- UsersUsage`
- Storybook `Admin/Components/JobKeyLink -- BubbleProof`
- Storybook `User Console/Fragments/Connectivity Checks -- State Gallery`
- Storybook proof for mobile guide dropdown clipping

## 里程碑

- [x] M1: 规格建档与 legacy overlay inventory 冻结
- [x] M2: 共享 Tooltip / anchored floating / layer tokens 落地
- [x] M3: 生产态 legacy overlay 调用点迁移完成
- [ ] M4: Storybook proof、源码 guard 与前端验证完成
- [ ] M5: 快车道 PR / checks / review-loop 收口到 merge-ready
