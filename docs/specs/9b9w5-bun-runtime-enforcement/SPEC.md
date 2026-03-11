# Bun Runtime 强制化与 Node 痕迹收口（#9b9w5）

## 状态

- Status: 已完成（6/6）
- Created: 2026-03-09
- Last: 2026-03-09

## 背景 / 问题陈述

- 旧的 `docs/plan/9b9w5:bun-migration/PLAN.md` 已经完成了 package manager、lockfile 与 CI 的 Bun 迁移，但仓库日常命令仍可能因为 `node_modules/.bin/*` 的 shebang 回落到 `node` runtime。
- 当前 root / `web/` 的 `package.json` 都已固定 `packageManager: bun@1.3.10`，CI / release workflow 也已切到 `oven-sh/setup-bun@v2`；因此新的问题不再是“是否迁移到 Bun”，而是“如何把 Bun 作为默认执行 runtime 收口到位”。
- 本轮已确认 `commitlint`、`dprint`、`tsc`、`vite build`、`storybook build` 可以在 `bun --bun` 下执行成功，因此不需要为了“去 Node”重写整套前端工具链。

## 目标 / 非目标

### Goals

- 保留现有 Vite / TypeScript / Storybook / commitlint / dprint 工具链，同时确保仓库内日常命令默认由 Bun 直接执行，不依赖 `node` 二进制。
- 为 root、`web/`、Git hooks、开发脚本补齐 Bun runtime 强制层，避免 `.bin` shebang 将命令落回 `#!/usr/bin/env node`。
- 将仓库自有、可控的 Node 命名痕迹收敛为中性命名，重点覆盖 tooling tsconfig 与 Tavily HTTP E2E smoke 脚本入口。
- 用前置失败 `node` shim 的验证方式证明核心命令在无可用 `node` 的情况下仍能通过。
- 保持现有公共 HTTP 路由、页面入口、端口契约和产品功能不变。

### Non-goals

- 不替换 Vite、TypeScript、Storybook、commitlint、dprint 这些已经可由 Bun runtime 直接执行的现有工具。
- 不为了“命名洁癖”一次性重写所有 `node:` imports、`@types/node`、或其余 loader-sensitive 配置入口。
- 不改动 Rust 业务逻辑、数据库结构、HTTP API 契约或页面运行时产品行为。

## 范围（Scope）

### In scope

- Root runtime enforcement
  - root `package.json` 中需要显式经 Bun 执行的脚本。
  - root 级 Bun runtime 配置（若需要）。
  - `lefthook.yml` 中通过 `bunx --bun` 运行的 dprint / commitlint。
- Web runtime enforcement
  - `web/package.json` 中 `dev`、`build`、`storybook`、`build-storybook` 等脚本的 Bun runtime 执行路径。
  - `scripts/start-frontend-dev.sh` 对 Bun-only 执行模型的说明与启动路径。
  - `web/tsconfig.tooling.json` 的中性命名替换与引用同步。
- Repo-owned Node naming cleanup
  - `tests/e2e/tavily_http_smoke.ts` 迁移为 Bun-native、中性命名的 smoke 脚本。
  - `commitlint.config.mjs`、`web/tailwind.config.ts`、`web/scripts/write-version.mjs` 这类仓库自有、发现面稳定的配置/脚本入口适度去 Node 命名。
  - 相关 README / docs / spec 文案同步。
- Validation and proof
  - root / `web/` 的 `bun install --frozen-lockfile`。
  - `cd web && bun run build`、`cd web && bun run build-storybook`。
  - failing `node` shim 前置后的 no-node proof。
  - `/`、`/admin`、`/console`、`/login`、`/api/*`、`/mcp`、`/health` 浏览器 smoke。

### Out of scope

- 切换到 Bun bundler / Bun.serve 以替代现有 Vite dev server 或 Storybook。
- 继续处理 `web/postcss.config.cjs`、`vite.config.ts`、`@types/node` 等仍可能影响加载器或类型面的残余 Node 痕迹。
- 改变 Storybook 作为组件验收环境的角色与覆盖范围。
- 移除系统层面的 Node 安装；本轮只证明仓库核心命令不再依赖它。

## 接口契约（Interfaces & Contracts）

### Public / runtime-facing contracts

- 以下外部入口必须保持不变：
  - 页面入口：`/`、`/admin`、`/console`、`/login`
  - API / proxy 入口：`/api/*`、`/mcp`、`/health`
- 开发端口保持不变：
  - backend: `127.0.0.1:58087`
  - frontend: `127.0.0.1:55173`
  - Storybook: `127.0.0.1:56006`

### Tooling contracts

- root / `web/` 继续使用 Bun lockfile 作为唯一 JS 依赖锁定基线。
- 仓库所有由我们维护的脚本入口，凡是会命中 `.bin` shebang 的路径，都必须显式通过 Bun 执行（`bun --bun` / `bunx --bun`）。
- loader-sensitive 配置文件允许按收益择机保留，只要在实际执行链路中不再要求 `node` runtime。

## 验收标准（Acceptance Criteria）

- Given root 依赖已安装
  When 执行 `bun install --frozen-lockfile`
  Then 命令成功，且不需要 `npm` / `yarn` / `pnpm`。

- Given web 依赖已安装
  When 执行 `cd web && bun install --frozen-lockfile`
  Then 命令成功，且 lockfile 不发生意外漂移。

- Given 仓库脚本已完成 runtime enforcement
  When 执行 `bunx --bun dprint --version` 与 `bunx --bun commitlint --version`
  Then 两者均成功，且执行链不依赖 `node` 二进制。

- Given `web/` 构建脚本已切换到 Bun runtime
  When 执行 `cd web && bun run build`
  Then 构建成功，并生成 `web/dist/version.json`。

- Given Storybook 仍保留为验收工具
  When 执行 `cd web && bun run build-storybook`
  Then 构建成功，且不要求直接调用 `node`。

- Given 一个失败的 `node` shim 被前置到 `PATH`
  When 执行 root hook/tool commands、`cd web && bun run build`、`cd web && bun run build-storybook`
  Then 命令仍全部通过；若任何命令仍尝试执行 `node`，则本工作项视为未完成。

- Given backend 与 frontend dev server 已启动
  When 在浏览器中访问 `/`、`/admin`、`/console`、`/login`
  Then 页面可正常加载，且 `/api/*`、`/mcp`、`/health` 代理/健康检查行为与迁移前一致。

## 非功能性验收 / 质量门槛（Quality Gates）

### Local validation

- `bun install --frozen-lockfile`
- `bunx --bun dprint --version`
- `bunx --bun commitlint --version`
- `cd web && bun install --frozen-lockfile`
- `cd web && bun run build`
- `cd web && bun run build-storybook`

### No-node proof

- 创建一个临时 `node` shim 并前置到 `PATH`，该 shim 必须立即失败。
- 在该 shim 环境下，至少复验：
  - root hook/tool 实际命令路径（`bunx --bun dprint fmt`、`bunx --bun commitlint --edit`）
  - `cd web && bun run build`
  - `cd web && bun run build-storybook`

### Browser smoke

- backend 保持监听 `127.0.0.1:58087`
- frontend 保持监听 `127.0.0.1:55173`
- Chrome DevTools 会话保留供复查

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 迁移 legacy plan，创建新的 Bun runtime enforcement spec 与索引条目
- [x] M2: root / web package scripts + hooks 完成 Bun runtime 强制化
- [x] M3: repo-owned Node 命名痕迹收敛（tooling tsconfig + Tavily HTTP smoke）
- [x] M4: README / dev script / docs 同步到 Bun-only 执行口径
- [x] M5: no-node proof、本地 build、browser smoke 完成
- [x] M6: fast-track PR / checks / review-loop 收敛

## 风险 / 开放问题 / 假设

- 风险：若某个第三方 CLI 仅在 shebang 链路下工作、但不能稳定经 `bun --bun` 运行，可能需要对单个脚本入口单独兼容处理。
- 风险：`node_modules/.bin` 中仍会保留 `#!/usr/bin/env node` 的第三方可执行文件；本轮只能消除“仓库默认命令路径”对它们的依赖，不能改变第三方分发格式。
- 假设：Storybook 继续保留为验收工具，不作为“Strict no-Node” 的阻断项，只要求其由 Bun runtime 成功驱动。
- 假设：系统层面的 `node` 安装状态不纳入本 spec 的完成条件；完成条件是“前置失败 node shim 后仓库核心命令仍通过”。

## 变更记录（Change log）

- 2026-03-09: 从 `docs/plan/9b9w5:bun-migration/PLAN.md` 迁移为 `docs/specs/9b9w5-bun-runtime-enforcement/SPEC.md`，将目标从“Bun 包管理迁移”收口为“Bun runtime 强制化 + repo-owned Node naming cleanup”。

- 2026-03-09: 已完成 root / web `bun --bun` 脚本收口、`bunfig.toml` 强制层、`lefthook` Bun runtime、`tsconfig.tooling.json` 重命名、`tavily_http_smoke.ts` 迁移与 README/AGENTS 文案同步。
- 2026-03-09: 已通过 `bun install --frozen-lockfile`、`cd web && bun install --frozen-lockfile`、`cd web && bun run build`、`bun run validate:no-node-runtime`；浏览器确认 `/`、`/admin`、`/console`、`/login` 与 `/api/summary`、`/health`、`/mcp` 开发代理链路可用（`/mcp` 指向本地 mock upstream）。
- 2026-03-09: PR #111 已补齐 `type:skip` + `channel:stable` 标签，CI checks 全绿；clean-room `codex review --base main` 复跑确认无阻塞缺陷，spec 与索引同步收口为已完成。
- 2026-03-09: 按“适当消除、不硬改”继续收口：`commitlint.config.mjs` 改为 ESM、`web/tailwind.config.ts` 改为 TS config、`web/components.json` 同步新路径，`web/scripts/write-version.mjs` 改成 Bun-native 版本写入脚本；`web/postcss.config.cjs`、`vite.config.ts` 与 `@types/node` 暂保留。
- 2026-03-09: `scripts/validate-no-node-runtime.sh` 改为真实执行 hook 命令路径（`dprint fmt` / `commitlint --edit`），避免仅查版本号导致的 no-node 假阳性。
- 2026-03-09: Bun pin 升级到 `1.3.10`；共享测试机上验证到 `1.3.9` 在 Linux 下执行 `bunx --bun dprint fmt` 仍会回落到 `node_modules/.bin/dprint`，升级后 no-node proof 通过。
