# Monitoring dashboard metrics catalog

本目录是首版 `hikari_` runtime metrics 的语义真相源。所有指标都由真实 source event 更新，Prometheus 负责历史，业务 SQLite 不保存这些时间序列。

## Common labels

所有指标都带有部署固定的 `service`、`environment`、`instance` const labels；其余适用指标最多使用以下固定 labels：`route_family`、`method`、`status_class`、`outcome`、`operation`、`phase`、`workload_class`、`channel`、`reason`、`upstream_class`。

未被某个指标明确列出的 label 不得添加。label value 必须来自代码内固定枚举或 deployment allowlist；不得包含 user/token/key/IP/request-id/trace-id、原始 URL/query/body、SQL 文本/参数、header 值、secret 或任意错误文本。

## Enum contract

- `route_family`: `mcp`, `http_search`, `http_extract`, `http_crawl`, `http_map`, `http_research`, `admin`, `user`, `public`, `internal`。
- `phase` on `hikari_http_duration_seconds`: `none`, `admission`, `key_selection`, `upstream`, `response_headers`, `response_body`, `persistence`, `ack`。
- `phase` on `hikari_http_request_duration_seconds`: `total`, `configuration`, `billing_lock_wait`, `preflight`, `key_selection`, `upstream`, `response_body`, `persistence`, `settlement`, `reconciliation_usage`, `reconciliation_schedule`, `other`。
- `operation` on `hikari_http_request_duration_seconds`: `search`, `extract`, `crawl`, `map`, `research`, `admin`, `user`, `public`, `internal`, `mcp_control`, `mcp_business`, `mcp_unknown`, `other`。
- `outcome`: `success`, `error`, `timeout`, `cancelled`, `rate_limited`, `upstream_exhausted`, `degraded`。
- `status_class`: `2xx`, `3xx`, `4xx`, `5xx`, `none`。
- `method`: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OTHER`, `SSE`, `MCP`。
- `upstream_class`: `tavily_http`, `tavily_mcp`, `forward_proxy`, `none`; `forward_proxy` identifies relay-candidate attempts, while direct fallback remains classified by its Tavily operation class。
- `workload_class`: `foreground`, `background`, `maintenance`, `reconciliation`, `observability`。
- `channel`: `control`, `billing`, `usage`, `research`, `none`。
- `reason`: fixed reason codes only, including `coverage_readiness`, `source_baseline`, `native_query_deadline`, `pool_pressure`, `writer_contention` and `other` for an unrecognized internal reason; implementation must never emit free text.

## Application metrics

| Name                                   | Type              | Unit         | Allowed labels                                            | Source event                    | Meaning                                                                                                                               |
| -------------------------------------- | ----------------- | ------------ | --------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `hikari_http_requests_total`           | Counter           | requests     | `route_family`, `method`, `status_class`, `outcome`       | request terminal event          | Completed or terminally rejected HTTP/MCP request count.                                                                              |
| `hikari_http_duration_seconds`         | classic Histogram | seconds      | `route_family`, `method`, `outcome`, `phase`              | request phase completion        | Bounded request or phase duration; `phase=none` represents total duration.                                                            |
| `hikari_http_request_duration_seconds` | classic Histogram | seconds      | `route_family`, `method`, `operation`, `outcome`, `phase` | request terminal event          | One atomic terminal event containing `phase=total` and all 11 mutually exclusive request leaf phases; zero-value phases are exported. |
| `hikari_upstream_requests_total`       | Counter           | attempts     | `upstream_class`, `operation`, `outcome`, `status_class`  | response-header completion      | Upstream attempts that received response headers; response-body failures remain visible in the request phase metrics.                 |
| `hikari_upstream_duration_seconds`     | classic Histogram | seconds      | `upstream_class`, `operation`, `outcome`, `phase`         | upstream attempt completion     | Time spent in a bounded upstream phase.                                                                                               |
| `hikari_sqlite_wait_duration_seconds`  | classic Histogram | seconds      | `workload_class`, `operation`, `reason`                   | pool/begin/lock wait completion | Time spent waiting for SQLite or its coordinator.                                                                                     |
| `hikari_sqlite_transactions_total`     | Counter           | transactions | `workload_class`, `operation`, `outcome`                  | transaction completion          | SQLite transaction outcomes without SQL text or parameters; admission-deferred work is excluded.                                      |
| `hikari_sqlite_admission_defers_total` | Counter           | work items   | `workload_class`, `operation`, `reason`                   | transaction admission decision  | SQLite work deferred before a transaction begins, with a fixed defer reason.                                                          |
| `hikari_background_tasks_total`        | Counter           | tasks        | `operation`, `outcome`, `reason`                          | task lifecycle event            | Bounded background, maintenance or reconciliation task outcomes.                                                                      |

## Runtime and observability metrics

| Name                                 | Type    | Unit    | Allowed labels      | Source event                | Meaning                                                                                                            |
| ------------------------------------ | ------- | ------- | ------------------- | --------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `hikari_process_cpu_seconds_total`   | Counter | seconds | none                | bounded runtime sample      | Process CPU time accumulated by the service.                                                                       |
| `hikari_process_memory_bytes`        | Gauge   | bytes   | `reason`            | bounded runtime sample      | Current process or cgroup memory measurement; `reason` distinguishes `rss`, `limit`, `headroom`, `swap` and `hwm`. |
| `hikari_observability_flush_total`   | Counter | flushes | `outcome`, `reason` | recorder flush              | Recorder flush success, defer, timeout or error.                                                                   |
| `hikari_observability_dropped_total` | Counter | samples | `reason`            | bounded queue/drop decision | Samples dropped because of configured queue, cardinality or timeout limits.                                        |

## Histogram rules

首期应用 Histogram 使用 classic `_bucket`、`_sum` 和 `_count` series；dashboard 与 recording rules 必须在聚合时保留 `le`，再使用 `histogram_quantile`。native Histogram、hotpath function/sql attribution 和 trace-derived metrics 不属于本目录。

## Cardinality and cost limits

- 每个 metric 的 label key 集合是静态的；unknown key/value MUST be rejected or mapped to a fixed `unknown` outcome before recording。
- recorder 对不同 label tuple 设置进程内上限；达到上限时增加 `hikari_observability_dropped_total`，不能继续扩展 map。
- 每个请求作用域只提交一个有界终态队列项；队列溢出时总耗时与全部阶段一起丢弃，不能产生部分阶段样本。
- 请求阶段来自同一 scope 的嵌套计时；进入子阶段会暂停父阶段，重复阶段累计，未进入阶段记录为零，取消使用固定 `cancelled` outcome。
- metrics exposition、flush 和 scrape handler 必须有独立超时；不得在请求 hot path 同步等待 Prometheus 或 Grafana。
- 当前数值上限、scrape interval、retention、alert threshold 和性能预算由部署实例配置并单独验收，本文件不规定 101 专属数字。
