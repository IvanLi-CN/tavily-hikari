# Implementation

## Backend

- Added `src/ha.rs` with HA mode, role state machine, runtime status view, and Tencent TC3-signed EdgeOne client calls.
- Added HA startup role detection from EdgeOne current origin.
- Added runtime EdgeOne authority refresh so a running old active enters `recovery` when the origin moves away, and an externally pointed standby only becomes `provisional_master` until administrator finalize.
- Replaced SQLite snapshot export/import with deprecated `410 Gone` responses so HA cannot transfer full database files.
- Added admin/internal endpoints for HA status, zstd NDJSON state baseline, zstd NDJSON outbox events, event acknowledgement, promote, finalize, and recovery import.
- Added per-node HA source settings persistence and admin API so the current instance can store a private source override, switch between direct origin and source group, and optionally apply the saved source to EdgeOne immediately. The startup config now also accepts `HA_SOURCE_KIND` and `HA_SOURCE_ORIGIN_GROUP_ID` as per-node defaults, while `EDGEONE_EXPECTED_ORIGIN_*` stays direct-origin only.
- Added standby pull-based sync controlled by `HA_SYNC_SOURCE_URL`, `HA_INTERNAL_TOKEN`, and `HA_SYNC_INTERVAL_SECS`; active nodes no longer push snapshots.
- Added `ha_outbox` and peer watermark storage so standby nodes can apply small, whitelisted state changes by seq.
- Added recovery batch idempotency, HA sync watermarks, failover operation persistence, EdgeOne request/response audit persistence, and node state persistence.
- Adjusted EdgeOne origin switching to require explicit origin protocol, host, and port configuration, send them as top-level EdgeOne API fields, and normalize EdgeOne describe responses that omit default ports.
- Added full-master fencing for system settings, upstream key creation, user token management, user quota changes, registration settings, OAuth login start, recharge order creation, and payment notify.
- Added basic-business fencing for external Tavily HTTP API, MCP root/subpaths, and Tavily usage routes; `standby` and `recovery` return 503 before auth/quota/upstream work.
- Restricted non-force promote to `standby` callers so an active node cannot demote itself through an accidental promote operation.
- Added HA schema tables for node state, sync watermarks, failover operations, recovery batches, and EdgeOne audit logs.
- Extended HA node state storage with direct-origin and source-group columns so the current instance can override the Env/CLI default source without joining HA sync.
- Recovery import now rejects request/auth-token log payloads and accepts only mergeable ledger-style payloads, keeping the importing new master in its current active role.

## Frontend

- Added API bindings for HA status, promote, and finalize.
- Added shared `HaStatusBanner` with admin and user presentation modes.
- Added admin HA service node panel with active-standby status details, including node inventory, role, origin, health, EdgeOne domain/current/expected origin, EdgeOne API configuration, sync timestamps, basic traffic/full write gates, recovery status, message, and row-level promote/finalize actions.
- Moved the full admin HA panel into the System Settings high-availability subpage at `/admin/system-settings/ha`; normal admin business pages no longer render HA UI, and abnormal states only render a compact link to the HA settings page.
- Added promote/finalize actions for degraded admin states inside the HA settings page only.
- Added user console banner for degraded HA states.
- Added Storybook scenarios for provisional, standby, full master, recovery, compact admin attention, System Settings high availability, and user degraded states.
- Added i18n-backed HA service-node copy for zh/en, plus a source-configuration dialog on the System Settings high-availability page and Storybook/tests that no longer hardcode English text.

## Validation

- `cargo fmt --check`
- `cargo check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test ha_ -- --nocapture`
- `cd web && bun run build`
- `python3 -m py_compile tests/ha/scripts/*.py`
- Shared `codex-testbox` Docker Compose harness with mock EdgeOne, mock ingress, dual app nodes, and mock upstream:
  `pre -> failover -> recovery`.

## Integration Harness

- Added `tests/ha/docker-compose.yml` for mock EdgeOne, EdgeOne ingress, mock Tavily upstream, `node-a`, `node-b`, and `ha-test-runner`.
- Added `tests/ha/scripts/run_ha_acceptance.py` with staged acceptance checks:
  `pre`, `failover`, and `recovery`.
- Added Python mocks for EdgeOne origin describe/modify, single-entry ingress forwarding, and Tavily/MCP upstream responses.
- The harness uses only mock upstreams and runs on `codex-testbox`; it does not call the production Tavily or EdgeOne endpoints.
