# HTTP API

（本计划不引入新的 endpoint，但会改变统计实现与日志可见范围。为避免“清理后统计回退”，本契约强调语义保持。）

## Summary（GET /api/summary）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: existing（保持不变）

### 响应（Response）

- Success: schema 不变
- Semantics（语义）:
  - 累计类字段保持“全历史累计”语义，不受在线 `request_logs` retention 影响

### 兼容性与迁移（Compatibility / migration）

- 允许内部实现从“扫描 request_logs”迁移为 “rollup buckets”，但对外字段与语义需保持一致。
  - 允许内部实现从“扫描 request_logs”迁移为 “rollup buckets”，但对外字段与语义需保持一致。

## API keys list / metrics（GET /api/keys, GET /api/keys/:id/metrics）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: existing（保持不变）

### 响应（Response）

- Success: schema 不变
- Semantics（语义）:
  - `total_requests` / `success_count` / `error_count` / `quota_exhausted_count` 必须表示全历史累计

### 兼容性与迁移（Compatibility / migration）

- 若引入回填：
  - 回填完成前不得切换默认统计口径
  - 需要在实现中提供一致性校验与可回滚路径

## Recent logs（GET /api/logs, GET /api/keys/:id/logs）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: existing（保持不变）

### 响应（Response）

- Success: schema 不变（返回 recent logs）
- Semantics（语义）:
  - 默认返回“最近请求日志”（仍然可用于排障）
  - 清理后，在线热数据仅覆盖最近 N 天；不提供归档查询
