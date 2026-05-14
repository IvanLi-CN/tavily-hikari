# Implementation

- Backend: request log schema, IP resolution, observed header values, recent 24h/7d IP count query, capped per-user IP address lists, and capped 7-day IP timeline query.
- API: system settings payload/response extension and admin-only observed header endpoint.
- UI: system settings dialog, global IP limit input, trusted client IP Apply/Cancel-confirmed draft editor, user IP count badges, user detail IP chart/list, recent request diagnostics.
- Operations: historical `request_user_id` repair is an explicit resumable
  `request_user_id_backfill` CLI batch job, not part of service startup.
- Performance: admin user list sorting now pages in SQL before hydrating usage details; recent IP count queries use the user/IP/time index path so bounded user pages do not scan the whole recent request-log window.

## Status

- Implemented on current fast-track branch.
- Startup only applies schema-compatible changes; large historical request log
  backfills must run outside the healthcheck path.
- Current follow-up adds 24h IP counts, global IP warning threshold, and user detail IP timeline/list on top of the original 7-day count work.
- User management and user usage list performance is optimized for large `request_logs` databases by avoiding full filtered-user hydration for DB-backed usage sorts.

## Validation

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`
- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
