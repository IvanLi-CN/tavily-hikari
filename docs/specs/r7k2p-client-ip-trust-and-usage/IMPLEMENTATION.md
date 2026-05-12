# Implementation

- Backend: request log schema, IP resolution, observed header values, recent 24h/7d IP count query, capped per-user IP address lists, and capped 7-day IP timeline query.
- API: system settings payload/response extension and admin-only observed header endpoint.
- UI: system settings dialog, global IP limit input, user IP count badges, user detail IP chart/list, recent request diagnostics.
- Operations: historical `request_user_id` repair is an explicit resumable
  `request_user_id_backfill` CLI batch job, not part of service startup.

## Status

- Implemented on current fast-track branch.
- Startup only applies schema-compatible changes; large historical request log
  backfills must run outside the healthcheck path.
- Current follow-up adds 24h IP counts, global IP warning threshold, and user detail IP timeline/list on top of the original 7-day count work.

## Validation

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`
- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
