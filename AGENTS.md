# Repository Guidelines

## Project Structure & Module Organization

- `src/`: Rust backend (`main.rs`, `lib.rs`, `server.rs`).
- `web/`: Vite + React SPA (TypeScript). Built assets in `web/dist`.
- `.env`: local config (e.g., `TAVILY_API_KEYS`). Do not commit secrets.
- SQLite files (`*.db`) are runtime artifacts and safe to ignore.

## Build, Test, and Development Commands

- Backend
  - `cargo build` — compile the server.
  - `cargo run -- --help` — show CLI flags; `--bind/--port/--db-path` etc.
  - `cargo fmt` — format Rust code; `cargo clippy -- -D warnings` — lint.
  - `cargo test` — run tests (add as you go).
- Frontend (`web/`)
  - `bun install --frozen-lockfile` — install deps; `bun run dev` — local dev (Vite).
  - `bun run build` — build SPA to `web/dist`; `bun run preview` — preview build.
- Hooks
  - `lefthook install` — enable pre-commit (`cargo fmt`, `clippy`, Markdown format) and commitlint.

## Coding Style & Naming Conventions

- Rust: 2024 edition, rustfmt defaults; modules/files `snake_case`, types `PascalCase`, functions/vars `snake_case`.
- TypeScript/React: components `PascalCase` in `*.tsx`; hooks `useXxx`.
- Markdown: formatted by dprint (line width 100). Run `bunx dprint fmt` for changed `.md`.

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

## Agent Runtime Conventions (Dev)

- Background, non-blocking: run backend and frontend concurrently without blocking the Agent’s prompt.
- Default high ports: backend `58087`, frontend `55173` (increment within high range if needed).

- Backend (Rust):
  - **Long-lived / persistent (Codex):** use `devctl` (Zellij) so it survives turn boundaries:
    - `~/.codex/bin/devctl up api -- env FOREGROUND=1 scripts/start-backend-dev.sh`
  - The script respects env vars like `TAVILY_API_KEYS`, `TAVILY_UPSTREAM`, `DEV_OPEN_ADMIN`.
  - One-off smoke check (foreground): `timeout 120s env FOREGROUND=1 scripts/start-backend-dev.sh` (avoid hand-rolling `cargo run`).

- Frontend (Vite):
  - **Long-lived / persistent (Codex):** use `devctl` (Zellij):
    - `~/.codex/bin/devctl up web -- env FOREGROUND=1 scripts/start-frontend-dev.sh`
  - `scripts/start-frontend-dev.sh` automatically installs dependencies if `node_modules` is missing.
  - Build for static serving: `cd web && bun run build`, then run backend with `scripts/start-backend-dev.sh` so it picks up `web/dist`.

- Stop services:
  - Persistent (devctl): `~/.codex/bin/devctl down api` and `~/.codex/bin/devctl down web` (or `~/.codex/bin/devctl down-all`)
  - Legacy nohup mode:
    - Backend: `kill $(cat logs/backend.pid)` (the script recreates PID file on next start)
    - Frontend: `kill $(cat logs/frontend.pid)`

- Logs & notes:
  - Persistent (devctl): `~/.codex/bin/devctl logs api -n 200` and `~/.codex/bin/devctl logs web -n 200`
  - Legacy nohup mode: `tail -f logs/backend.dev.log` and `tail -f logs/web.dev.log`
  - Ensure `logs/` exists; do not commit log or PID files.
  - Vite dev server proxies to backend when configured in `web/vite.config.ts`.

- Validation:
  - Keep Playwright/Chrome DevTools sessions open for review; verify `/api/*`, `/mcp`, and SPA routes.
  - Health: `curl -s http://127.0.0.1:58087/health` → `200`; Summary: `curl -s http://127.0.0.1:58087/api/summary | jq .`.

**IMPORTANT**

- 2025-03-??: During high-anonymity testing we accidentally hit the official Tavily MCP endpoint. Testing is now restricted to stub or sandbox upstreams only. Never point this project at the production Tavily endpoint unless explicitly approved.

### Project-Specific Notes

- 2025-03-??: During high-anonymity testing we accidentally hit the official Tavily MCP endpoint. All future tests must target a local/mock upstream. Never hit production Tavily without explicit approval.
- daisyUI llms.txt: https://daisyui.com/llms.txt give me a light daisyUI 5 theme with tropical color palette

## Agent Review Prep

- 工作收尾时，心羽需确保后端服务正在运行（dev 模式可加 `--dev-open-admin`），以便主人可以立即访问 `/` 或 `/admin` 进行验收。若需关闭服务，必须先征得主人确认再停。
- 心羽在“工作就绪”进入评审前，必须确保开发服务器已就绪：后端监听在 `127.0.0.1:58087` 且健康检查通过，前端 Vite Dev Server 运行在 `127.0.0.1:55173`，页面可直接打开并完成交互验证（必要时保持 Playwright 会话开启供主人复查）。
