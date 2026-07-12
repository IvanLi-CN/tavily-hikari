# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）历史

- 2026-06-24: 创建 follow-up spec，冻结 public/admin 双 PWA identity、离线壳页和管理员缓存预算控制边界。
- 2026-06-24: 落地双 PWA 生成产线、前端 service worker 注册、Rust `.webmanifest`/`sw-*` 静态托管，以及 public / console / admin / login 的离线错误提示第一版。
- 2026-06-24: 跑通 Chromium 离线 E2E 与完整 `cargo test`，补齐 public / console / admin 三类离线壳视觉证据，并确认普通公共身份离线访问 `/admin` 不会命中 admin 壳缓存。
- 2026-06-24: 根据视觉反馈将统一离线提示 icon 从 `mdi:earth-off` 调整为 `mdi:web-off`，并补充 banner 级视觉证据。
- 2026-06-25: 基于最新 `origin/main` 同步后，将 Relay Mesh 品牌接入既有双身份 PWA 产线，更新 favicon、touch icon、public/admin PWA 图标、docs-site 品牌入口与主要 Web 品牌位。
- 2026-06-27: 将 Relay Mesh、LinuxDo 与 favicon 位图依赖统一迁到 `/assets/*`，删除根路径品牌资源公开合同并补齐静态服务回归覆盖。
- 2026-07-08: 更新生命周期改为 precache 完成后 waiting，页面用共享 inline banner 提示用户确认激活，避免后端版本变化时误报资源已 ready。
- 2026-07-11: 修复用户确认更新后永久停在 `activating` 的状态机缺口。发送激活消息不再视为完成证据；runtime 现在等待 controller/worker 成功信号并以 watchdog、失败重试和单次 reload guard 收口，同时修正 public controller 干扰 admin 首装判断的问题。
- 2026-07-12: 修复生成 Service Worker 将网络拒绝直接传给 `respondWith` 的缺口。MCP 与未预缓存的同源请求现在在网络不可达时返回 `503`，不再产生未处理的 `FetchEvent` promise rejection。
