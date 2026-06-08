# Implementation：Release 裸机二进制资产分发（#xxgfb）

## 当前实现

- `build.rs` 在 `web/dist` 存在时复制静态资源到 `OUT_DIR`，生成 `embedded_web_assets.rs`，并通过 `include_bytes!` 内嵌每个资源。
- `src/web_assets.rs` 暴露内嵌资源查询入口。
- SPA 服务路径改为统一从外部静态目录优先读取，找不到时回落到内嵌资源；`/assets/*`、`/favicon.svg`、`/linuxdo-logo.svg`、`/version.json` 与 HTML 页面共享这套读取逻辑。
- 版本检测同样保持外部静态目录优先，避免 `--static-dir` 覆盖部署时版本信息与实际服务的前端不一致。
- `Dockerfile` 在 builder 阶段复制 `build.rs`，保证新增 Cargo build script 后容器构建路径仍可用；容器运行时继续通过 `WEB_STATIC_DIR=/srv/app/web` 使用镜像内静态目录。
- release workflow 先在单独的 `web-assets` job 内构建一次 `web/dist` 并上传 `release-web-dist` artifact，随后 `docker-native` 与 `binary-native` 都只下载该 artifact 复用，不再各自重复 Bun 安装与前端构建。
- `binary-native` matrix 继续在 `ubuntu-24.04` 与 `ubuntu-24.04-arm` 上构建 release binary、打包 `tar.gz`、生成 `.sha256` 并 smoke 解包后的 binary。
- GitHub Release job 下载 binary artifacts 后用 `gh release upload --clobber --repo "${GITHUB_REPOSITORY}"` 上传资产；该 job 没有 checkout，不能依赖本地 `.git` 推断仓库。PR release comment 列出 binary
  资产名称。
- CI workflow 增加 embedded asset contract coverage，避免无外部静态目录的 binary 路径回归。

## 验证

- `cargo test --locked --all-features console_route_serves_spa_when_user_oauth_is_disabled -- --test-threads=1`
- `cargo test --locked --all-features console_deep_link_route_serves_spa_when_user_oauth_is_disabled -- --test-threads=1`
- `cargo test --locked --all-features embedded_public_assets_are_served_without_static_dir -- --test-threads=1`
- `cargo test --locked --all-features embedded_admin_page_is_served_when_dev_open_admin_is_enabled -- --test-threads=1`
- `cargo test version_detection_tests::static_dir_version_overrides_embedded_version`
- `cargo clippy -- -D warnings`
- Packaged release binary smoke: unpacked binary served `/health`, `/`, `/admin`, `/console`, `/version.json`, `/favicon.svg`, and `/linuxdo-logo.svg` without external static dir.
- PR #312 checks on `1fd029f`: `Release intent label gate`, `Lint & Checks`, `Frontend Checks`, `Backend Tests`, `Build (Release)`, `Compose Smoke (ForwardAuth + Caddy)`, Docs Pages checks.
- Codex review-loop on `1fd029f` found no remaining behavior or release-flow defects.

## 后续状态

- PR #312 已达到 merge-ready 状态。
