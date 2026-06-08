# History：Release 裸机二进制资产分发（#xxgfb）

## 关键决策

- 首批 binary 平台与镜像平台保持一致：`linux/amd64` 与 `linux/arm64`。
- 资产采用自包含 `tar.gz`，保留 Linux 可执行权限并减少裸机部署步骤。
- 每个平台资产同时发布 SHA256 校验文件，便于运维审计与下载校验。
- 外部静态目录继续保留为运行时覆盖入口；内嵌 Web 资产只作为默认发布兜底。
- `xray` 不随主程序 binary 打包，继续由宿主环境单独安装或配置。
- GitHub Release job 不 checkout 仓库时，`gh` CLI 必须显式指定 repository，不能依赖本地 `.git` 上下文。
- release workflow 内部的 `web/dist` 只构建一次，再通过 release-local artifact 复用给 Docker 与 binary 矩阵，避免在发布链里重复 Bun 安装与前端构建。
