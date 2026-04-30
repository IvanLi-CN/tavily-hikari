# Request Logs Body 压缩（#lbzst）

## 状态

- Status: 已实现（待审查）

## 背景 / 问题陈述

- 101 部署实例中 `request_logs` 表约 `5.50GB`，其中 `request_body/response_body`
  BLOB 合计约 `5.06GB`，主要来自 `/mcp` 响应正文。
- 线上样本压缩实验显示 zlib/zstd 类压缩后可降到约 `27%` 量级；SQLite 主库文件真正缩小仍需要单独维护窗口执行 `VACUUM`。
- 现有统计 SQL 会从 `request_body` 推导 `mcp:batch` 的计费/价值分类，直接压缩正文会破坏 dashboard、facets 与 operational class。

## 目标 / 非目标

### Goals

- 对 `request_logs.request_body` 与 `request_logs.response_body` 增加透明 zstd 压缩读写。
- legacy 未压缩行、plain 新行、compressed 新行必须混读。
- 新增派生列保存 `counts_business_quota` 与 `request_value_bucket`，热路径 SQL 优先读取派生列，避免依赖压缩后的正文。
- 服务启动后自动、小批次、可续跑地迁移历史 body，并把进度写入 `meta` 与 scheduled job message。
- 提供回退开关：
  - `REQUEST_LOG_BODY_COMPRESSION_ENABLED=false`
  - `REQUEST_LOG_BODY_MIGRATION_ENABLED=false`

### Non-goals

- 不提供正文 SQL 搜索或 FTS。
- 不自动执行 `VACUUM`。
- 不压缩 `auth_token_logs`。
- 不改变现有 HTTP API body 字段形状；detail endpoints 仍返回字符串或 `null`。

## 数据库合同

- `request_logs` 新增 body 编码元数据：
  - `request_body_codec TEXT`
  - `request_body_uncompressed_bytes INTEGER`
  - `response_body_codec TEXT`
  - `response_body_uncompressed_bytes INTEGER`
- `request_logs` 新增统计派生列：
  - `counts_business_quota INTEGER`
  - `request_value_bucket TEXT`
- `NULL` codec 表示 legacy/plain 原文。
- `zstd` codec 表示 BLOB 内容为 zstd 压缩字节，读取 detail/list-with-bodies 时必须先解压。
- 派生列为空的 legacy 行仍可按旧逻辑从 plain body 推导；压缩迁移会先补齐派生列再压缩。

## 运行时合同

- 新写入默认只压缩大于等于 `1024` bytes 的单个 body 字段。
- 若压缩后不小于原始字节数，保留 plain 存储。
- 自动历史迁移默认每批最多 `200` 行或 `32MiB` 原始 body，批间休眠 `2s`。
- 迁移失败只记录 scheduled job error；代理请求不因历史迁移失败而阻断。
- `REQUEST_LOG_BODY_COMPRESSION_ENABLED=false` 仅关闭新写入压缩。
- `REQUEST_LOG_BODY_MIGRATION_ENABLED=false` 关闭启动后的历史压缩任务。

## 验收标准

- 旧 plain `request_logs` 行无需迁移即可读取。
- 新写入超过阈值的 request/response body 以 `zstd` 存储，并通过 body detail、list include bodies、key/token/global body endpoints 返回原始正文。
- `mcp:batch` 全部非计费控制面请求保持 `non_billable` / `neutral` 分类。
- billable Tavily MCP tool call 和 API tool call 的 dashboard/value 分类保持不变。
- 历史迁移可被中断并按 cursor 续跑，不会二次压缩已压缩字段。
- 生产说明必须明确：压缩降低后续 DB/WAL 增长；历史主库文件实际缩小需要单独批准 `VACUUM`。
