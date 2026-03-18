# Tavily Hikari Docs

Tavily Hikari is a Rust + Axum proxy for Tavily traffic. It rotates multiple upstream keys, keeps
full SQLite-backed audit logs, supports admin and end-user access flows, and ships with a React +
Vite operator console.

This public docs site is intentionally scoped to:

- product overview and local onboarding
- deployment and access patterns
- HTTP API integration guidance
- development and Storybook review entry points

It intentionally does **not** publish internal execution specs under `docs/specs/**` or legacy work
plans under `docs/plan/**`.

## Documentation map

1. Start with [Quick Start](/quick-start) if you need a local dev, Docker, or Docker Compose setup.
2. Open [Configuration & Access](/configuration-access) for CLI flags, ForwardAuth, built-in admin
   login, and Linux DO OAuth.
3. Use [HTTP API Guide](/http-api-guide) when integrating clients such as Cherry Studio.
4. Read [Deployment & Anonymity](/deployment-anonymity) for production and high-anonymity notes.
5. Visit [Storybook](/storybook.html) or the curated [Storybook Guide](/storybook-guide.html) when
   you want to review UI states instead of prose.

## Product shape

- Backend: Rust 2024, Axum, SQLx, Tokio, Clap
- Data: SQLite (`api_keys`, `request_logs`, user/session state)
- Frontend: React 18, TanStack Router, Tailwind CSS, shadcn/ui, Vite 5
- Delivery surfaces: runtime web app, Storybook, and this public docs site

## What to open next

- Need a running instance fast: [Quick Start](/quick-start)
- Need auth and operator model details: [Configuration & Access](/configuration-access)
- Need endpoint and token contract details: [HTTP API Guide](/http-api-guide)
- Need visual QA surfaces: [Storybook Guide](/storybook-guide.html)
