# Implementation

## Backend

- Added `src/ha.rs` with HA mode, role state machine, runtime status view, and Tencent TC3-signed EdgeOne client calls.
- Added HA startup role detection from EdgeOne current origin.
- Added runtime EdgeOne authority refresh so a running old active enters `recovery` when the origin moves away, and an externally pointed standby only becomes `provisional_master` until administrator finalize.
- Replaced SQLite snapshot export/import with deprecated `410 Gone` responses so HA cannot transfer full database files.
- Added admin/internal endpoints for HA status, zstd NDJSON state baseline, zstd NDJSON outbox events, event acknowledgement, promote, finalize, and recovery import.
- Added per-node HA source settings persistence and admin API so the current instance can store a private source override, switch between direct origin and source group, and optionally apply the saved source to EdgeOne immediately. The startup config now also accepts `HA_SOURCE_KIND` and `HA_SOURCE_ORIGIN_GROUP_ID` as per-node defaults, while `EDGEONE_EXPECTED_ORIGIN_*` stays direct-origin only.
- Locked the HA source settings request/response wire contract to lowercase `directOriginScheme` values (`http|https|follow`) by adding serde lowercase support on the shared Rust enum, while preserving the existing uppercase conversion for outbound EdgeOne payloads.
- Added standby pull-based sync controlled by `HA_SYNC_SOURCE_URL`, `HA_INTERNAL_TOKEN`, and `HA_SYNC_INTERVAL_SECS`; active nodes no longer push snapshots.
- Replaced the old implicit single-channel HA contract with explicit `control` / `billing` /
  `runtime` channels carried over the same `/api/admin/ha/baseline`, `/api/admin/ha/events`, and
  `/api/admin/ha/events/ack` endpoints via a required `channel` contract.
- Narrowed `ha_outbox` to the `control` channel only. `billing_ledger` now emits into
  `ha_billing_outbox`, while minimal runtime state emits into `ha_runtime_outbox`.
- `HA_MODE=single` now drops all HA replication triggers at startup and does not emit new HA
  events, while preserving schema compatibility for future `active_standby` cold start.
- Added per-channel peer watermark storage keyed by `(peer_node_id, channel)` and a compatibility
  rebuild for old single-column `ha_peer_watermarks` schemas.
- Added bounded `ha_outbox_gc` maintenance with scheduler/manual-job support. The online path only
  deletes expired rows from `control` / `billing` / `runtime` outboxes within their own retention
  windows, then runs `PRAGMA wal_checkpoint(PASSIVE)`; it never runs full `VACUUM`.
- Added offline `ha_outbox_cleanup_once` and `scripts/ha-outbox-maintenance.sh` so large retained
  historical `ha_outbox` rows can be cleaned explicitly during a maintenance window before an
  optional `db_compaction_once`.
- Added a dedicated `ha_trigger_repair_once` one-shot CLI and upgraded HA trigger reconcile so an
  upgraded database no longer relies on the current whitelist alone when dropping triggers.
  Startup/manual repair now enumerates `sqlite_master`, removes legacy `trg_ha_outbox_*` leftovers
  that no longer belong to `control/billing/runtime`, then rebuilds only the current three-channel
  contract.
- Added `scripts/export-live-db-snapshot-to-testbox.sh` so operators can export the full live
  SQLite validation input from 101 into an isolated `codex-testbox` run directory. The script is
  intentionally sidecar-aware and treats the validation input as a set:
  - core DB snapshot for `tavily_proxy.db`
  - observability sibling snapshot for `tavily_proxy-observability.db`
  - per-run manifest with source paths, byte counts, SHA-256 sums, and integrity-check results
- The accepted offline validation sequence is now explicit:
  1. create a full read-only snapshot set on 101
  2. upload that full set into one `codex-testbox` run directory
  3. run `ha_trigger_repair_once` or `ha_outbox_cleanup_once --repair-triggers` there
  4. run `ha_outbox_cleanup_once` / `scripts/ha-outbox-maintenance.sh` there
  5. run `db_compaction_once` only if the threshold gate says reclaimable space is large enough
- `ha_outbox_cleanup_once` now distinguishes immediate invalid-legacy cleanup from normal retention
  cleanup in its JSON/plain reports, so operators can prove whether the pass is shrinking a stale
  upgraded backlog or only trimming aged rows.
- Shared-testbox validation against a full 101 snapshot confirmed the upgraded-database failure
  mode and the repair-first fix:
  - `ha_trigger_repair_once --ha-mode active_standby` dropped `30` legacy single-channel triggers
    before recreating the current `control/billing/runtime` trigger set.
  - `ha_outbox_cleanup_once --repair-triggers --run-until-complete --json` deleted `29,143,494`
    rows in total, including `27,770,036` invalid legacy `ha_outbox` rows and `1,373,458` normal
    retention rows.
  - After cleanup, `ha_outbox` contained only current control resources and shrank to `163,357`
    rows, while `ha_billing_outbox` / `ha_runtime_outbox` stayed at `265,496` / `19,822` rows.
  - The same validation run left `freelist_count=2,951,432`, proving roughly `12 GiB` of
    reclaimable space and confirming that a follow-up compaction is worth running in the real
    maintenance window.
  - `db_compaction_once` on the shared testbox failed with `database or disk is full` because the
    host root filesystem had only about `3.5 GiB` free. That is an environment-capacity blocker,
    not a repair-path failure; production compaction still needs adequate temporary free space.
- “Full live DB validation input” does not mean “copy the main `.db` file only.” For this service
  it means the core DB plus the observability sibling sidecar; otherwise offline verification can
  silently miss the production request-log/read-model layout.
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
- Added a local shadcn-style `Alert` primitive and upgraded the HA source settings dialog submit-failure state to use a destructive alert with operator-friendly titles, concise recovery guidance, and default-collapsed technical details. Field-level validation remains attached to the relevant direct/origin-group inputs instead of collapsing into a single raw error block.

## Visual Evidence

- Storybook canvas: `Components/HaStatusBanner/SourceDialogSubmitFailure`
  - evidence_note: The submit-failure story now actively triggers the failed `保存并切换 EdgeOne 到此源站` path and renders the formal destructive alert with a stronger clay-native error peak, mode-specific recovery guidance, auto-focus on the failure region, a clearer raw-response disclosure, and de-emphasized footer actions while the failure is present. The captured dialog screenshot is stored at `docs/specs/f36b4-edgeone-active-standby-ha/assets/ha-source-dialog-submit-failure-alert.png`, measured `672x952`, and bound to `60388584700da855bb1e015402aab9baa4951314`. `trim_whitespace.py` re-ran and reported `no_meaningful_whitespace`, so the original crop remained canonical.

## Validation

- `cargo fmt --check`
- `cargo check`
- `cargo test alerts_and_ha -- --nocapture`
- `cargo test ha_source_endpoint_accepts_lowercase_direct_origin_scheme`
- `cargo test standalone_ha_outbox_gc_deletes_expired_rows_across_channels_in_bounded_batches -- --nocapture`
- `cd web && bun run build`
- `cd web && bun test src/admin/HaSourceSettingsDialog.interaction.test.tsx src/components/HaStatusBanner.stories.test.tsx`
- `python3 -m py_compile tests/ha/scripts/*.py`
- Shared `codex-testbox` Docker Compose harness with mock EdgeOne, mock ingress, dual app nodes, and mock upstream:
  `pre -> failover -> recovery`.

## Integration Harness

- Added `tests/ha/docker-compose.yml` for mock EdgeOne, EdgeOne ingress, mock Tavily upstream, `node-a`, `node-b`, and `ha-test-runner`.
- Added `tests/ha/scripts/run_ha_acceptance.py` with staged acceptance checks:
  `pre`, `failover`, and `recovery`.
- Added Python mocks for EdgeOne origin describe/modify, single-entry ingress forwarding, and Tavily/MCP upstream responses.
- The harness uses only mock upstreams and runs on `codex-testbox`; it does not call the production Tavily or EdgeOne endpoints.
