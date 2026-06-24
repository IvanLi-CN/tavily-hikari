# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）实现记录

## 当前实现状态

- 状态：已完成（待 Safari / iOS 手工补验）
- 分支：`th/2br7z-web-pwa-split-identities-offline-shells`

## 实现决策

- 采用现有 Vite multipage 构建，新增 build manifest 输出与 post-build 脚本。
- 通过生成脚本构造 public/admin 两套 asset graph、manifest、service worker 与图标，不引入单 manifest 注入式 PWA 插件。
- 继续沿用服务端对 `/admin` 与 `/console` 的既有鉴权入口；PWA 不改变认证契约。
- 页面离线失败语义优先复用现有 unavailable/error surface，不引入离线成功假象。
- 为避免 public root service worker 抢占已安装 admin app 的离线入口，admin 入口在运行时归一到 `/admin/`，并让 admin manifest/scope 与 SW 都锁定 `/admin/`。

## 待完成项

- Safari / iOS 安装与离线重开手工验证记录。

## 验证记录

- `cd web && bun run build`
- `cd web && bun test src/pwa/assetGraph.test.ts`
- `cd web && bun test`
- `cd web && bun run build-storybook`
- `bun run test:e2e:pwa-offline`
- `cargo test`

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

## 后续微调

- 2026-06-24: 统一离线提示 banner 的图标从 `mdi:earth-off` 微调为 `mdi:web-off`，以匹配“经纬线地球 + 无网络斜杠”的语义预期。

## 已知未完成验证

- Safari / iOS 的安装与离线重开未在当前自动化环境中执行，需后续手工补验并把结论回写到 spec。
