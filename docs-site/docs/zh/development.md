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

- `main` 现在按 repo-local `quality-gates` contract 保护：全员 `PR-only`、required checks 使用 strict 模式、要求 signed commits、禁止 force-push 和 branch deletion，且对 admins 生效。
- merge contract 的 required checks 固定为：
  - `Quality Gates Contract`
  - `Release intent label gate`
  - `Worktree Bootstrap Smoke`
  - `Web Assets`
  - `Lint & Checks`
  - `Backend Shard Plan`
  - `Backend Tests`
  - `Frontend Checks`
  - `Compose Smoke (ForwardAuth + Caddy)`
  - `Build (Release)`
  - `Docs Pages Gate`
- `Docs Pages Gate` 是 docs surface 的唯一 required check。`build-docs`、`build-storybook`、`assemble-pages` 仍会保留为 leaf checks，用来定位具体失败点；若本次改动与 docs/site/Storybook 无关，该 gate 会 success/no-op，而不是缺失。
- `CI Pipeline` 负责 Rust 检查、后端测试与 compose smoke。
- `Docs Pages` 对所有 PR / `main` push 都会执行 scope 判断；命中 docs/web/README/根 `.bun-version`/assemble 相关改动时，它负责 docs-site + Storybook 的构建、组装与 GitHub Pages 发布，否则只产出 `Docs Pages Gate` 的 no-op success。
- `Release` 根据 PR intent label 发布容器镜像。

## Owner-side drift audit

在本机登录好 `gh` 后，可以直接运行：

```bash
python3 .github/scripts/check_quality_gates.py --github-live IvanLi-CN/tavily-hikari --github-branch main
```

它会同时校验：

- repo-local `.github/quality-gates.json` schema 与 workflow inventory
- GitHub `main` branch protection 的 required checks / strict / admin enforcement / force-push / deletion 状态
- GitHub `main` 的 required signatures（signed commits）

如需重新生成 branch protection payload，再交给 `gh api` 同步，可先运行：

```bash
python3 .github/scripts/check_quality_gates.py --emit-branch-protection-payload
```
