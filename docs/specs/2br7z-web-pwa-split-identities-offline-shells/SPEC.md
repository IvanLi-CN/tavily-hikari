# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）

## 状态

- Status: 已完成（快车道）
- Created: 2026-06-24
- Last: 2026-07-11

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
- 将 public/admin 双身份 PWA 的图标、touch icon 与站点 favicon 收口到 repo-local 的经批准 Relay Mesh lockup/icon 导出链，不改变 identity/scope/start_url。
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
- `web/public/assets/relay-mesh-lockup*.png`
- `web/public/assets/relay-mesh-icon*.png`
- `web/public/assets/relay-mesh-mark*.{png,svg}`
- `web/public/assets/linuxdo-logo.svg`
- `web/public/assets/favicon-*.png`
- `web/public/assets/apple-touch-icon.png`
- `web/src/*main.tsx`
- `web/src/api/runtime.ts`
- `web/src/components/**`
- `web/src/PublicHome.tsx`
- `web/src/user-console/runtime.tsx`
- `web/src/admin/AdminDashboardRuntime.tsx`
- `docs-site/rspress.config.ts`
- `docs-site/docs/public/assets/relay-mesh-lockup*.png`
- `docs-site/docs/public/assets/relay-mesh-icon*.png`
- `docs-site/docs/public/assets/relay-mesh-mark*.{png,svg}`
- `docs-site/docs/public/assets/favicon-*.png`
- `docs-site/docs/public/assets/apple-touch-icon.png`
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

## 更新提示合同

- `sw-public.js` 与 `sw-admin.js` 安装时必须先完成 precache，再进入 waiting；不得在 install 阶段主动 `skipWaiting()`。
- 页面检测到 `/api/version.frontend` 变化时，只触发当前 identity 的 `registration.update()`；用户可见的更新提示必须以 service worker 已发现新 worker、并至少进入 installing/ready 更新生命周期为准。
- 用户点击更新时：
  - 若新 worker 尚在安装或缓存资源，更新按钮保持 loading，等待 worker 进入 waiting 后再发送激活消息。
  - 若新 worker 已经 waiting，页面向该 worker 发送 `TAVILY_HIKARI_ACTIVATE_UPDATE`，由 worker `skipWaiting()`。
  - 页面只在用户主动更新后的 `controllerchange` 中 reload，避免静默打断当前任务。
  - waiting worker 已进入 `activated` 但当前页未收到 `controllerchange` 时，页面允许执行一次受 guard 保护的 reload；不得形成刷新循环。
  - 激活请求在 10 秒内既未接管页面也未确认 worker 已激活时，提示必须退出 loading 并进入可重试失败态；`redundant` 与消息发送异常同样按失败处理。
- 首次安装 identity 时以当前 registration 是否已有 active worker 判定是否属于更新；public 根作用域 controller 不得让 admin 首次安装误报“有新版本”，此时 admin waiting worker 应静默激活。
- 更新提示必须覆盖 `/`、`/console`、`/login`、`/registration-paused` 与 `/admin/**`，但继续保持 public/admin 双 service worker 边界。
- 提示形态为 inline banner，不使用 modal，不强制用户立即刷新。

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
- `web/public/assets/relay-mesh-lockup*.png`
- `web/public/assets/relay-mesh-icon*.png`
- `web/public/assets/relay-mesh-mark*.{png,svg}`
- `web/public/assets/linuxdo-logo.svg`
- `web/public/assets/favicon-*.png`
- `docs-site/docs/public/assets/relay-mesh-lockup*.png`
- `docs-site/docs/public/assets/relay-mesh-icon*.png`
- `docs-site/docs/public/assets/relay-mesh-mark*.{png,svg}`
- `docs-site/docs/public/assets/favicon-*.png`

### 构建输入

- Vite build manifest 必须开启，供 post-build 读取 multipage output graph。
- 生成脚本必须按 entrypoint 归类 public/admin asset graph，并输出两套 PWA 合同文件。
- Relay Mesh 资产导出链必须显式产出 light / dark / mono 变体，并保留默认亮色别名文件用于现有入口兼容。
- PWA manifest 必须覆盖 `64, 96, 128, 144, 152, 167, 180, 192, 256, 384, 512, 1024` 尺寸，并额外提供 `192/512` maskable 图标。

### 静态托管

- Rust 静态服务必须可直出 `.webmanifest`、`sw-public.js`、`sw-admin.js` 与 `pwa/*` 图标资产。
- `.webmanifest` 返回 `application/manifest+json`。
- service worker 脚本必须可在浏览器直接访问。
- owner-facing 品牌位统一通过 `/assets/*` 暴露；`/favicon.svg` 只作为站点 favicon 入口保留根路径合同。

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

- Given 后端报告新的 `frontend` 版本
  When 当前 identity 的 service worker 尚未完成新资源安装
  Then 页面只触发更新检查，不提示“可更新”。

- Given 新 service worker 正在安装并缓存资源
  When 用户点击更新按钮
  Then 更新按钮进入 loading，直到 worker ready 后激活并刷新当前页。

- Given 新 service worker 已经 waiting
  When 用户点击更新按钮
  Then 页面发送 `TAVILY_HIKARI_ACTIVATE_UPDATE` 并在 `controllerchange` 后 reload。

- Given 用户已请求激活 waiting worker
  When 10 秒内没有 controller 接管、worker 激活确认或可恢复终态
  Then 更新提示退出 loading，说明更新未完成，并允许用户重试或暂不提醒。

- Given admin identity 首次注册且当前页面仅由 public 根作用域 service worker 控制
  When admin worker 完成安装
  Then admin worker 静默激活，不展示版本更新提示，不触发主动 reload。

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
- `95768005+` Relay Mesh public 品牌入口：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-public-home.png` PR: include
- `95768005+` Relay Mesh console 品牌入口：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-console-header.png`
- `95768005+` Relay Mesh admin 品牌入口：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-shell.png` PR: include
- `95768005+` Relay Mesh admin login 品牌入口：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-login.png`
- `95768005+` Relay Mesh registration-paused 品牌入口：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-registration-paused.png`
- `95768005+` Relay Mesh docs-site 品牌入口：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-docs-site.png`
- `95768005+` Relay Mesh PWA/icon 导出预览：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-pwa-icons.png` PR: include
- `2026-06-27` 品牌静态资源 `/assets` 路由校准后的 admin 壳验证：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-shell-assets-route-fixed.png` PR: include
- `2026-07-08` PWA 更新提示 ready 状态（Storybook canvas）：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-ready-storybook.png`
- `2026-07-08` PWA 更新提示 installing/loading 状态（Storybook canvas）：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-installing-storybook.png`
- `2026-07-08` PWA 更新提示 dark ready 状态（Storybook canvas）：`docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-dark-ready-storybook.png`
- `2026-07-11` PWA 更新激活失败亮色态（Storybook canvas，mock-only，element capture，无敏感数据）：

  PR: include

  ![PWA 更新激活失败亮色态](./assets/update-banner-activation-failed-storybook.png)

- `2026-07-11` PWA 更新激活失败暗色态（Storybook canvas，mock-only，element capture，无敏感数据）：

  PR: include

  ![PWA 更新激活失败暗色态](./assets/update-banner-activation-failed-dark-storybook.png)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 双 manifest / 双 service worker 生成管线落地
- [x] M2: 入口页注册、HTML 合同与 Rust 静态托管落地
- [x] M3: public/admin cache 边界与 navigation fallback 落地
- [x] M4: 公共页、控制台、后台离线错误态收口
- [x] M5: Storybook、测试、浏览器离线验证、Relay Mesh 品牌接入与视觉证据完成

## 风险 / 假设

- 假设：本轮“不要让非管理员缓存 admin Web App”的定义聚焦于 PWA/service worker 长期缓存与安装身份，而不是普通 HTTP 层的瞬时下载。
- 风险：Safari 对多 identity 安装入口与 scope 的表现比 Chromium 更保守，因此需要明确手工结论。
- 风险：管理员后台已有大量模块化加载状态，离线时若错误语义分散，必须通过共享错误规范避免出现局部空白。
- 假设：产品命名继续保持 `Tavily Hikari` / `Tavily Hikari Proxy`，`Relay Mesh` 仅作为视觉资产方向，不构成对外 rename。

## 变更记录（Change log）

- 2026-06-24: 创建 spec，冻结双身份 PWA、离线壳与管理员缓存预算控制的实现合同。
- 2026-06-24: 完成 Vite multipage 双 manifest / 双 service worker 生成、PWA 图标产线、前端入口注册、Rust 静态托管与主界面离线提示第一版。
- 2026-06-24: 补齐 Chromium 离线视觉证据，确认 public identity 离线访问 `/admin` 不会命中 cached admin shell。
- 2026-06-24: 将统一离线提示 banner 图标从 `mdi:earth-off` 调整为更贴近无网络语义的 `mdi:web-off`，并更新对应视觉证据。
- 2026-06-25: 将 split public/admin PWA 图标、touch icon 与站点 favicon 切换到经批准的 Relay Mesh lockup/icon 导出链，并同步接入 public/console/admin/docs-site 品牌位而不改变 PWA identity 合同。
- 2026-06-25: 补齐 Relay Mesh light/dark/mono 变体、主题感知 favicon 与全尺寸 PWA icon 覆盖，并更新品牌资产导出预览证据。
- 2026-06-27: 品牌静态资源合同统一收口到 `/assets/*`；根路径 Relay Mesh 与 LinuxDo 品牌资源退出长期公开路由，仅保留 `/favicon.svg` 作为站点入口。
- 2026-07-08: 补齐 public/admin PWA 更新提示合同，要求新 service worker 完成资源缓存后等待用户确认激活，并以 inline banner 覆盖全部 Web 入口。
- 2026-07-11: 更新激活状态机增加 10 秒 watchdog、失败重试、`redundant`/消息异常终态与 activated 单次刷新回退，并按 registration 自身 active worker 区分首次安装与升级。
