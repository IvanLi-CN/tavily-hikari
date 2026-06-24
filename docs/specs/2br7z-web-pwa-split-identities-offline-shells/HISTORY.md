# Web PWA 双身份离线壳与管理员缓存预算控制（#2br7z）历史

- 2026-06-24: 创建 follow-up spec，冻结 public/admin 双 PWA identity、离线壳页和管理员缓存预算控制边界。
- 2026-06-24: 落地双 PWA 生成产线、前端 service worker 注册、Rust `.webmanifest`/`sw-*` 静态托管，以及 public / console / admin / login 的离线错误提示第一版。
- 2026-06-24: 跑通 Chromium 离线 E2E 与完整 `cargo test`，补齐 public / console / admin 三类离线壳视觉证据，并确认普通公共身份离线访问 `/admin` 不会命中 admin 壳缓存。
- 2026-06-24: 根据视觉反馈将统一离线提示 icon 从 `mdi:earth-off` 调整为 `mdi:web-off`，并补充 banner 级视觉证据。
