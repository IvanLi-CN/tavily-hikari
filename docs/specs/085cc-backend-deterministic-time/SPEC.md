# 后端 deterministic time 与真实时间依赖根治（#085cc）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## Summary

- 新建内部统一时间入口 `src/backend_time.rs`，作为后端行为性时间读取、sleep、deadline 与测试注入的唯一边界。
- 生产路径继续使用真实系统时间，不缩短任何生产 budget/backoff 常量，不变更 HTTP/CLI/schema 契约。
- 后端慢测不再依赖真实多秒 wall-clock 等待；测试优先改为显式时间戳控制、局部可注入时钟或 paused-runtime 可验证路径。
- 本主题只负责“行为时间基础设施 + 测试根治”，不替代 `3grrf` 的 CI topology，也不替代 `2wdrp` 的 SQLite 锁重试语义。

## Scope

- `src/backend_time.rs`
  - 提供统一 wall-clock / monotonic helper。
  - 提供 test-only manual wall clock handle，允许测试显式推进持久化时间。
- `src/store/**`
  - 所有会影响锁获取、GC、迁移 backfill、排序、过期判断、retry/backoff 的时间读取与 sleep 必须改走 `BackendTime`。
- `src/tavily_proxy/**`
  - key selection、cooldown、request-id affinity、quota sync、maintenance 触发判定中的行为时间必须改走 `BackendTime`。
- `src/server/**`
  - admin/manual job polling、retry-after、scheduler wait/recheck、job terminal polling 中的行为时间必须改走统一时间层。
- `src/forward_proxy/**`
  - GEO refresh、runtime cache freshness、validation readiness wait、window cache TTL 中的行为时间必须统一到 `BackendTime` 或由上层传入的 deterministic 时间。
- `src/tests/**` / `src/server/tests/**`
  - 删除依赖 1s/1.1s/5.2s 真等待的测试写法；改为明确的时间戳或可控时钟推进。

## Non-goals

- 不继续靠新增 CI shard、`cargo-nextest`、skip/ignore 测试来掩盖慢测。
- 不通过缩短生产 budget、修改公开构造器字段要求、引入 env-flag fast mode 来“假提速”。
- 不升级 `last_used_at` / `created_at` / `updated_at` 等 schema 精度。
- 不变更 owner-facing 项目入口文档、HTTP 接口、CLI 参数或数据库业务语义。

## Requirements

- 所有行为性 UTC 秒值必须通过 `BackendTime` 读取；业务代码不得再零散直读 `Utc::now()`。
- 所有行为性等待、deadline、poll/retry backoff 必须通过统一时间 helper 构造与消费；业务代码不得直接 `tokio::time::sleep(...)` 或直接依赖 `Instant::now()` 做状态推进。
- 允许保留的 direct `Instant::now()` 仅限纯日志/纯耗时观测。
- 允许保留的 direct `Utc::now()` / `Local::now()` 仅限经审计的纯展示、纯 DTO 文案或非行为性场景。
- `KeyStore::new`、`open_for_request_logs_gc`、`TavilyProxy::new/with_endpoint/with_options` 的公开签名保持不变；注入入口通过 internal/test-only helper 提供。
- 慢测中如仅需验证“同一秒内不应重写/重插/重排”的持久化行为，应优先使用显式时间戳控制，而不是依赖真实跨秒等待。
- `python3 scripts/ci_backend_tests.py verify` 必须持续可用，且 backend 总测试数量不得减少。

## Acceptance

- 后端产品代码中行为性时间入口已集中到 `src/backend_time.rs`，没有新的直接行为性 `Utc::now()` / `tokio::time::sleep(...)` 漏点继续扩散。
- 代表性慢测已去除真实多秒等待，至少覆盖：
  - SQLite transient lock 热点：`tests::quota_subject_lock_retries_transient_sqlite_write_lock`
  - 秒级排序/复用热点：`server::tests::tavily_http_research_result_reuses_key_selected_by_research_create`
- `scripts/ci_backend_tests.py verify` 在当前仓库能完成 lib/bin/integration coverage 校验，不再卡死在 `cargo test --bin tavily-hikari -- --list`。
- 同机性能证明至少覆盖一条 lib 慢测、一条 bin/server 慢测、以及当前最慢 backend shard，且纳入验收的对象墙钟下降不少于 50%。

## References

- `docs/specs/3grrf-ci-backend-test-split/SPEC.md`
- `docs/specs/2wdrp-sqlite-write-lock-hardening/SPEC.md`
- `docs/solutions/operations/sqlite-write-lock-contention.md`
