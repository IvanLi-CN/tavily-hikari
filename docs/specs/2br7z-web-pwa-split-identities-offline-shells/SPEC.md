# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）

## 状态

- Status: 进行中（快车道）
- Created: 2026-06-24
- Last: 2026-06-24

## 背景 / 问题陈述

- 当前 Web 前台、用户控制台与管理员后台都没有真正的 PWA 合同，已访问用户在断网时只能看到浏览器级失败或空白。
- 现有静态托管是 multipage HTML，但没有 service worker、manifest、离线壳页或浏览器安装身份，因此无法为已访问用户提供“离线还能打开页面框架，但数据区明确失败”的体验。
- 你的核心资源约束不是“前端包绝对保密”，而是“不让普通用户长期缓存管理员 Web App”；这要求 admin 与 public 不能共享单一 PWA identity。

## 目标 / 非目标

### Goals

- 将公共/用户侧与管理员侧拆成两套独立 PWA identity、manifest、service worker、scope 与安装入口。
- 让已在线访问过相应页面的用户在离线时仍可打开公共首页、用户控制台与管理员后台的页面壳。
- 所有业务数据请求、SSE、MCP、登录提交与保存/操作在离线时都保持明确失败语义，不伪造成功，不回显旧快照。
- 非管理员不注册 admin service worker、不看到 admin manifest 安装入口、不在 public SW cache 中形成 admin 壳页长期缓存。
- 补齐测试、Storybook 状态、浏览器离线验证与视觉证据，并将合同冻结到本 spec。

### Non-goals

- 不实现离线业务 JSON 快照、最后成功数据回显、Background Sync、Push 或离线 mutation 队列。
- 不调整后端鉴权模型、管理员 cookie、LinuxDo OAuth 流程或 `/api/*` 返回结构。
- 不为了完全阻止普通用户瞬时下载任意 admin 前端资源而重构静态资源权限托管。
- 不把 `/login` 纳入 admin PWA scope，也不拆出第三套独立身份给 `/console`。

## 范围（Scope）

### In scope

- `web/vite.config.ts`
- `web/package.json`
- `web/scripts/**`
- `web/*.html`
- `web/src/*main.tsx`
- `web/src/api/runtime.ts`
- `web/src/components/**`
- `web/src/PublicHome.tsx`
- `web/src/user-console/runtime.tsx`
- `web/src/admin/AdminDashboardRuntime.tsx`
- `src/server/spa.rs`
- `src/server/serve.rs`
- `docs/specs/README.md`

### Out of scope

- 后端业务 API handler 语义与数据库迁移。
- Firefox PWA 安装的正式兼容承诺。
- 新路由框架接入或单页路由体系重写。

## PWA 身份合同

### 公共 / 用户侧 identity

- `manifest.webmanifest`
- `scope=/`
- `start_url=/`
- 安装入口存在于 `/`、`/console/**`、`/login`、`/registration-paused`
- public service worker 仅缓存公共/用户侧 HTML 壳与对应静态资源

### 管理员 identity

- `manifest-admin.webmanifest`
- `scope=/admin/`
- `start_url=/admin`
- 安装入口只存在于 `/admin/**`
- admin service worker 只缓存 admin HTML 壳与对应静态资源

### 关键边界

- `login.html` 只归公共 identity，不暴露 admin manifest。
- `admin.html` 不承载公共 manifest。
- public service worker 不能把 `/admin/**` 作为 navigation fallback，也不能把 admin HTML 或 admin 入口图纳入 precache/runtime cache。
- admin identity 只在管理员真实进入 `/admin/**` 后才注册并形成持久缓存。

## 离线行为合同

### Public / Console

- 已访问过 `/`、`/console/**`、`/login`、`/registration-paused` 的用户，在离线时仍可打开相应壳页。
- 页面框架、主题切换、静态文案与本地 UI 状态可用。
- `/api/*`、`/mcp`、SSE、登录动作、保存动作一律 network-only，失败时显示明确错误。
- 离线访问 `/admin` 时，public SW 不提供 admin shell fallback。

### Admin

- 已作为管理员在线访问过 `/admin/**` 的浏览器，在离线时可打开 admin 壳页与路由骨架。
- Dashboard、列表、HA、设置、日志流与保存动作均不展示旧业务数据快照，只显示明确失败或不可用状态。
- admin SW 只能在 admin scope 内处理 navigation fallback，不接管 `/`、`/console`、`/login`。

## 接口契约（Interfaces & Contracts）

### 产物

- `web/dist/manifest.webmanifest`
- `web/dist/manifest-admin.webmanifest`
- `web/dist/sw-public.js`
- `web/dist/sw-admin.js`
- `web/dist/pwa/public-*.png`
- `web/dist/pwa/admin-*.png`
- `web/dist/pwa/public-touch-icon.png`
- `web/dist/pwa/admin-touch-icon.png`

### 构建输入

- Vite build manifest 必须开启，供 post-build 读取 multipage output graph。
- 生成脚本必须按 entrypoint 归类 public/admin asset graph，并输出两套 PWA 合同文件。

### 静态托管

- Rust 静态服务必须可直出 `.webmanifest`、`sw-public.js`、`sw-admin.js` 与 `pwa/*` 图标资产。
- `.webmanifest` 返回 `application/manifest+json`。
- service worker 脚本必须可在浏览器直接访问。

## 验收标准（Acceptance Criteria）

- Given 普通用户只访问过公共页或控制台
  When 浏览器离线后访问 `/` 或 `/console/**`
  Then 页面壳可打开，数据区显示明确离线/加载失败提示。

- Given 普通用户安装了公共 PWA
  When 浏览器离线后直接访问 `/admin`
  Then 不得命中 cached admin shell，不得形成 admin 安装身份，只能得到网络失败或非 admin fallback 语义。

- Given 真实管理员已在线访问 `/admin/**`
  When 离线后重开 `/admin/**`
  Then admin 壳与导航可打开，但数据模块、保存与操作全部维持失败语义，不显示旧业务快照。

- Given 任意身份离线
  When 触发 `/api/*`、SSE、MCP、登录提交、保存动作
  Then 一律保持 network failure 语义，不返回伪成功。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cd web && bun test`
- `cargo test`

### Build

- `cd web && bun run build`
- `cd web && bun run build-storybook`

### Browser / E2E

- `bun run test:e2e:pwa-offline`
- Chromium 自动化覆盖公共页、控制台、管理员后台三段离线路径。
- Safari/iOS 至少做一次手工安装与离线重开验证，并将结论写入 spec evidence 或 implementation notes。

## Visual Evidence

- `97cccf60` 公共首页离线壳：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/public-offline-shell.png`
- `97cccf60` 用户控制台离线壳：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/console-offline-shell.png`
- `97cccf60` 管理员后台离线壳：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/admin-offline-shell.png`
- 统一离线提示 banner 图标调整：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/offline-banner-web-off.png`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 双 manifest / 双 service worker 生成管线落地
- [x] M2: 入口页注册、HTML 合同与 Rust 静态托管落地
- [x] M3: public/admin cache 边界与 navigation fallback 落地
- [x] M4: 公共页、控制台、后台离线错误态收口
- [ ] M5: Storybook、测试、浏览器离线验证与视觉证据完成

## 风险 / 假设

- 假设：本轮“不要让非管理员缓存 admin Web App”的定义聚焦于 PWA/service worker 长期缓存与安装身份，而不是普通 HTTP 层的瞬时下载。
- 风险：Safari 对多 identity 安装入口与 scope 的表现比 Chromium 更保守，因此需要明确手工结论。
- 风险：管理员后台已有大量模块化加载状态，离线时若错误语义分散，必须通过共享错误规范避免出现局部空白。

## 变更记录（Change log）

- 2026-06-24: 创建 spec，冻结双身份 PWA、离线壳与管理员缓存预算控制的实现合同。
- 2026-06-24: 完成 Vite multipage 双 manifest / 双 service worker 生成、PWA 图标产线、前端入口注册、Rust 静态托管与主界面离线提示第一版。
- 2026-06-24: 补齐 Chromium 离线视觉证据，确认 public identity 离线访问 `/admin` 不会命中 cached admin shell。
- 2026-06-24: 将统一离线提示 banner 图标从 `mdi:earth-off` 调整为更贴近无网络语义的 `mdi:web-off`，并更新对应视觉证据。
