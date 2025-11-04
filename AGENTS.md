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
  - `npm ci` — install deps; `npm run dev` — local dev (Vite).
  - `npm run build` — build SPA to `web/dist`; `npm run preview` — preview build.
- Hooks
  - `lefthook install` — enable pre-commit (`cargo fmt`, `clippy`, Markdown format) and commitlint.

## Coding Style & Naming Conventions

- Rust: 2024 edition, rustfmt defaults; modules/files `snake_case`, types `PascalCase`, functions/vars `snake_case`.
- TypeScript/React: components `PascalCase` in `*.tsx`; hooks `useXxx`.
- Markdown: formatted by dprint (line width 100). Run `npx dprint fmt` for changed `.md`.

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
  - Foreground (debug): `RUST_LOG=info cargo run -- --bind 127.0.0.1 --port 58087 --db-path tavily_proxy.db`
  - Background: `nohup env RUST_LOG=info cargo run -- --bind 127.0.0.1 --port 58087 --db-path tavily_proxy.db > logs/backend.dev.log 2>&1 & echo $! > logs/backend.pid`
  - Time-limited check: `timeout 120s RUST_LOG=info cargo run -- --bind 127.0.0.1 --port 58087 --db-path tavily_proxy.db --static-dir web/dist`
    - Always prefer wrapping foreground runs in `timeout` (adjust seconds as needed) so automation never blocks when ports are busy or startup stalls.

- Frontend (Vite):
  - Foreground (debug): `cd web && npm ci && npm run dev -- --host 127.0.0.1 --port 55173`
  - Background: `cd web && nohup npm run dev -- --host 127.0.0.1 --port 55173 > ../logs/web.dev.log 2>&1 & echo $! > ../logs/web.pid`
  - Build for static serving: `cd web && npm run build` then run backend with `--static-dir web/dist`.

- Stop background servers:
  - Backend: `kill $(cat logs/backend.pid)`
  - Frontend: `kill $(cat logs/web.pid)`

- Logs & notes:
  - `tail -f logs/backend.dev.log` and `tail -f logs/web.dev.log` to monitor runtime.
  - Ensure `logs/` exists; do not commit log or PID files.
  - Vite dev server proxies to backend when configured in `web/vite.config.ts`.

- Validation:
  - Keep Playwright/Chrome DevTools sessions open for review; verify `/api/*`, `/mcp`, and SPA routes.
  - Health: `curl -s http://127.0.0.1:58087/health` → `200`; Summary: `curl -s http://127.0.0.1:58087/api/summary | jq .`.

**IMPORTANT**

- 2025-03-??: During high-anonymity testing we accidentally hit the official Tavily MCP endpoint. Testing is now restricted to stub or sandbox upstreams only. Never point this project at the production Tavily endpoint unless explicitly approved.

### Project-Specific Notes

- 2025-03-??: During high-anonymity testing we accidentally hit the official Tavily MCP endpoint. All future tests must target a local/mock upstream. Never hit production Tavily without explicit approval.
