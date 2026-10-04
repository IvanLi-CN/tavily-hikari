# Implementation：Release 裸机二进制资产分发（#xxgfb）

## 当前实现

- `build.rs` 在 `web/dist` 存在时复制静态资源到 `OUT_DIR`，生成 `embedded_web_assets.rs`，并通过 `include_bytes!` 内嵌每个资源。
- `src/web_assets.rs` 暴露内嵌资源查询入口。
- SPA 服务路径改为统一从外部静态目录优先读取，找不到时回落到内嵌资源；`/assets/*`、`/favicon.svg`、`/version.json` 与 HTML 页面共享这套读取逻辑。
- 版本检测同样保持外部静态目录优先，避免 `--static-dir` 覆盖部署时版本信息与实际服务的前端不一致。
- `Dockerfile` 在 builder 阶段复制 `build.rs`，保证新增 Cargo build script 后容器构建路径仍可用；容器运行时继续通过 `WEB_STATIC_DIR=/srv/app/web` 使用镜像内静态目录。
- `Dockerfile` 固定 Rust、Debian 与 Xray 基础镜像的 tag+digest；十个维护程序无产品 SemVer 构建，主服务单独注入编译期 SemVer。所有最终 payload 先在中间阶段归一为 `SOURCE_DATE_EPOCH=0`、owner `0:0` 与规范权限，再从只读 BuildKit bind mount 写入各自最终层，并在同层复位目标目录 mtime。实测发现，即使来源文件已归一，最终 `COPY` 仍会更新目标父目录 mtime，导致内容不变的独立层 diffID 漂移。前端 JS、HTML shells、workers 和版本化 asset graph 合为真实应用层，图标、manifest/favicon、运行脚本及各维护二进制继续独立分组。
- `.dockerignore` 采用 Cargo 源码、两个 Docker 脚本与 `web/dist` 的严格 allowlist；`context-audit` target 枚举上下文并检查 `.env`、数据库与 `node_modules` 不会进入上下文，Dependabot 每周更新 Docker 基础镜像。
- 后端版本 helper 从主服务二进制编译期 `APP_EFFECTIVE_VERSION` 读取产品版本，不再使用运行时 ENV；`/api/version` 与动态 `/version.json` 使用同一产品版本，显式外部静态目录内的 `version.json` 仍可覆盖前端版本。OCI label 继续报告同一 SemVer，镜像 `Config.Env` 不含该变量。
- `scripts/check-version-layer-reuse.sh` 与 CI job 构建同版本/不同输入 mtime 及同源码/不同合成 SemVer 镜像，按架构检查 RootFS diffID、动态层归属、gzip 压缩字节、版本接口、OCI label、无静态版本 JSON 与上下文审计；合成 SemVer A/B 是包装合同测试，不代表生产历史中曾发生纯版本号发布。CI 继续上传 amd64 B 镜像供 Compose mock smoke 使用。
- release workflow 将 `org.opencontainers.image.version` 显式绑定到 `APP_EFFECTIVE_VERSION`，避免 metadata-action 的默认标签覆盖发布版本。
- release workflow 先在单独的 `web-assets` job 内构建一次 `web/dist` 并上传 `release-web-dist` artifact，随后 `docker-native` 与 `binary-native` 都只下载该 artifact 复用，不再各自重复 Bun 安装与前端构建。
- `binary-native` matrix 继续在 `ubuntu-24.04` 与 `ubuntu-24.04-arm` 上构建 release binary、打包 `tar.gz`、生成 `.sha256` 并 smoke 解包后的 binary。
- `reqwest` 改为按目标平台分流：glibc / native Linux 资产继续保留原本默认 `default-tls` 行为，避免现有发布因为 portable 需求而整体切换 TLS backend；只有 musl portable 构建改用 `rustls-tls-webpki-roots`，把 HTTPS trust roots 一并内嵌进 release artifact，同时只保留与 native 资产对齐的 `charset`、`http2`、`system-proxy`、`json`、`stream` 与 `socks` 能力，避免 portable 资产单独启用透明压缩解码后改变上游响应语义。
- `sqlx` 继续使用 `sqlite` 特性，自带 bundled `libsqlite3-sys` 静态链接路径，不再依赖宿主机 `libsqlite3` 运行时。
- release workflow 新增 `binary-portable` matrix：在 `ubuntu-24.04` 与 `ubuntu-24.04-arm` 上安装 Zig 与固定版本的 `cargo-zigbuild`，分别构建 `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl` 版本，并打包为 `*-portable.tar.gz` 与 `.sha256`。
- `prepare` job 额外从被 checkout 的目标源码树读取 release contract marker；只有目标树声明 `portable_release_contract=v1` 时才启用 `binary-portable` 与 portable 资产文案。这样当前 workflow 仍可用 `workflow_dispatch(head_sha=...)` 回填 pre-portable 历史提交，而不会把新 portable 构建强加到旧依赖树上。
- portable matrix 在 smoke 前额外执行链接面检查：同时读取 `file`、`readelf -d` 与 `readelf -l`，要求产物保持 static/static-pie、没有 `PT_INTERP`、没有 `DT_NEEDED`，并显式拒绝 `glibc` / `OpenSSL` / `libsqlite3` 运行时依赖，避免仅靠 `ldd` 文本匹配漏过 glibc 动态链接回归。
- 2026-06-27 的品牌资源 `/assets` 迁移后，release workflow 的 native/portable binary smoke 也同步改为探测 `/assets/linuxdo-logo.svg` 与 `/assets/relay-mesh-lockup-light.png`，不再保留失效的根路径 `/linuxdo-logo.svg` 断言；embedded HTTP 合同测试同步覆盖 PNG + SVG 品牌资产。
- GitHub Release job 下载 binary artifacts 后用 `gh release upload --clobber --repo "${GITHUB_REPOSITORY}"` 上传资产；该 job 没有 checkout，不能依赖本地 `.git` 推断仓库。GitHub Release 会包含 binary
  资产名称及新增的 portable 资产。
- CI workflow 增加 embedded asset contract coverage，避免无外部静态目录的 binary 路径回归。
- `scripts/check-version-layer-reuse.sh` 在 `codex-testbox` 的 amd64 OCI 验收通过：21 个 RootFS diffID 在同源码、同 SemVer、不同输入 mtime 构建间逐层一致；合成 SemVer A/B 只改变主服务层（gzip -1 估算 15,498,527 bytes）与前端应用层（7,431,177 bytes）。该 A/B 是包装合同验证，不证明生产历史曾只改版本号。当前 testbox BuildKit 仅提供 amd64 平台，arm64 验收仍待具备 arm64 执行能力的 runner。
- 逐层 amd64 RootFS diffID 与压缩字节明细：[oci-acceptance-amd64-335859fa.md](evidence/oci-acceptance-amd64-335859fa.md)。

## 验证

- `cargo test --locked --all-features console_route_serves_spa_when_user_oauth_is_disabled -- --test-threads=1`
- `cargo test --locked --all-features console_deep_link_route_serves_spa_when_user_oauth_is_disabled -- --test-threads=1`
- `cargo test --locked --all-features embedded_public_assets_are_served_without_static_dir -- --test-threads=1`
- `cargo test --locked --all-features embedded_admin_page_is_served_when_dev_open_admin_is_enabled -- --test-threads=1`
- `cargo test version_detection_tests::static_dir_version_overrides_embedded_version`
- `cargo clippy -- -D warnings`
- Packaged release binary smoke: unpacked binary served `/health`, `/`, `/admin`, `/console`, `/version.json`, `/favicon.svg`, and representative `/assets/*` brand assets without external static dir.
- Portable binary linkage gate: unpacked `*-portable` binary passes `ldd` / `file` style verification without `glibc` / `OpenSSL` / `libsqlite3` runtime dependencies.
- PR #312 checks on `1fd029f`: `Release intent label gate`, `Lint & Checks`, `Frontend Checks`, `Backend Tests`, `Build (Release)`, `Compose Smoke (ForwardAuth + Caddy)`, Docs Pages checks.
- Codex review-loop on `1fd029f` found no remaining behavior or release-flow defects.

## 后续状态

- PR #312 已达到 merge-ready 状态。

## 状态

- Status: active
- Created: 2026-06-04
- Last: 2026-06-04
