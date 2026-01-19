# 数据库（DB）

## request_logs retention + rollup buckets

- 范围（Scope）: internal
- 变更（Change）: Modify
- 影响表（Affected tables）:
  - `request_logs`（在线热数据）
  - `scheduled_jobs`（新增 job_type 记录，现有表复用）
  - （新增）按 API key 的 rollup 桶表（见下方）

### Schema delta（结构变更）

#### 在线 DB（primary）

为保证“全历史累计”统计语义不因 `request_logs` retention 删除而回退，本计划采用“rollup 桶”作为统计数据源（少表 + 可细粒度）：

- 新增表：`api_key_usage_buckets`
  - 主键：`(api_key_id, bucket_start, bucket_secs)`
  - 字段：
    - `api_key_id TEXT NOT NULL`（引用 `api_keys(id)`）
    - `bucket_start INTEGER NOT NULL`（bucket 起始时间戳，秒）
    - `bucket_secs INTEGER NOT NULL`（bucket 粒度，秒；例如 `3600`/`86400`）
    - `total_requests INTEGER NOT NULL`
    - `success_count INTEGER NOT NULL`
    - `error_count INTEGER NOT NULL`
    - `quota_exhausted_count INTEGER NOT NULL`
    - `updated_at INTEGER NOT NULL`
  - 索引：
    - 主键已覆盖按 key+时间范围查询
    - 如需全局按时间范围聚合：可加 `(bucket_start DESC)`（可选）

bucket 对齐规则（与 `contracts/config.md` 保持一致）：

- `bucket_start` 必须按服务器本地时区对齐到自然边界：
  - `bucket_secs=86400`：对齐到本地 00:00
  - `bucket_secs=3600`：对齐到本地整点

写入路径（实现阶段细化）：

- 每次插入 `request_logs` 时，同一事务内对当前 bucket 做 UPSERT 增量更新（按本次请求的 `result_status` 增加对应计数）。
- 对历史已有数据：提供一次性回填（Backfill），把现有 `request_logs` 聚合到 `api_key_usage_buckets`，并记录在 `meta` 表中避免重复回填。

### Migration notes（迁移说明）

- 向后兼容窗口（Backward compatibility window）:
  - HTTP API 输出字段保持不变；若新增字段需保持旧字段语义一致（全历史累计）。
- 发布/上线步骤（Rollout steps）:
  - 先落地 `api_key_usage_buckets` 与一次性回填（不启用删除）。
  - 切换统计口径到 `api_key_usage_buckets`（保持 API 输出字段与语义不变）。
  - 最后启用 `request_logs` retention 删除（每日定时任务）。
- 回滚策略（Rollback strategy）:
  - 可关闭定时任务（或将 retention 设置为极大），停止进一步删除。
  - 统计切换需可回滚到旧实现（在数据一致性校验通过前；但启用删除后，旧实现将不再具备全历史语义）。
- 回填/数据迁移（Backfill / data migration）:
  - 一次性从现有 `request_logs` 回填累计统计，并对照校验（清理前后统计一致）。
