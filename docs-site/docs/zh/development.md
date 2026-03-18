# 开发

## 仓库结构

- `src/`：Rust 后端、路由、服务、CLI
- `web/`：React + Vite 应用与 Storybook
- `docs-site/`：公开 Rspress 文档站
- `docs/`：内部设计文档、规格与历史计划

## 核心命令

### 后端

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

### 前端

```bash
cd web
bun install --frozen-lockfile
bun test
bun run build
bun run build-storybook
```

### docs-site

```bash
cd docs-site
bun install --frozen-lockfile
bun run build
```

## 验收面

- 运行时应用：后端 + Vite dev server
- Storybook：组件、片段、页面级 UI 验收面
- docs-site：面向操作者与集成方的公开参考文档

GitHub Pages workflow 会分别构建 docs-site 与 Storybook，再把 Storybook 挂载到 `/storybook/`
形成统一静态发布面。

## CI 与发版

- `CI Pipeline` 负责 Rust 检查、后端测试与 compose smoke。
- `Docs Pages` 负责 docs-site + Storybook 的构建、组装与 GitHub Pages 发布。
- `Release` 根据 PR intent label 发布容器镜像。
