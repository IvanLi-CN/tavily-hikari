# 上游不可知 API 负载均衡实现状态（#cp8s9）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现，等待 PR review/CI 收敛
- Lifecycle: active
- Catalog note: `/api/tavily/*` 默认走 generic API rebalance selector，Hikari routing key 与 Tavily adapter 兼容已接入。

## Coverage / rollout summary

- `/api/tavily/search|extract|crawl|map|research` 默认使用 `api_rebalance_http` selector。
- 无 routing key 的 API 请求使用 full-pool selector，不再默认落到 user/token primary affinity。
- `X-Hikari-Routing-Key` 被解析为本地 routing subject，转发上游前剥离。
- `X-Project-ID` 在没有 Hikari routing key 时作为 Tavily adapter fallback routing subject，并继续透传上游。
- `/api/tavily/research/:request_id` 继续保留 request-id affinity 优先级，同时 API transient backoff 仍写入 `api_rebalance_http`。
- Request log / dashboard effect bucket 已接入 generic API binding 与 selection effect code。

## Remaining Gaps

- 全量 CI 与 review-loop 仍需在 PR 阶段收敛。
- 未新增 UI 文案；当前假设 Admin UI 的 effect fallback 能覆盖新增 code。

## Related Changes

- Rust:
  - `src/tavily_proxy/proxy_affinity.rs`
  - `src/tavily_proxy/proxy_http_and_logs.rs`
  - `src/server/handlers/tavily.rs`
  - `src/store/key_store_keys.rs`
  - `src/store/key_store_request_logs_and_dashboard.rs`
- Tests:
  - `src/tests/chunk_07.rs`
  - `src/server/tests/chunk_03.rs`

## Validation

- `cargo fmt --check`
- `cargo test api_rebalance -- --nocapture`
- `cargo test tavily_http_search_hikari_routing_key_is_internal_and_takes_affinity_precedence -- --nocapture`
- `cargo test tavily_http_search_forwards_raw_x_project_id_and_logs_api_route_affinity_effect -- --nocapture`
- `cargo test tavily_http_search_dev_open_admin_fallback_keeps_project_header_without_primary_pin -- --nocapture`
- `cargo test http_project_affinity -- --nocapture`
- `cargo test unknown_403 -- --nocapture`
- `cargo test successful_request_clear_links_transient_backoff_maintenance_to_request_log -- --nocapture`
- `cargo test research_result_get_429_still_arms_mcp_session_init_backoff -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

## References

- `./SPEC.md`
- `./HISTORY.md`
