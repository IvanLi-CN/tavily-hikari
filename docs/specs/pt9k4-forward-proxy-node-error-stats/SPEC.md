# Forward Proxy Node Error Statistics

Spec ID: pt9k4

## Scope

Add an admin-facing error statistics view for forward proxy nodes and support persistent node enablement overrides.

## Requirements

- The admin forward proxy panel has a two-option view switcher: node pool and error statistics.
- Error statistics aggregate by forward proxy node and count only real requests. Probe, validation, subscription refresh, and maintenance attempts are excluded from error statistics.
- Fixed windows are `1m`, `15m`, `1h`, `1d`, and `7d`.
- Each window reports `totalCount`, `errorCount`, and `errorRate`.
- The 24 hour view reports:
  - hourly activity buckets with successful request count and error counts by normalized error type.
  - total request count, total error count, and 24 hour error rate.
  - distribution buckets for the error pie chart.
- Known normalized error types are:
  - `proxy_unreachable`
  - `send_error`
  - `validation_failed`
  - `upstream_unknown_403`
  - `upstream_rate_limited_429`
  - `upstream_usage_limit_432`
  - `upstream_gateway_5xx`
  - `transport_send_error`
  - `unknown`
- Error statistics rows are sorted by 24 hour error rate descending. Nodes with zero 24 hour requests sort after nodes with measurable traffic.
- Node disable overrides are persisted by `proxy_key` and survive runtime row pruning during subscription refresh.
- Disabled nodes are excluded from real request routing candidates.
- The admin UI supports row selection in both the node pool and error statistics views.
- The floating bulk action bar supports selecting all rows in the current view, inverting selection in the current view, disabling selected nodes, and enabling selected nodes.

## UI

- Node pool remains the default view.
- Error statistics columns are `节点`, `1分钟`, `15分钟`, `1小时`, `1天`, `7天`, `24小时活动`, and `24小时错误分布图`.
- Empty request windows render as `-`.
- Error window cells render failure percentage and failure count.
- The 24 hour activity mini timeline uses low contrast background for successful traffic and stronger colors for error mix.
- The 24 hour error distribution chart uses a compact pie visualization with details available in hover text.

## API

- `GET /api/stats/forward-proxy/errors` returns the aggregated error statistics payload.
- `POST /api/settings/forward-proxy/nodes/state` accepts `{ proxyKeys: string[], disabled: boolean }` and returns per-node update results.

## Validation

- Rust tests cover real-request filtering, error classification, default sorting, routing exclusion for disabled nodes, and persistence across runtime sync.
- Storybook mocks cover node pool, error statistics, empty data, high error rate, unknown errors, long names, selected rows, bulk action bar, disabled rows, and mobile horizontal scrolling.

## Visual Evidence

- source_type: storybook_canvas
  story_id_or_title: Admin/ForwardProxySettingsModule/ErrorStatistics
  state: error statistics
  evidence_note: verifies the node pool switcher, error statistics table, 24 hour activity bars, pie distribution, disabled node badge, and floating bulk action bar surface.

![Forward proxy error statistics story](./assets/error-statistics-story.png)
