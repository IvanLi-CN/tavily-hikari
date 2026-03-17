# jspgt · Forward Proxy GEO 负缓存与 24h 刷新

## Summary

- 为 forward proxy runtime 的 GEO 元数据增加持久化时间戳与 `negative` 占位来源，避免注册 IP 亲和路径反复 trace 同一批无 GEO 结果节点。
- 将请求路径的 GEO 补全限制为“仅修复从未完成缓存或历史脏数据”，批量 API Keys 入库在单批内预热一次 GEO 缓存，不再逐 key 重跑整池 trace。
- 新增独立 scheduler，每 24 小时批量刷新全部非 Direct 节点的 GEO 元数据，并将结果记录到 `scheduled_jobs`。

## Functional/Behavior Spec

### GEO cache semantics

- `forward_proxy_runtime` 新增 `geo_refreshed_at`，默认值为 `0`。
- `resolved_ip_source` 语义调整为：
  - `trace`：已成功拿到出口 IP；可带一个或多个 `resolved_ips` 与 `resolved_regions`。
  - `negative`：本轮 trace/GEO 解析失败，作为正式持久化占位缓存。
  - `""`：仅兼容历史数据，视为未完成缓存。
- 请求路径只把“`geo_refreshed_at = 0` 或 `resolved_ip_source` 为空”的 runtime 行视为待修复；一旦 runtime 行带有 `trace` 或 `negative` 且 `geo_refreshed_at > 0`，后续请求直接复用缓存，不再同步 trace。

### Request-path behavior

- registration-aware 代理亲和选择继续使用 forward proxy GEO 元数据。
- `create_api_keys_batch` 在单批内只做一次 forward proxy GEO 预热，随后复用缓存处理该批所有带 registration metadata 的 key。
- hint-only 导入不触发 GEO 预热，也不为节点写入 GEO 占位数据。
- legacy host-based / 空 source / 无 timestamp 的历史 runtime 行，在首次命中时会被修复成 `trace` 或 `negative`。

### Scheduled refresh

- 新增 `forward_proxy_geo_refresh` 定时任务。
- 周期固定为 24 小时。
- 每轮刷新全部非 Direct 节点：
  - trace 成功则写回 `trace` 和新的 `geo_refreshed_at`。
  - trace 失败则写回 `negative`、空 `resolved_ips`/`resolved_regions`，并更新时间戳。
- 每轮任务都写入 `scheduled_jobs`，便于后台排查。

## Acceptance

- 对同一无 GEO 节点，第一次 registration-aware 请求写入 `negative` 占位；后续请求不再重复 trace。
- batch 导入带 registration metadata 时，forward proxy GEO 只会在该批开始前预热一次。
- hint-only batch 导入不会修改 forward proxy runtime 的 GEO 字段。
- `forward_proxy_geo_refresh` 任务会刷新全部非 Direct 节点，并保留 Direct 的空 GEO 状态不变。

## Verification

- `cargo check -q`
- `cargo test -q forward_proxy_ -- --nocapture`
- `cargo test -q api_keys_batch_ -- --nocapture`
- `cargo test -q forward_proxy_geo_refresh_job_ -- --nocapture`
