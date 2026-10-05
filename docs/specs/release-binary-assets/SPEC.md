# Release：裸机二进制资产分发（#xxgfb）

## Context and Scope

- Context: 发布产品 SemVer 必须与 Web、Rust 主服务及容器镜像身份一致，且容器更新应复用内容未变的 OCI 文件系统层。
- In scope: 发布 Web/Rust 版本来源、Docker payload 归一化、镜像分层与 `/api/version`、`/version.json` 兼容行为。
- Out of scope: 生产部署、发布 GHCR 镜像、修改 release intent 路由及合并维护程序层。

## 背景 / 问题陈述

- 现有 release workflow 已能发布 GHCR `linux/amd64` + `linux/arm64` 镜像，但裸机部署仍需要额外处理容器运行时或手工拼装二进制与 Web 静态资源。
- `tavily-hikari` 的 Rust 服务可以直接作为单进程运行；发布链路应提供可直接下载、校验、解压和启动的 Linux 二进制资产。
- Web SPA 资产必须随 release binary 可用，否则裸机用户还需要单独同步 `web/dist`，发布体验会与容器镜像不一致。

## 目标 / 非目标

### Goals

- stable / rc release 均在 GitHub Release 中发布 `linux/amd64` 与 `linux/arm64` 的 native `tar.gz` 二进制资产。
- stable / rc release 额外并行发布 `linux/amd64-portable` 与 `linux/arm64-portable` 的 portable `tar.gz` 二进制资产，面向 old-Linux / 无宿主机 OpenSSL/SQLite 运行时依赖的裸机部署。
- 每个二进制资产同时发布 `.sha256` 校验文件。
- release binary 内嵌构建时的 `web/dist`，即使运行时没有外部静态目录，也能服务 `/`、`/admin`、`/console`、`/version.json` 与公共图标资源。
- portable binary 必须保持单文件分发语义，不额外要求宿主机提供 `glibc`、`OpenSSL` 或 `libsqlite3` 运行时库。
- 保留 `--static-dir` / `WEB_STATIC_DIR` 外部静态目录覆盖，且外部目录优先于内嵌资产。
- 继续保留 GHCR 镜像发布路径，不用 binary 替代镜像。
- release workflow 在上传 GitHub Release 前，对打包后的 binary 做本机 smoke，阻断不可用资产发布。
- release workflow 内部的前端 `web/dist` 只构建一次，并通过 release-local artifact 复用给 Docker 与 binary 发布 job。
- Docker 镜像必须使用已解析的 tag+digest 基础镜像；最终镜像中的程序和静态资产都须先将时间戳固定到 `SOURCE_DATE_EPOCH=0`，并归一化 owner/mode。每个 runtime payload 层在写入后，还须在同一层中复位目标父目录及构建期间会变化的 `/etc`、`/tmp` 目录 mtime。后续静态资源层只能复位自身目标目录，不得递归触碰已由先前层写入的应用文件。
- 产品发布 SemVer 必须由同一发布输入编入 Tavily Hikari 主服务二进制和真实前端 JavaScript 应用包；两个 service worker 使用该版本作为对应 PWA identity 的缓存版本。
- `APP_EFFECTIVE_VERSION` 是 Docker/Rust 构建输入，不写入运行时环境；OCI `org.opencontainers.image.version` label、主服务二进制、前端包和发布 workflow 使用同一产品 SemVer。
- Docker 最终镜像不得包含静态 `/srv/app/web/version.json` 或只承载版本元数据的文件系统层。HTTP `/version.json` 保留 `{ "version": "..." }` 兼容响应，并在无外部静态覆盖时由服务端动态生成；`--static-dir` / `WEB_STATIC_DIR` 中显式提供的 `version.json` 仍可覆盖 `/api/version.frontend` 与该 HTTP 响应。
- 镜像将主服务二进制、十个维护二进制与前端应用包保持为独立可观察的层。相同源码与发布版本只改变构建输入 mtime 时，每个架构的文件系统层 digest 都必须相同；仅改变合成测试 SemVer 时，只有主服务二进制层和包含真实 JavaScript 的前端应用包层变化，十个维护二进制层与归一化静态层保持不变。
- 同源码、不同 SemVer 的 A/B 是包装与 OCI 缓存合同测试，不证明生产历史中存在“只改版本号”的发布。
- Docker 构建上下文采用严格 allowlist，必须排除环境文件、数据库与依赖目录；Docker Dependabot 每周检查基础镜像更新。

### Non-goals

- 不把 `xray` 一起打包进本次二进制资产。
- 不改变数据库、API、MCP、计费或认证业务语义。
- 不改变现有 GHCR tag、manifest 与 release intent 语义。
- 不新增 Windows 或 macOS 二进制资产。
- 不废弃现有 native Linux binary 资产；portable 资产是并行新增，不替换老资产。

## 范围（Scope）

### In scope

- `.github/workflows/release.yml`
- `.github/workflows/ci.yml`
- `Dockerfile`
- `build.rs`
- `Cargo.toml` / `Cargo.lock`
- `src/web_assets.rs`
- `src/server/spa.rs`
- `src/server/serve.rs`
- `src/server/handlers/admin_resources/versions_and_proxy_helpers.rs`
- `web/vite.config.ts`
- `web/scripts/generate_pwa_assets.py`
- `web/src/version.ts`
- release / install documentation
- embedded asset HTTP contract tests

### Out of scope

- 101 部署 rollout
- 改变容器对外服务接口与职责
- Web UI 视觉或交互设计变更

## 验收标准（Acceptance Criteria）

- Given release workflow 进入发布阶段
  When `binary-native` 与 `binary-portable` jobs 完成
  Then GitHub Release 必须同时包含 `tavily-hikari-<tag>-linux-amd64.tar.gz`、`tavily-hikari-<tag>-linux-amd64.tar.gz.sha256`、`tavily-hikari-<tag>-linux-arm64.tar.gz`、`tavily-hikari-<tag>-linux-arm64.tar.gz.sha256`、`tavily-hikari-<tag>-linux-amd64-portable.tar.gz`、`tavily-hikari-<tag>-linux-amd64-portable.tar.gz.sha256`、`tavily-hikari-<tag>-linux-arm64-portable.tar.gz`、`tavily-hikari-<tag>-linux-arm64-portable.tar.gz.sha256`。
- Given release workflow 通过 `workflow_dispatch(head_sha=...)` 回填一个 pre-portable 迁移之前的历史提交
  When 当前 workflow checkout 到目标源码树并检测其 release contract
  Then portable binary job 必须被跳过，GitHub Release 继续只发布该历史提交原本支持的 native binary 资产，而不是因为当前主干 workflow 新增 portable job 而回填失败。
- Given release workflow 需要同时构建 Docker 镜像与 binary 资产
  When `web-assets` job 成功完成
  Then `docker-native` 与 `binary-native` 必须下载同一个 `release-web-dist` artifact，而不是各自重复执行 Bun 安装与前端构建。
- Given 二进制资产被解压到无 `web/dist` 的裸机目录
  When 使用 `--bind`、`--port`、`--db-path` 启动服务
  Then `/health` 返回 200，`/`、`/admin`、`/console` 能返回 HTML，`/version.json` 返回版本 JSON，图标资源可访问。
- Given portable 二进制资产被解压到 old-Linux 风格宿主机
  When 用 `ldd` 或等价方式检查二进制依赖
  Then 不得再暴露对宿主机 `glibc`、`OpenSSL`、`libsqlite3` 的运行时依赖。
- Given 运行时指定了有效外部静态目录
  When 该目录中存在目标文件
  Then 服务优先返回外部静态目录内容，内嵌资产只作为兜底。
- Given 任一架构 binary smoke 失败
  When release workflow 进入 GitHub Release job 前
  Then GitHub Release 资产上传必须被阻断。
- Given 相同源码和发布版本的两次构建具有不同输入文件 mtime
  When 在 Docker-enabled Linux VM 分别构建 `linux/amd64` 镜像
  Then 两次构建的所有文件系统层 digest 必须完全相同。
- Given 相同源码只改变合成测试 SemVer
  When 在 Docker-enabled Linux VM 使用相应版本构建 `linux/amd64` 镜像 A/B
  Then 只有主服务二进制层和包含真实 JavaScript 的前端应用包层变化；十个维护二进制层及归一化静态层相同，不存在版本元数据专层，OCI label、CLI `--version`、`/api/version` 与动态 `/version.json` 各自匹配该构建的 SemVer。
- Given production `web/dist` 被打包进 Docker 镜像
  When 检查镜像文件与运行时配置
  Then 不存在 `/srv/app/web/version.json`，`Config.Env` 不含 `APP_EFFECTIVE_VERSION`，且 `org.opencontainers.image.version` 等于产品发布 SemVer。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo check --locked --all-targets --all-features`
- `cd web && bun install --frozen-lockfile && bun run build`
- Targeted contract tests for embedded public/admin assets and existing console route compatibility.
- `scripts/check-version-layer-reuse.sh` 必须执行 mtime 重复构建与合成 SemVer A/B，逐架构核对 OCI 层 digest、变化层的压缩字节数、版本接口/label、无静态版本文件及严格上下文审计；基础镜像均使用 tag+digest，`.github/dependabot.yml` 每周更新 Docker ecosystem。

## 风险 / 假设

- 假设 GitHub-hosted `ubuntu-24.04` 与 `ubuntu-24.04-arm` runners 均可用，并能通过 Zig + musl 产出 portable binary。
- 风险：构建时没有 `web/dist` 时，binary 将不含内嵌资源；release workflow 必须先构建 Web 资产再构建 release binary。
- 假设 release-local artifact 复用继续沿用 `web/dist` 目录合同，因此 Dockerfile 与 build script 无需改动路径语义。
- 风险：若依赖树回退到 `native-tls` 或 `sqlite-unbundled`，portable 资产会重新引入宿主机动态库依赖；workflow 必须显式检查打包产物链接面。

## Requirements

### REQ-REL-VERSION-EMBEDDING

- 产品发布 SemVer MUST 编译进 Tavily Hikari 主服务二进制与真实前端 JavaScript 应用包；服务 CLI 的 `--version` 与后端版本接口 MUST 报告该 SemVer；两个 service worker MUST 使用同一 SemVer；`APP_EFFECTIVE_VERSION` MUST NOT 出现在镜像运行时环境中。

### REQ-REL-MTIME-NORMALIZATION

- Docker MUST 将 payload 的 mtime 归一为 `SOURCE_DATE_EPOCH=0`、owner 归一为 `0:0`，并将普通文件、目录及可执行文件权限分别归一为 `0644`、`0755`、`0755`。每个 runtime payload 层写入文件后 MUST 在同一层内将受影响的目标父目录以及 `/etc`、`/tmp` 的 mtime 复位到该 epoch。后续静态资源层 MUST NOT 递归改写先前应用层中的版本化文件；只可归一化本层新写入的 payload 和受影响目录。

### REQ-REL-LAYER-BOUNDARIES

- 生产镜像 MUST NOT 包含静态版本元数据专层或静态 `/srv/app/web/version.json`；维护程序 MUST 独立于产品 SemVer 构建，前端应用层 MUST 包含带版本的真实 JavaScript。

### REQ-REL-VERSION-COMPATIBILITY

- `/api/version` 与动态 `/version.json` MUST 报告一致的产品版本并保持既有字段和 JSON 形状；显式外部静态目录中的 `version.json` MUST 继续覆盖前端版本。

## Verification

### VER-REL-WEB-PACKAGE

- Method: build production `web/dist` with a release SemVer and inspect generated JavaScript, workers, HTML and static files.
- covers: `REQ-REL-VERSION-EMBEDDING`, `REQ-REL-LAYER-BOUNDARIES`
- Pass condition: JavaScript and both workers contain the release SemVer; no HTML version meta or static `version.json` exists.

### VER-REL-OCI-MTIME

- Method: run `PLATFORMS=linux/amd64 scripts/check-version-layer-reuse.sh` in a Docker-enabled Linux VM.
- covers: `REQ-REL-MTIME-NORMALIZATION`
- Pass condition: same-source/same-version AMD64 images built from distinct input mtimes have identical RootFS layer digests.

### VER-REL-OCI-SEMVERS

- Method: synthetic same-source SemVer A/B packaging comparison in a Docker-enabled Linux VM.
- covers: `REQ-REL-LAYER-BOUNDARIES`
- Pass condition: only the main service binary layer and frontend application layer change; each contains its real payload, no metadata-only layer exists, and compressed changed-layer byte counts are recorded.

### VER-REL-VERSION-ROUTES

- Method: `cargo test --locked --bin tavily-hikari version_detection`, `cargo test --locked --bin tavily-hikari cli_version_uses_the_embedded_product_version`, `cargo test --locked --test server_http_contract embedded_public_assets_are_served_without_static_dir`, and the version-layer image HTTP smoke checks.
- covers: `REQ-REL-VERSION-COMPATIBILITY`
- Pass condition: packaged backend API, CLI `--version`, frontend version, and dynamic `/version.json` agree for release builds; an explicit external static override remains supported.

## Related ADRs

- [ADR 0007: Release Version Embedding and Image Layer Reuse](../../adr/0007-release-version-embedding-and-image-layer-reuse.md)
