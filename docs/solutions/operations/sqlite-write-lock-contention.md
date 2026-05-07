---
title: SQLite write-lock contention
module: tavily-hikari
problem_type: production_lock_contention
component: sqlite-writes
tags:
  - sqlite
  - production
  - billing
  - mcp
status: active
related_specs:
  - docs/specs/2wdrp-sqlite-write-lock-hardening/SPEC.md
  - docs/specs/s2vd2-upstream-credits-billing/SPEC.md
  - docs/specs/34pgu-mcp-session-privacy-affinity-hardening/SPEC.md
---

# SQLite write-lock contention

## Context

Tavily Hikari uses one SQLite database for billing ledgers, session affinity, scheduled job logs,
OAuth account state, quota sync samples, and admin/read models. WAL mode allows readers and one
writer to coexist, but only one writer can hold the write slot at a time.

## Symptoms

- Logs contain `database is locked` while `/health` remains OK.
- Request-path messages mention `token billing lock failed` or `mcp session ... lock failed`.
- Background messages mention `token-usage-rollup: start job error`, `quota-sync-hot: start job error`, or LinuxDo OAuth upsert failures.
- WAL can be large without itself proving corruption; it is a signal to inspect writer pressure and
  long readers before performing maintenance.

## Root Cause

Short-lived SQLite writer collisions can happen when request-path billing/session locks and
background writes all touch the same DB. Treating every transient busy/locked response as fatal makes
brief contention visible as HTTP 500s or failed background bookkeeping.

## Resolution

- Keep billing and MCP serialization fail-closed, but retry transient SQLite busy/locked writes
  inside the existing bounded lock wait or lease budget.
- Retry background job bookkeeping writes before surfacing scheduler errors.
- Retry OAuth upsert/refresh wrapper calls so login/profile sync can survive short writer collisions.
- Prefer bounded retries and narrower write windows before increasing SQLite pool size.

## Guardrails / Reuse Notes

- Do not fix this class of problem by simply raising `sqlx` pool size; more concurrent writers can
  increase lock pressure.
- Do not hand-edit production ledgers. Use repository repair binaries or controlled migrations when
  historical data needs correction.
- Keep request-path quota semantics stable: locked billing subject, pending replay, quota precheck,
  and settlement must remain one coherent subject.
- For WAL growth, inspect active readers and checkpoint behavior before running live maintenance.

## References

- `src/store/mod.rs`
- `src/store/key_store_users_and_oauth.rs`
- `src/store/key_store_request_logs_and_dashboard.rs`
- `src/tavily_proxy/proxy_auth_and_oauth.rs`
