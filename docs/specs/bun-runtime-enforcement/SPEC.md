# Bun Runtime 强制化与 Node 痕迹收口（#9b9w5）

## Context and Scope

- 旧的 `docs/specs/bun-runtime-enforcement/HISTORY.md` 已经完成了 package manager、lockfile 与 CI 的 Bun 迁移，但仓库日常命令仍可能因为 `node_modules/.bin/*` 的 shebang 回落到 `node` runtime。
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

## Requirements

### REQ-BUN-REPO-RUNTIME

- 仓库维护的 root 与 web 命令 MUST 通过 Bun 执行，不得依赖系统 `node` runtime。

### REQ-BUN-WEB-BUILD

- Vite production build MUST 成功，且 `web/dist` MUST NOT 包含静态版本元数据 `version.json`。

### REQ-BUN-HTTP-ROUTES

- Web 构建 MUST 保持现有页面、API、代理与健康检查入口不变。

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

## Verification

### VER-BUN-CORE-COMMANDS

- Method: run the root/web Bun commands and the failing-`node`-shim proof below.
- covers: `REQ-BUN-REPO-RUNTIME`, `REQ-BUN-WEB-BUILD`
- Pass condition: required install, build, and tool commands pass without invoking the system `node` runtime; the production build has no static `version.json`.

### VER-BUN-BROWSER-ROUTES

- Method: smoke-test the page, API, proxy, and health-check entrypoints below.
- covers: `REQ-BUN-HTTP-ROUTES`
- Pass condition: routes remain available and preserve their existing behavior.

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
  Then 构建成功。

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

## 风险 / 开放问题 / 假设

- 风险：若某个第三方 CLI 仅在 shebang 链路下工作、但不能稳定经 `bun --bun` 运行，可能需要对单个脚本入口单独兼容处理。
- 风险：`node_modules/.bin` 中仍会保留 `#!/usr/bin/env node` 的第三方可执行文件；本轮只能消除“仓库默认命令路径”对它们的依赖，不能改变第三方分发格式。
- 假设：Storybook 继续保留为验收工具，不作为“Strict no-Node” 的阻断项，只要求其由 Bun runtime 成功驱动。
- 假设：系统层面的 `node` 安装状态不纳入本 spec 的完成条件；完成条件是“前置失败 node shim 后仓库核心命令仍通过”。

## Related ADRs

None
