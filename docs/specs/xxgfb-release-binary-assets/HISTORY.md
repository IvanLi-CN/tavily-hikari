# History：Release 裸机二进制资产分发（#xxgfb）

## 关键决策

- 首批 binary 平台与镜像平台保持一致：`linux/amd64` 与 `linux/arm64`。
- 资产采用自包含 `tar.gz`，保留 Linux 可执行权限并减少裸机部署步骤。
- 每个平台资产同时发布 SHA256 校验文件，便于运维审计与下载校验。
- 外部静态目录继续保留为运行时覆盖入口；内嵌 Web 资产只作为默认发布兜底。
- portable Linux binary 采用“并行新增”策略，而不是替换现有 native binary 资产，避免破坏既有下载脚本与回滚路径。
- portable 资产固定走 musl/Zig 构建，目标是 old-Linux 风格单文件分发；验收点不是“更小”，而是“不再依赖宿主机 glibc/OpenSSL/libsqlite3 运行时”。
- 为避免引入发布语义回归，native Linux binary 继续保留现有 TLS backend；只有 portable musl 构建切到 `rustls` 静态链路。
- `xray` 不随主程序 binary 打包，继续由宿主环境单独安装或配置。
- GitHub Release job 不 checkout 仓库时，`gh` CLI 必须显式指定 repository，不能依赖本地 `.git` 上下文。
- release workflow 内部的 `web/dist` 只构建一次，再通过 release-local artifact 复用给 Docker 与 binary 矩阵，避免在发布链里重复 Bun 安装与前端构建。
- portable 资产合同按目标源码树声明启用，而不是按“当前主干 workflow 是否支持 portable”强推到所有历史 backfill；`workflow_dispatch(head_sha=...)` 必须继续兼容 pre-portable 提交的 native-only 发布事实。
- portable 构建链上的 `cargo-zigbuild` 必须显式钉版本，否则 tag 重放或历史 backfill 会因外部工具漂移而失去可复现性。
