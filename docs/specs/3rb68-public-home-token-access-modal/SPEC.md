# PublicHome：未登录无令牌时隐藏 Token 面板 + 令牌访问弹窗入口（#3rb68）

## 状态

- Status: 待实现
- Created: 2026-02-28
- Last: 2026-02-28

## 背景 / 问题陈述

- 未登录访问首页时，当前页面仍展示「令牌使用统计」「近期请求」与 Access Token 输入框；当用户既未登录又没有 token 信息时，这些信息对新用户无意义，会增加认知负担。
- 旧用户仍可能只通过 token 链接或手工输入 token 来查看用量与近期请求，需要保留“基于 token 的访问入口”。

## 目标 / 非目标

### Goals

- 未登录且 token 为空时，隐藏首页的「令牌使用统计」「近期请求」两块（含 Access Token 行）。
- 在首屏按钮区增加「使用令牌访问」按钮；点击后弹出与现有 Access Token 输入框功能等价的弹窗（输入、遮罩切换、复制）。
- 用户在弹窗确认使用后，调用现有 token 持久化逻辑（hash/localStorage）并恢复当前 token 体验（统计 + 近期请求）。
- 弹窗内提示：建议使用 linux.do 登录以绑定账号（跳转 `/auth/linuxdo`）。

### Non-goals

- 不改后端 API、鉴权链路或 SSE 协议。
- 不新增前端测试框架（保持现有 build + 手工验收）。
- 不改变已登录自动回填 token 的策略（URL hash / localStorage / `/api/user/token`）。

## 范围（Scope）

### In scope

- `web/src/PublicHome.tsx`
  - 增加 `hideTokenPanels` 判定并在满足条件时隐藏两块 panel
  - 增加 token access modal（draft state + confirm/cancel；取消不污染页面 token）
  - 首屏按钮区增加 token access 入口按钮
- `web/src/i18n.tsx`
  - 新增 `public.tokenAccess.*` 文案（en/zh）
- `web/src/index.css`
  - 如有需要，为 token access modal 补充移动端输入行自适配样式（不影响其它 modal）

### Out of scope

- `src/**` 后端无改动
- 数据库 schema 无变更

## 验收标准（Acceptance Criteria）

- Given `GET /api/profile` 返回 `userLoggedIn=false` 且首页无 token（hash 为空、localStorage 无 last token）
  When 页面渲染
  Then 不显示「令牌使用统计」「近期请求」两块，但 Linux DO 登录按钮仍可见
  And 首屏按钮区出现「使用令牌访问」按钮。

- When 点击「使用令牌访问」打开弹窗，输入合法 `th-xxxx-...` token 并确认
  Then 弹窗关闭，页面恢复现有 token 体验：显示「令牌使用统计」与「近期请求（最近 20 条）」两块，并按原逻辑拉取数据。

- When 弹窗中输入部分内容后点击取消
  Then 页面 token 仍为空，且两块 panel 仍保持隐藏。

- Given URL hash/localStorage 已有 token，或 `userLoggedIn=true` 且 `/api/user/token` 回填成功
  Then 首页行为与当前一致：两块 panel 正常展示，不强制用户走弹窗路径。

## 非功能性验收 / 质量门槛（Quality Gates）

- Frontend build: `cd web && bun run build`
- 手工浏览器验证：覆盖未登录无 token / 输入 token / 取消 / 已登录自动回填等分支。

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: Spec + i18n 文案补齐
- [ ] M2: 首页隐藏逻辑 + token access modal 落地
- [ ] M3: 构建验证 + 手工验收通过
- [ ] M4: fast-track 交付（push + PR + checks + review-loop）

## 变更记录（Change log）

- 2026-02-28: 创建规格，冻结范围与验收口径。
