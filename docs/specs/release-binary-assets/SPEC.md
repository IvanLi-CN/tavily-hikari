# Release：裸机二进制资产分发（#xxgfb）

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
- Docker 镜像必须使用已解析的 tag+digest 基础镜像；稳定运行时层先复制 Xray、入口、维护二进制与稳定 Web 资产，最后以单一动态 COPY 放入版本 HTML shell、service worker 与 `version.json`。
- `APP_EFFECTIVE_VERSION` 只能作为运行时配置与 OCI 元数据输入，不能污染 Rust builder、稳定运行时层或 hashed Web 资产；后端运行时优先读取该值并保留编译期回退。
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
- release / install documentation
- embedded asset HTTP contract tests

### Out of scope

- 101 部署 rollout
- 容器镜像运行时行为变更
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
- Given 两次构建只改变发布版本号
  When 使用同一 `web/dist` 稳定资产构建镜像 A/B
  Then RootFS 稳定前缀完全相同，只有最后动态元数据层不同；镜像 ENV 与 OCI version label 必须分别匹配 A/B，且 `/api/version` 后端与前端版本一致。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo check --locked --all-targets --all-features`
- `cd web && bun install --frozen-lockfile && bun run build`
- Targeted contract tests for embedded public/admin assets and existing console route compatibility.
- `scripts/check-version-layer-reuse.sh` 必须执行前端产物 A/B、镜像 RootFS 前缀、ENV/LABEL 与严格上下文审计；基础镜像均使用 tag+digest，`.github/dependabot.yml` 每周更新 Docker ecosystem。

## 风险 / 假设

- 假设 GitHub-hosted `ubuntu-24.04` 与 `ubuntu-24.04-arm` runners 均可用，并能通过 Zig + musl 产出 portable binary。
- 风险：构建时没有 `web/dist` 时，binary 将不含内嵌资源；release workflow 必须先构建 Web 资产再构建 release binary。
- 假设 release-local artifact 复用继续沿用 `web/dist` 目录合同，因此 Dockerfile 与 build script 无需改动路径语义。
- 风险：若依赖树回退到 `native-tls` 或 `sqlite-unbundled`，portable 资产会重新引入宿主机动态库依赖；workflow 必须显式检查打包产物链接面。
