# 上游不可知 API 负载均衡实现状态（#cp8s9）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现，等待 PR review/CI 收敛
- Lifecycle: active
- Catalog note: `/api/tavily/*` 通过 System Settings 开关与按请求比例进入 generic API rebalance selector，默认关闭 0%；Hikari routing key、Tavily adapter 兼容与 research lifecycle pinning 已接入。

## Coverage / rollout summary

- `SystemSettings` 持久化 `api_rebalance_enabled` 与 `api_rebalance_percent`，默认 `false / 0`。
- `/api/tavily/search|extract|crawl|map|research` 在开关开启且本请求随机 bucket 命中时使用 `api_rebalance_http` selector；否则走 legacy primary / Tavily adapter 选路。
- 命中 API Rebalance 且无 routing key 的 API 请求使用 full-pool selector，不默认落到 user/token primary affinity。
- `X-Hikari-Routing-Key` 被解析为本地 routing subject，转发上游前剥离。
- `X-Project-ID` 在 API Rebalance 命中且没有 Hikari routing key 时作为 Tavily adapter fallback routing subject，并继续透传上游；未命中 API Rebalance 时保持 legacy HTTP project affinity。
- `POST /api/tavily/research` 成功后记录 `request_id -> 实际 key_id`；`GET /api/tavily/research/:request_id` 不参与 rollout gating，只使用创建时记录的 key，记录 key 不可用时返回错误且不 fallback。
- System Settings UI 提供 API Rebalance Switch、0-100 slider 与数字输入；关闭时比例控件禁用。
- Request log / dashboard effect bucket 已接入 generic API binding 与 selection effect code。

## Remaining Gaps

- GitHub PR CI 仍需远端收敛。

## Related Changes

- Rust:
  - `src/tavily_proxy/proxy_affinity.rs`
  - `src/tavily_proxy/proxy_http_and_logs.rs`
  - `src/server/handlers/tavily.rs`
  - `src/store/key_store_keys.rs`
  - `src/store/key_store_request_logs_and_dashboard.rs`
  - `web/src/admin/SystemSettingsModule.tsx`
  - `web/src/api/runtime.ts`
- Tests:
  - `src/tests/chunk_07.rs`
  - `src/server/tests/chunk_03.rs`
  - `src/server/tests/chunk_08.rs`
  - `src/server/tests/chunk_09.rs`
  - `src/server/tests/chunk_11.rs`
  - `src/server/tests/chunk_12.rs`
  - `web/src/admin/SystemSettingsModule.stories.tsx`

## Validation

- `cargo fmt --check`
- `cargo test api_rebalance -- --nocapture`
- `cargo test tavily_http_search_hikari_routing_key_is_internal_and_takes_affinity_precedence -- --nocapture`
- `cargo test tavily_http_search_forwards_raw_x_project_id_and_logs_api_route_affinity_effect -- --nocapture`
- `cargo test tavily_http_search_default_rollout_uses_legacy_project_affinity_effect -- --nocapture`
- `cargo test tavily_http_search_api_rebalance_percent_100_uses_generic_selector -- --nocapture`
- `cargo test tavily_http_search_api_rebalance_percent_zero_uses_legacy_primary -- --nocapture`
- `cargo test tavily_http_research_result_returns_error_when_pinned_key_unavailable_without_fallback -- --nocapture`
- `cargo test tavily_http_research_result -- --nocapture`
- `cargo test admin_system_settings_reject_invalid_api_rebalance_percent -- --nocapture`
- `bun test web/src/admin/SystemSettingsModule.render.test.ts web/src/admin/SystemSettingsModule.stories.test.ts web/src/api.test.ts`
- `bun --bun ./node_modules/.bin/tsc -b`
- `bun --bun ./node_modules/.bin/storybook build --disable-telemetry`
- `cargo test tavily_http_search_dev_open_admin_fallback_keeps_project_header_without_primary_pin -- --nocapture`
- `cargo test http_project_affinity -- --nocapture`
- `cargo test unknown_403 -- --nocapture`
- `cargo test successful_request_clear_links_transient_backoff_maintenance_to_request_log -- --nocapture`
- `cargo test research_result_get_429_still_arms_mcp_session_init_backoff -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `codex -m gpt-5.5 -c model_reasoning_effort="low" --sandbox read-only -a never review --base origin/main`

## References

- `./SPEC.md`
- `./HISTORY.md`
