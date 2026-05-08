# Implementation

- Backend: request log schema, IP resolution, observed header values, recent IP count query.
- API: system settings payload/response extension and admin-only observed header endpoint.
- UI: system settings dialog, user IP count badges, recent request diagnostics.

## Status

- Implemented on current fast-track branch.

## Validation

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`
- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
