# Implementation

- Backend: request log schema, IP resolution, observed header values, recent IP count query.
- API: system settings payload/response extension and admin-only observed header endpoint.
- UI: system settings dialog, user IP count badges, recent request diagnostics.
- Operations: historical `request_user_id` repair is an explicit resumable
  `request_user_id_backfill` CLI batch job, not part of service startup.

## Status

- Implemented on current fast-track branch.
- Startup only applies schema-compatible changes; large historical request log
  backfills must run outside the healthcheck path.

## Validation

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`
- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
