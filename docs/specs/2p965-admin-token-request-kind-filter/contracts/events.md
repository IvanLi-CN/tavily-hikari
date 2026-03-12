# Events

## GET /api/tokens/:id/events

- `snapshot` 事件内的 `logs[]` 必须与 `TokenLogView` 字段语义一致。
- `snapshot.logs[]` 新增：
  - `request_kind_key`
  - `request_kind_label`
  - `request_kind_detail`

## Contract rules

- SSE snapshot 不负责下发 `request_kind_options`；前端在分页接口响应中维护可选项集合。
- 当第一页处于筛选状态时，收到 snapshot 后前端必须用当前筛选条件重新拉取 `/logs/page`，而不是直接信任 snapshot 的未过滤日志数组。
