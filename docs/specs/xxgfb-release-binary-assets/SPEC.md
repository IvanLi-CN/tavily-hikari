# Release：裸机二进制资产分发（#xxgfb）

## 状态

- Status: active
- Created: 2026-06-04
- Last: 2026-06-04

## 背景 / 问题陈述

- 现有 release workflow 已能发布 GHCR `linux/amd64` + `linux/arm64` 镜像，但裸机部署仍需要额外处理容器运行时或手工拼装二进制与 Web 静态资源。
- `tavily-hikari` 的 Rust 服务可以直接作为单进程运行；发布链路应提供可直接下载、校验、解压和启动的 Linux 二进制资产。
- Web SPA 资产必须随 release binary 可用，否则裸机用户还需要单独同步 `web/dist`，发布体验会与容器镜像不一致。

## 目标 / 非目标

### Goals

- stable / rc release 均在 GitHub Release 中发布 `linux/amd64` 与 `linux/arm64` 的 `tar.gz` 二进制资产。
- 每个二进制资产同时发布 `.sha256` 校验文件。
- release binary 内嵌构建时的 `web/dist`，即使运行时没有外部静态目录，也能服务 `/`、`/admin`、`/console`、`/version.json` 与公共图标资源。
- 保留 `--static-dir` / `WEB_STATIC_DIR` 外部静态目录覆盖，且外部目录优先于内嵌资产。
- 继续保留 GHCR 镜像发布路径，不用 binary 替代镜像。
- release workflow 在上传 GitHub Release 前，对打包后的 binary 做本机 smoke，阻断不可用资产发布。

### Non-goals

- 不把 `xray` 一起打包进本次二进制资产。
- 不改变数据库、API、MCP、计费或认证业务语义。
- 不改变现有 GHCR tag、manifest 与 release intent 语义。
- 不新增 Windows 或 macOS 二进制资产。

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
  When `binary-native` jobs 完成
  Then GitHub Release 必须包含 `tavily-hikari-<tag>-linux-amd64.tar.gz`、`tavily-hikari-<tag>-linux-amd64.tar.gz.sha256`、`tavily-hikari-<tag>-linux-arm64.tar.gz`、`tavily-hikari-<tag>-linux-arm64.tar.gz.sha256`。
- Given 二进制资产被解压到无 `web/dist` 的裸机目录
  When 使用 `--bind`、`--port`、`--db-path` 启动服务
  Then `/health` 返回 200，`/`、`/admin`、`/console` 能返回 HTML，`/version.json` 返回版本 JSON，图标资源可访问。
- Given 运行时指定了有效外部静态目录
  When 该目录中存在目标文件
  Then 服务优先返回外部静态目录内容，内嵌资产只作为兜底。
- Given 任一架构 binary smoke 失败
  When release workflow 进入 GitHub Release job 前
  Then GitHub Release 资产上传必须被阻断。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo check --locked --all-targets --all-features`
- `cd web && bun install --frozen-lockfile && bun run build`
- Targeted contract tests for embedded public/admin assets and existing console route compatibility.

## 风险 / 假设

- 假设 GitHub-hosted `ubuntu-24.04` 与 `ubuntu-24.04-arm` runners 均可用并能安装 native SQLite build dependencies。
- 风险：构建时没有 `web/dist` 时，binary 将不含内嵌资源；release workflow 必须先构建 Web 资产再构建 release binary。
