# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）实现记录

## 当前实现状态

- 状态：已完成（含 Relay Mesh 品牌接入、可恢复 PWA 更新提示与安装图标更新交付修复；待 Safari / iOS 手工补验）
- 分支：`th/fix-pwa-admin-icon-update`

## 实现决策

- 采用现有 Vite multipage 构建，新增 build manifest 输出与 post-build 脚本。
- 通过生成脚本构造 public/admin 两套 asset graph、manifest、service worker 与图标，不引入单 manifest 注入式 PWA 插件。
- manifest 继续作为支持平台的安装元数据来源：public/admin 分别固定 `id=/` 与 `id=/admin/`，HTML 只声明匹配 manifest，移除会在 WebKit 中优先覆盖 manifest 的 `apple-touch-icon` link。
- PWA PNG 以最终字节的 12 位 SHA-256 摘要命名；manifest 与对应 worker 只引用自己的当前 URL。metadata 资源要求重新验证，哈希图标使用 immutable 缓存；worker precache 使用 `cache: 'reload'`，页面注册使用 `updateViaCache: 'none'`。
- 品牌导出器按 mark 的可见 alpha 边界放置方形 launcher、maskable 与 mono 图标，避免批准稿透明画布 padding 造成前景偏移；CI 逐一检查 Web 与 docs-site 导出物的可见边界中心。
- 品牌资产采用单一矢量 lockup 母版：Roboto Condensed weight 400 的预实例化静态字体固定为 reviewed tagline outline 的来源；完整 SVG 保存该不可变 outline，并以两组路径哈希防止不同主机的字体栅格化或意外编辑改写母版。完整版保持既有 `1000 × 310` 横向轮廓，右侧主字标与副标语作为两行文字块共享 Relay Mesh mark 的光学中轴；`KEY POOL · BALANCE. ROUTE.` 保留字间点并使用一条连续渐变，而非分段色块或竖线。Web 与 docs-site 的完整/compact SVG 与 PNG 均从母版稳定导出，favicon、launcher icon 与 PWA identity 保持既有合同。
- `BrandLockup` 以 `full | compact | responsive` 三态统一公共首页、用户控制台、后台、登录、暂停注册和 404；`responsive` 以 `260px` 的实际容器宽度为门槛选择完整或 compact。Rspress `Layout.navTitle` 复用同一主题与容器查询合同，但作为导航 utility 位固定为 `180px`，因此稳定选择 compact 版本。
- 品牌导出链现在显式产出 lockup / mark / launcher icon 的 light、dark、mono 变体，并保留默认亮色别名文件给现有入口复用。
- owner-facing 品牌静态资源统一改由 `/assets/*` 合同暴露；根路径只保留 `/favicon.svg`、manifest 与 PWA 入口，不再公开 Relay Mesh / LinuxDo 品牌文件。
- 继续沿用服务端对 `/admin` 与 `/console` 的既有鉴权入口；PWA 不改变认证契约。
- 页面离线失败语义优先复用现有 unavailable/error surface，不引入离线成功假象。
- 为避免 public root service worker 抢占已安装 admin app 的离线入口，admin 入口在运行时归一到 `/admin/`，并让 admin manifest/scope 与 SW 都锁定 `/admin/`。
- public/admin service worker 安装阶段只负责 precache，不主动 `skipWaiting()`；用户确认更新后，页面向 waiting worker 发送 `TAVILY_HIKARI_ACTIVATE_UPDATE` 激活消息。
- `/api/version.frontend` 变化只触发 `registration.update()`，不直接展示可更新提示；安装/缓存中的中间态继续静默，只有 waiting worker 已 ready 或用户触发后的失败态才展示 banner。
- 更新横幅的“当前版本”现在由当前 HTML shell 的 `tavily-hikari-build-version` meta 标记提供；“目标版本”会在初始版本探测、waiting worker ready、以及失败重试态重新向 `/api/version` 校准，避免回退到 `latest` 或把服务器版本误认成当前页版本。
- `write-version.mjs` 支持 `WEB_DIST_DIR`，在五个 HTML shell 中注入 HTML 转义后的版本、写入 `version.json`；PWA 生成器校验该 JSON 并把版本纳入两个 worker 的 cache identity，不改写 hashed assets、asset graph 或 web manifest。
- Chromium 离线 E2E 直接断言初始 release shell 的 HTML meta 版本，并在切换到新 worker 前验证旧 shell 离线可用，覆盖纯版本更新的真实缓存生命周期。
- 更新提示由共享 runtime/hook 与 `UpdateAvailableBanner` 承载，覆盖 public、console、login、registration-paused 与 admin app shell。
- 管理员登录页将更新提示提升为页头后的页面级状态：桌面宽度独立于 `36rem` 登录表单，移动端保持操作按钮同行且无横向溢出；提示标题、版本信息和操作按钮按阅读优先级分层。
- 用户触发激活后以 `controllerchange` 或 waiting worker 的 `activated` 状态确认成功；后者使用单次 reload guard 兼容浏览器漏发当前页接管事件的情况。
- 激活请求以 10 秒 watchdog 收口；超时、worker `redundant` 或激活消息发送异常都会进入 `activation-failed`，退出 loading 并允许用户重试或暂不提醒。
- 如果用户暂不点击“立即刷新”，runtime 会在 `pagehide` 时静默向 waiting worker 发送激活消息，让下一次导航直接进入新版本，而不在当前页强制打断操作。
- 首次安装与版本升级以当前 registration 自身是否已有 active worker 区分；public 根作用域 controller 不再让 admin 首装误报更新，admin waiting worker 会静默激活并在下一次 admin 导航接管。
- public/admin worker 不接管 `/api/*`、`/mcp`、SSE、认证与写请求等 network-only 流量，避免长连接 FetchEvent 阻塞旧 worker 退场；未预缓存的普通同源运行时资源仍在网络拒绝时返回 `503 Service Unavailable`。
- worker activate 事件只清理旧 cache，不调用 `clients.claim()`；首次安装在下一次同 scope 导航接管，版本更新则由 runtime 在目标 worker 到达 `activated` 后 reload。
- Android Chrome/WebAPK 与 Chromium desktop 按稳定 manifest `id` 处理同一安装的后续 metadata 更新；既有 iOS/iPadOS Web Clip 和不支持 manifest metadata 同步的浏览器无法由网站强制迁移图标，该限制不以“重新安装”作为常规用户流程解决。

## 待完成项

- Safari / iOS 安装与离线重开手工验证记录。

## 验证记录

- 2026-07-31 品牌可读性修复：
  - 前置依赖：Python 侧执行 `python3 -m pip install --user Pillow fonttools numpy`；安装
    `rsvg-convert` 与 `potrace`（macOS 使用 `brew install librsvg potrace`，Debian/Ubuntu 使用
    `sudo apt-get install librsvg2-bin potrace`）。
  - `python3 web/scripts/vectorize_relay_mesh_lockups.py`
  - `python3 web/scripts/verify_relay_mesh_lockup_geometry.py`
  - `python3 web/scripts/generate_relay_mesh_brand_assets.py`
  - `cd web && bun test`
  - `cd web && bun run build`
  - `cd web && bun run build-storybook`
  - `cd docs-site && bun run build`

- `cd web && bun run build`
- `cd web && bun test src/pwa/assetGraph.test.ts`
- `cd web && bun test`
- `cd web && bun run build-storybook`
- `cd web && bun run test:e2e:pwa-offline`
- `cargo test`
- `cd docs-site && bun run build`
- 版本 A/B 构建与 Docker 层复用门禁：仅版本 JSON、五个 HTML shell 与两个 worker 可变，稳定 Web 层与镜像 RootFS 前缀必须保持一致。
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
- 2026-07-12:
  - `cd web && bun run build && bun test ./src/pwa/assetGraph.test.ts`
  - 生成的 public/admin worker 对自身拥有的未预缓存运行时资源提供 `503` 网络失败响应。
- 2026-07-14:
  - `cd web && bun test src/pwa/runtime.test.ts`
  - `cd web && bun test src/pwa/assetGraph.test.ts`
  - `cd web && bun run test:e2e:pwa-offline`
  - Chromium 两版本 E2E 直接断言更新必须 reload；若进入 `activation-failed` 则立即失败。
- 2026-07-15:
  - `cd web && bun test src/hooks/useUpdateAvailable.test.tsx`
  - `cd web && bun test src/pwa/runtime.test.ts src/components/UpdateAvailableBanner.stories.test.tsx`
  - `cd web && bun run build`

## 已实现内容

- Vite build 现在输出 `.vite/manifest.json`，随后由 `web/scripts/generate_pwa_assets.py` 生成：
  - `manifest.webmanifest`
  - `manifest-admin.webmanifest`
  - `sw-public.js`
  - `sw-admin.js`
  - `pwa/public-*-<content-hash>.png`
  - `pwa/admin-*-<content-hash>.png`
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
  - `web/scripts/generate_pwa_assets.py` 将每个图标的最终 PNG 内容摘要写入 URL，并将相应 manifest 纳入对应 service worker precache；public/admin 产物不互相引用
  - `BrandLockup` 组件统一 public home、console header、admin shell、login、registration-paused 与 404 fallback 的显式品牌位，并按主题切换 lockup 亮/暗版
  - `docs-site/rspress.config.ts` 与 `docs-site/docs/public/*` 接入同一套文档站品牌入口，并补上主题感知 favicon
  - 2026-06-27 follow-up 将 public/docs-site 品牌导出物迁到各自 `public/assets/` 目录，HTML shell、组件引用与 Rust 静态合同统一改为 `/assets/*`
- Chromium 离线 proof 已覆盖：
  - 公共首页离线壳可打开，并显示 `Offline shell loaded`
  - 用户控制台离线壳可打开，并显示 `Console structure is available`
  - 管理员后台离线壳可打开，并显示 `Admin shell loaded offline`
  - 公共 identity 离线访问 `/admin` 不会命中 cached admin shell

## 视觉证据

- `docs/specs/web-pwa-split-identities-offline-shells/assets/public-offline-shell.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/console-offline-shell.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/admin-offline-shell.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/offline-banner-web-off.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-public-home.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-console-header.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-shell.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-admin-login.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-registration-paused.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-docs-site.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/relay-mesh-pwa-icons.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-ready-storybook.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-installing-storybook.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-dark-ready-storybook.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-activation-failed-storybook.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-activation-failed-dark-storybook.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-login-header-desktop.png`
- `docs/specs/web-pwa-split-identities-offline-shells/assets/update-banner-login-header-mobile.png`

## 后续微调

- 2026-06-24: 统一离线提示 banner 的图标从 `mdi:earth-off` 微调为 `mdi:web-off`，以匹配“经纬线地球 + 无网络斜杠”的语义预期。
- 2026-06-25: 品牌层切换到经批准的 Relay Mesh lockup/icon 资产，并复用既有 public/admin 双身份 PWA 产线导出所有安装资产。
- 2026-06-25: 修正 `web/package.json` 中 `test:e2e:pwa-offline` 的仓库相对路径，恢复按命令名直接执行的离线 PWA E2E 验证链。
- 2026-06-25: 品牌导出链追加 light/dark/mono 变体、主题感知 favicon 与 `64..1024 + maskable` 全尺寸 PWA icon 覆盖。
- 2026-06-27: 品牌公开路径从根路径裸文件收敛到 `/assets/*`，并补齐 `/assets/* + /favicon.svg` 的服务合同测试。
- 2026-07-08: 将更新检测从 PublicHome 局部版本比较升级为共享 PWA update runtime；新 worker precache 完成后进入 waiting，用户确认时才激活并 reload。
- 2026-07-31: 管理员登录页更新提示移至全局页头下方，采用页头级宽度并修正移动端按钮同行、信息层级和横向溢出。
- 2026-09-01: 修复安装图标更新交付链：manifest 增加稳定 identity `id`，PNG URL 使用内容哈希，HTML 移除 legacy `apple-touch-icon` 覆盖，metadata/图标缓存策略分离，并让对应 service worker 重新验证自己的 manifest 与图标。

## 已知未完成验证

- Safari / iOS 的安装与离线重开未在当前自动化环境中执行，需后续手工补验并把结论回写到 spec。

## 状态

- Status: 已完成（快车道）
- Created: 2026-06-24
- Last: 2026-09-01

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 双 manifest / 双 service worker 生成管线落地
- [x] M2: 入口页注册、HTML 合同与 Rust 静态托管落地
- [x] M3: public/admin cache 边界与 navigation fallback 落地
- [x] M4: 公共页、控制台、后台离线错误态收口
- [x] M5: Storybook、测试、浏览器离线验证、Relay Mesh 品牌接入与视觉证据完成
- [x] M6: 稳定 manifest identity、哈希安装图标、metadata 缓存与双 worker 更新交付链落地
