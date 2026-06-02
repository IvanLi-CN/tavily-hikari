# Implementation

## Current Coverage

- Backend settings now expose `requestLogRetention` with defaults, range validation, and save-time
  clamp to `maxLogRetentionDays`.
- `request_logs` stores body byte counts, SHA-256 hashes, cleanup reason, and cleanup timestamp;
  policy-zero and expired bodies clear only BLOB columns.
- Request body retention policy classifies business, non-business, and non-success requests, with
  `mcp:batch` treated as business when any contained method is business.
- `request_logs.counts_business_quota` preserves `mcp:batch` billing/operational classification
  after policy-zero or expired body cleanup.
- User debug-sharing consent is persisted on `users` and exposed through the user dashboard and
  `PUT /api/user/debug-info-sharing`.
- Existing bounded `request_logs_gc` now cleans expired bodies before deleting rows past the
  configured maximum log retention window.
- Admin settings UI includes the high-frequency threshold slider and nonlinear day sliders for
  global, high-frequency, and debug-sharing profiles.
- User console shows the shared debug information toggle, and request detail views summarize
  cleaned body metadata when full bodies are no longer retained.

## Validation

- `cargo clippy -- -D warnings`
- `cargo test request_log_retention -- --nocapture`
- `cargo test request_logs_gc -- --nocapture`
- `cargo test request_log_policy_preserves_batch_non_business_classification_without_body -- --nocapture`
- `cd web && bun run build`
- Storybook visual evidence captured for Admin settings and user console debug-sharing states.
