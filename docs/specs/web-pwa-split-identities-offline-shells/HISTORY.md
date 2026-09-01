# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）历史

- 2026-06-24: 创建 follow-up spec，冻结 public/admin 双 PWA identity、离线壳页和管理员缓存预算控制边界。
- 2026-06-24: 落地双 PWA 生成产线、前端 service worker 注册、Rust `.webmanifest`/`sw-*` 静态托管，以及 public / console / admin / login 的离线错误提示第一版。
- 2026-06-24: 跑通 Chromium 离线 E2E 与完整 `cargo test`，补齐 public / console / admin 三类离线壳视觉证据，并确认普通公共身份离线访问 `/admin` 不会命中 admin 壳缓存。
- 2026-06-24: 根据视觉反馈将统一离线提示 icon 从 `mdi:earth-off` 调整为 `mdi:web-off`，并补充 banner 级视觉证据。
- 2026-06-25: 基于最新 `origin/main` 同步后，将 Relay Mesh 品牌接入既有双身份 PWA 产线，更新 favicon、touch icon、public/admin PWA 图标、docs-site 品牌入口与主要 Web 品牌位。
- 2026-06-27: 将 Relay Mesh、LinuxDo 与 favicon 位图依赖统一迁到 `/assets/*`，删除根路径品牌资源公开合同并补齐静态服务回归覆盖。
- 2026-07-08: 更新生命周期改为 precache 完成后 waiting，页面用共享 inline banner 提示用户确认激活，避免后端版本变化时误报资源已 ready。
- 2026-07-11: 修复用户确认更新后永久停在 `activating` 的状态机缺口。发送激活消息不再视为完成证据；runtime 现在等待 controller/worker 成功信号并以 watchdog、失败重试和单次 reload guard 收口，同时修正 public controller 干扰 admin 首装判断的问题。
- 2026-07-12: 尝试将 Service Worker 拥有请求的网络拒绝转换为 `503`，消除未处理的 `FetchEvent` promise rejection；后续真实更新复现进一步收窄了 network-only 请求的正确所有权边界。
- 2026-07-14: 真实 Chromium A/B 更新复现确认 network-only 长连接被 Service Worker `respondWith` 接管会阻塞旧 worker 退场，并使新 worker停在 `activating`。network-only 请求改由浏览器直接处理，activate 阶段移除 `clients.claim()`，首次安装在下一次导航接管，版本更新由 `activated` 后 reload 完成。
- 2026-07-15: 更新横幅改为只在 waiting worker ready 后通知用户，不再暴露 installing 中间态；当前/目标版本都改为具体版本号，且当前版本来自正在运行的 bundle。若用户暂不点击“立即刷新”，runtime 会在下一次刷新/离页前静默激活 waiting worker，使下次导航直接进入新版本。
- 2026-07-31: 管理员登录页将更新提示从凭据流程中移出，固定在全局页头之后、登录主内容之前；桌面采用页头级宽度，移动端保持操作按钮同行，并重新建立标题、版本信息与操作按钮的视觉层级。
- 2026-08-01: 完整 Relay Mesh lockup 将副标语固定为 `KEY POOL · BALANCE. ROUTE.` 的 weight 400 outline；右侧两行文字块与 Relay Mesh mark 共享光学中轴，字间点和单一连续渐变保留原始品牌语法。完整/compact 的选择改由 `260px` 容器宽度驱动，PWA 图标与 identity 不随文字可读性修复改变。
- 2026-08-01: 文档站导航被确认为 utility 位而非品牌主视觉；Rspress 的品牌容器固定为 `180px`，按共享容器合同显示 compact lockup，避免将完整两行标志压入导航栏。
- 2026-08-18: 前端版本身份从 Vite 编译期 define 收敛到 HTML shell meta、`version.json` 与版本化 service worker cache；纯版本发布只改变尾部动态产物，旧 shell 继续支持离线回退。
- 2026-09-01: 修复安装元数据与图标更新链：为 public/admin manifest 固定身份，移除会覆盖 manifest 的 legacy touch-icon HTML 声明，改用内容哈希图标 URL，并让静态缓存与 service worker 更新遵循可重新验证的 metadata / immutable asset 边界。
- 2026-09-01: 修正方形品牌图标按透明源画布居中导致的前景偏移；导出器改按可见 mark 边界居中，并加入 Web 与 docs-site 图标几何门禁。

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
- 2026-08-18: 补充版本 A/B 机械门禁，确认稳定 PWA 资产不随纯版本发布变化，且两个 worker 的 cache identity 随版本变化。
- 2026-08-18: 离线浏览器 E2E 增加 HTML meta 版本来源与旧 release shell 离线可用断言。
- 2026-09-01: 补齐 manifest identity、图标内容哈希、缓存策略与 service worker 安装预缓存回归；记录 Chromium 可迁移更新路径及既有 iOS/iPadOS Web Clip 无法强制迁移的兼容边界。

## Legacy Identity

- Legacy compatibility identity: `#2br7z`.
