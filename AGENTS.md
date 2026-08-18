# Repository Guidelines

## Project Structure & Module Organization

- `src/`: Rust backend (`main.rs`, `lib.rs`, `server.rs`).
- `web/`: Vite + React SPA (TypeScript). Built assets in `web/dist`.
- `.env`: local config (e.g., `TAVILY_API_KEYS`). Do not commit secrets.
- SQLite files (`*.db`) are runtime artifacts and safe to ignore.

## Build, Test, and Development Commands

- Repo tooling
  - `bun install --frozen-lockfile` — install root tooling deps and run the shared hook installer.
  - `bun run hooks:install` — reinstall the shared `post-checkout` hook and refresh `lefthook` commit hooks when the binary is available on `PATH`.
  - `bun run worktree:setup` — force a strict linked-worktree repair for env/deps/`cargo fetch`.
  - `bun run test:worktree-bootstrap` — run the linked-worktree bootstrap smoke contract.
- Backend
  - `cargo build` — compile the server.
  - `cargo run -- --help` — show CLI flags; `--bind/--port/--db-path` etc.
  - `cargo fmt` — format Rust code; `cargo clippy -- -D warnings` — lint.
  - `cargo test` — run tests (add as you go).
- Frontend (`web/`)
  - `bun install --frozen-lockfile` — install deps; `bun run --bun dev` — local dev (Vite under Bun runtime).
  - `bun run build` — build SPA to `web/dist`; `bun run preview` — preview build.
  - `bun run storybook` — run Storybook dev server at `http://127.0.0.1:56006`.
- Hooks
  - `bun install --frozen-lockfile` or `bun run hooks:install` — install the shared `post-checkout` hook; if `lefthook` exists on `PATH`, also refresh pre-commit (`cargo fmt`, `clippy`, Markdown format) and commitlint.

## Coding Style & Naming Conventions

- Rust: 2024 edition, rustfmt defaults; modules/files `snake_case`, types `PascalCase`, functions/vars `snake_case`.
- TypeScript/React: components `PascalCase` in `*.tsx`; hooks `useXxx`.
- Markdown: formatted by dprint (line width 100). Run `bunx --bun dprint fmt` for changed `.md`.

## Testing Guidelines

- Rust: prefer module unit tests via `#[cfg(test)]` and integration tests under `tests/` when needed. Run with `cargo test`.
- Frontend: no test tooling preconfigured; if introducing tests, prefer Vitest + React Testing Library in `web/`.

## Commit & Pull Request Guidelines

- Conventional Commits enforced (English only): `feat: add key rotation`, `fix(proxy): handle 432`.
  - Header ≤ 72 chars; body wrapped ≤ 100; no Chinese chars (commitlint rule).
- PRs: include clear description, linked issues, CLI or UI screenshots for relevant changes, and local run steps.

## Security & Configuration Tips

- Configure keys via `.env` or env vars (`TAVILY_API_KEYS`).
- Do not commit secrets or local DB files. Backend can serve `web/dist` when present.

## Agent skills

### Issue tracker

Engineering work is tracked in GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the canonical five-role triage vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

The repository uses a single-context domain layout. See `docs/agents/domain.md`.

## Agent Runtime Conventions (Dev)

- Default high ports: backend `58087`, frontend `55173` (increment within high range if needed).
- Prefer foreground execution for development commands; if non-blocking execution is required, the caller manages lifecycle and logging explicitly.

- Backend (Rust):
  - Start: `scripts/start-backend-dev.sh`
  - The script respects env vars like `TAVILY_API_KEYS`, `TAVILY_UPSTREAM`, `DEV_OPEN_ADMIN`.
  - One-off smoke check (foreground): `timeout 120s scripts/start-backend-dev.sh` (avoid hand-rolling `cargo run`).

- Frontend (Vite):
  - Start: `scripts/start-frontend-dev.sh`
  - `scripts/start-frontend-dev.sh` automatically installs dependencies if `node_modules` is missing, then starts Vite with `bun run --bun dev`.
  - Build for static serving: `cd web && bun run build`, then run backend with `scripts/start-backend-dev.sh` so it picks up `web/dist`.

- Linked worktrees:
  - The first checkout in a linked worktree now runs a best-effort bootstrap through the shared `post-checkout` hook.
  - Auto bootstrap only copies missing root `.env` / `.env.*` files from the primary worktree, restores missing root / `web` / `docs-site` Bun dependencies, and runs `cargo fetch --locked`.
  - Auto bootstrap never blocks checkout; missing `lefthook`, `bun`, `cargo`, or source env files only warn.
  - `bun run worktree:setup` is the explicit strict repair entrypoint.
  - The contract intentionally does not restore `*.db`, `web/dist`, `web/storybook-static`, `downloads/`, browser caches, or other runtime artifacts.

- Stop services:
  - Use the process manager or shell session that launched each service.
  - Avoid terminating unrelated sessions; only stop processes you started for this task.

- Logs & notes:
  - Logs stream to current stdout/stderr.
  - If you need persisted logs, redirect output in the caller command and keep ownership clear.
  - Vite dev server proxies to backend when configured in `web/vite.config.ts`.

- Storybook:
  - Start: `cd web && bun install --frozen-lockfile && bun run storybook` → `http://127.0.0.1:56006` (Storybook CLI forced through Bun runtime by the package script).
  - 仅在任务需要 Storybook 的 UI/浏览器验证时，于验证期间将其保留在当前 shell 或使用团队认可的后台策略；验证完成后释放该进程和会话。

- Validation:
  - 当任务需要交互验收、UI/浏览器验证或 HTTP 集成测试时，保持相关的 Playwright/Chrome DevTools 会话以供复核，并验证任务涉及的 `/api/*`、`/mcp` 和 SPA 路由。
  - 后端服务参与该验证时，Health: `curl -s http://127.0.0.1:58087/health` → `200`; Summary: `curl -s http://127.0.0.1:58087/api/summary | jq .`.

**IMPORTANT**

- 2025-03-??: During high-anonymity testing we accidentally hit the official Tavily MCP endpoint. Testing is now restricted to stub or sandbox upstreams only. Never point this project at the production Tavily endpoint unless explicitly approved.

### Project-Specific Notes

- 2025-03-??: During high-anonymity testing we accidentally hit the official Tavily MCP endpoint. All future tests must target a local/mock upstream. Never hit production Tavily without explicit approval.

## Local Service Review

- 仅在主人要求交互验收、任务的 UI/浏览器验证，或集成测试需要运行中的应用时，启动本地后端或前端服务。
- 对文档、配置、构建、CI 与其他非交互任务，使用任务对应的验证；完成工作本身不启动或保留本地服务、浏览器会话或固定端口。
- 启动服务时说明用途并限定于当前任务；仅管理当前任务启动的进程，并保留不终止无关进程的安全边界。
- 交付 localhost URL 时，仅在已请求的复核或验证期间保持相应端口和会话，并遵守全局端口租约规则。
- 相关复核或验证结束后，关闭当前任务打开的浏览器会话，停止当前任务启动的服务并释放所持端口；不得影响无关进程。
