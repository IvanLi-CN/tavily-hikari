# jspgt · Forward Proxy GEO 负缓存与 24h 刷新

## Summary

- 为 forward proxy runtime 的 GEO 元数据增加持久化时间戳与 `negative` 占位来源，避免注册 IP 亲和路径反复 trace 同一批无 GEO 结果节点。
- 将请求路径的 GEO 补全限制为“仅修复从未完成缓存或历史脏数据”，批量 API Keys 入库不再额外同步整池 GEO 预热，避免 handler 把导入请求卡住。
- 新增独立 scheduler，每 24 小时批量刷新全部非 Direct 节点的 GEO 元数据，并将结果记录到 `scheduled_jobs`。

## Functional/Behavior Spec

### GEO cache semantics

- `forward_proxy_runtime` 新增 `geo_refreshed_at`，默认值为 `0`。
- `resolved_ip_source` 语义调整为：
  - `trace`：已成功拿到出口 IP；可带一个或多个 `resolved_ips` 与 `resolved_regions`；若暂时拿不到 region，允许保留 `resolved_ips` 并在后续懒刷新中继续补 region。
  - `negative`：trace 失败后写入的正式持久化占位缓存；默认不携带可用于匹配的 GEO 数据。
  - `""`：仅兼容历史数据，视为未完成缓存。
- 请求路径只把“`geo_refreshed_at = 0`、`resolved_ip_source` 为空、或 `trace` 仅有 `resolved_ips` 但还没有 `resolved_regions`”的 runtime 行视为待修复；对最后一种情况，仅当 `resolved_ips` 里仍有可用的 global GEO IP 时，请求路径才只重试 region 补全，否则必须重新 trace。
- `negative` 且 `geo_refreshed_at > 0` 的 runtime 行会作为占位缓存持久化，但请求路径只在短冷却窗口内直接复用；冷却窗口过后，下一次 registration-aware 请求可再次尝试 trace/GEO 修复。
- GEO 元数据落库只能更新 `resolved_ip_source` / `resolved_ips` / `resolved_regions` / `geo_refreshed_at`，不得覆盖 weight、EMA、failure 计数等运行时健康字段。

### Request-path behavior

- registration-aware 代理亲和选择继续使用 forward proxy GEO 元数据。
- `create_api_keys_batch` 不再额外同步整池 GEO 预热；批内的 registration-aware 选择依赖持久化缓存与短冷却重试机制，避免在 handler 里先做一轮额外全池阻塞工作。
- lazy request-path 若 GEO 落库暂时失败，进程内 runtime 仍要保留本次解析出的 GEO 结果，避免在 SQLite busy/locked 时把 trace/GEO 请求风暴放大。
- hint-only 导入不触发 GEO 预热，也不为节点写入 GEO 占位数据。
- legacy host-based / 空 source / 无 timestamp 的历史 runtime 行，在首次命中时会被修复成 `trace` 或 `negative`。

### Scheduled refresh

- 新增 `forward_proxy_geo_refresh` 定时任务。
- 周期固定为 24 小时。
- scheduler 需要周期性重算剩余 TTL；若现有 non-Direct 节点 GEO 元数据仍缺失/不完整，或已过期（>=24h），需立即补跑首轮刷新；否则只等待当前剩余 TTL，并在后续通过短周期 recheck 避免新增/变更节点把首轮刷新拖到原先的 24h deadline 之后。
- 对“刚刷新过但仍缺 region 的 trace 结果”不能进入无休眠热循环；这类 incomplete runtime 只有在仍持有 global GEO IP 时才遵守短冷却退避，loopback/RFC1918 等不可用 trace 结果必须立即重试。
- 每轮刷新全部非 Direct 节点：
  - trace 成功则写回 `trace` 和新的 `geo_refreshed_at`。
  - trace 失败则写回 `negative`、空 `resolved_ips`/`resolved_regions`，并更新时间戳。
- 每轮任务都写入 `scheduled_jobs`，便于后台排查。

## Acceptance

- 对同一无 GEO 节点，第一次 registration-aware 请求写入 `negative` 占位；后续请求不再重复 trace。
- batch 导入带 registration metadata 时，不会在 handler 入口先额外做一轮整池 GEO 预热。
- hint-only batch 导入不会修改 forward proxy runtime 的 GEO 字段。
- `forward_proxy_geo_refresh` 任务会刷新全部非 Direct 节点，并保留 Direct 的空 GEO 状态不变。

## Verification

- `cargo check -q`
- `cargo test -q forward_proxy_ -- --nocapture`
- `cargo test -q api_keys_batch_ -- --nocapture`
- `cargo test -q forward_proxy_geo_refresh_job_ -- --nocapture`
