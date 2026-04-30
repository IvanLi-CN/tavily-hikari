# Request Logs Body 压缩实现

## 实现摘要

- 新增 `zstd` 依赖与 request-log body codec helper。
- `log_attempt` 写入前先计算 request kind、计费派生值与 value bucket，再按阈值压缩 request/response body。
- request log row mapper 与 body detail mapper 按 `*_body_codec` 透明解压，API 输出仍是原始字符串或 `null`。
- request log list/catalog/dashboard SQL 对 canonical 行优先使用 `counts_business_quota` 与 `request_value_bucket`，避免热路径读取压缩正文。
- 历史迁移 scheduler 在服务启动后按批次处理 request_logs，进度记录到：
  - `meta.request_log_body_compression_cursor_v1`
  - `meta.request_log_body_compression_done_v1`
  - `scheduled_jobs.job_type = request_log_body_compression_migration`

## 验证

- `cargo check`
- `cargo test request_log_large_bodies_are_zstd_compressed_and_read_transparently -- --nocapture`
- `cargo test request_log_body_migration_compresses_legacy_rows_and_preserves_classification -- --nocapture`
