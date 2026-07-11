# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）实现记录

## 当前实现状态

- 状态：已完成（含 Relay Mesh 品牌接入与可恢复 PWA 更新提示；待 Safari / iOS 手工补验）
- 分支：`th/fix-pwa-update-activation-stall`

## 实现决策

- 采用现有 Vite multipage 构建，新增 build manifest 输出与 post-build 脚本。
- 通过生成脚本构造 public/admin 两套 asset graph、manifest、service worker 与图标，不引入单 manifest 注入式 PWA 插件。
- 品牌资产采用“批准稿 lockup/icon + 位图导出”双轨：仓库保留经批准的 Relay Mesh lockup/icon PNG，并通过构建稳定导出 favicon、touch icon 与 public/admin PWA PNG。
- 品牌导出链现在显式产出 lockup / mark / launcher icon 的 light、dark、mono 变体，并保留默认亮色别名文件给现有入口复用。
- owner-facing 品牌静态资源统一改由 `/assets/*` 合同暴露；根路径只保留 `/favicon.svg`、manifest 与 PWA 入口，不再公开 Relay Mesh / LinuxDo 品牌文件。
- 继续沿用服务端对 `/admin` 与 `/console` 的既有鉴权入口；PWA 不改变认证契约。
- 页面离线失败语义优先复用现有 unavailable/error surface，不引入离线成功假象。
- 为避免 public root service worker 抢占已安装 admin app 的离线入口，admin 入口在运行时归一到 `/admin/`，并让 admin manifest/scope 与 SW 都锁定 `/admin/`。
- public/admin service worker 安装阶段只负责 precache，不主动 `skipWaiting()`；用户确认更新后，页面向 waiting worker 发送 `TAVILY_HIKARI_ACTIVATE_UPDATE` 激活消息。
- `/api/version.frontend` 变化只触发 `registration.update()`，不直接展示可更新提示；提示状态以 service worker 的 installing/ready 生命周期为准。
- 更新提示由共享 runtime/hook 与 `UpdateAvailableBanner` 承载，覆盖 public、console、login、registration-paused 与 admin app shell。
- 用户触发激活后以 `controllerchange` 或 waiting worker 的 `activated` 状态确认成功；后者使用单次 reload guard 兼容浏览器漏发当前页接管事件的情况。
- 激活请求以 10 秒 watchdog 收口；超时、worker `redundant` 或激活消息发送异常都会进入 `activation-failed`，退出 loading 并允许用户重试或暂不提醒。
- 首次安装与版本升级以当前 registration 自身是否已有 active worker 区分；public 根作用域 controller 不再让 admin 首装误报更新，admin waiting worker 会静默激活并在下一次 admin 导航接管。

## 待完成项

- Safari / iOS 安装与离线重开手工验证记录。

## 验证记录

- `cd web && bun run build`
- `cd web && bun test src/pwa/assetGraph.test.ts`
- `cd web && bun test`
- `cd web && bun run build-storybook`
- `cd web && bun run test:e2e:pwa-offline`
- `cargo test`
- `cd docs-site && bun run build`
- 2026-07-08:
  - `cd web && bun test`
  - `cd web && bun run build`
  - `cd web && bun run build-storybook`
  - `cd web && bun run test:e2e:pwa-offline`
- 2026-07-11:
  - `cd web && bun test src/pwa/runtime.test.ts src/components/UpdateAvailableBanner.stories.test.tsx`
  - `cd web && bun run build`
  - `cd web && bun run test:e2e:pwa-offline`
  - Chromium E2E 以同源临时静态目录模拟 release A/B，验证 public 更新接管、reload 与 admin 跨 scope 首装。

## 已实现内容

- Vite build 现在输出 `.vite/manifest.json`，随后由 `web/scripts/generate_pwa_assets.py` 生成：
  - `manifest.webmanifest`
  - `manifest-admin.webmanifest`
  - `sw-public.js`
  - `sw-admin.js`
  - `pwa/public-*.png`
  - `pwa/admin-*.png`
  - `pwa/asset-graphs.json`
- 公共入口 `/`、`/console`、`/login`、`/registration-paused` 现在注册 public service worker。
- 管理员入口 `/admin/**` 现在只注册 admin service worker，并在运行时将 `/admin` 归一到 `/admin/`。
- Rust 静态托管新增：
  - `.webmanifest` content-type
  - `/manifest.webmanifest`
  - `/manifest-admin.webmanifest`
  - `/sw-public.js`
  - `/sw-admin.js`
  - `/pwa/*`
- 主可见界面当前已提供统一离线 banner：
  - `PublicHome`
  - `UserConsole`
  - `AdminDashboardRuntime`
  - `AdminLogin`
- `web/src/api/runtime.ts` 统一将浏览器裸网络失败归一为离线错误消息，减少 `Failed to fetch` 直出。
- Relay Mesh 品牌接入包括：
  - `web/scripts/generate_relay_mesh_brand_assets.py` 基于批准稿导出透明底 `lockup / mark / icon` 资产，以及 light/dark/mono 变体与 favicon/touch icon
  - `web/scripts/generate_pwa_assets.py` 从 `relay-mesh-icon-light.png` / `relay-mesh-icon-dark.png` 导出 public/admin 两套全尺寸 PWA PNG、maskable 图标、touch icon 与 manifest 主题字段
  - `BrandLockup` 组件统一 public home、console header、admin shell、login、registration-paused 与 404 fallback 的显式品牌位，并按主题切换 lockup 亮/暗版
  - `docs-site/rspress.config.ts` 与 `docs-site/docs/public/*` 接入同一套文档站品牌入口，并补上主题感知 favicon
  - 2026-06-27 follow-up 将 public/docs-site 品牌导出物迁到各自 `public/assets/` 目录，HTML shell、组件引用与 Rust 静态合同统一改为 `/assets/*`
- Chromium 离线 proof 已覆盖：
  - 公共首页离线壳可打开，并显示 `Offline shell loaded`
  - 用户控制台离线壳可打开，并显示 `Console structure is available`
  - 管理员后台离线壳可打开，并显示 `Admin shell loaded offline`
  - 公共 identity 离线访问 `/admin` 不会命中 cached admin shell

## 视觉证据

- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/public-offline-shell.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/console-offline-shell.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/admin-offline-shell.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/offline-banner-web-off.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-public-home.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-console-header.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-shell.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-login.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-registration-paused.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-docs-site.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/relay-mesh-pwa-icons.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-ready-storybook.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-installing-storybook.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-dark-ready-storybook.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-activation-failed-storybook.png`
- `docs/specs/2br7z-web-pwa-split-identities-offline-shells/assets/update-banner-activation-failed-dark-storybook.png`

## 后续微调

- 2026-06-24: 统一离线提示 banner 的图标从 `mdi:earth-off` 微调为 `mdi:web-off`，以匹配“经纬线地球 + 无网络斜杠”的语义预期。
- 2026-06-25: 品牌层切换到经批准的 Relay Mesh lockup/icon 资产，并复用既有 public/admin 双身份 PWA 产线导出所有安装资产。
- 2026-06-25: 修正 `web/package.json` 中 `test:e2e:pwa-offline` 的仓库相对路径，恢复按命令名直接执行的离线 PWA E2E 验证链。
- 2026-06-25: 品牌导出链追加 light/dark/mono 变体、主题感知 favicon 与 `64..1024 + maskable` 全尺寸 PWA icon 覆盖。
- 2026-06-27: 品牌公开路径从根路径裸文件收敛到 `/assets/*`，并补齐 `/assets/* + /favicon.svg` 的服务合同测试。
- 2026-07-08: 将更新检测从 PublicHome 局部版本比较升级为共享 PWA update runtime；新 worker precache 完成后进入 waiting，用户确认时才激活并 reload。

## 已知未完成验证

- Safari / iOS 的安装与离线重开未在当前自动化环境中执行，需后续手工补验并把结论回写到 spec。
