# 后端 deterministic time 实现状态（#085cc）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与验证事实。

## Current Status

- Implementation: completed
- Lifecycle: done
- Catalog note: backend deterministic time seam + slow-test de-wallclocking

## Implemented Now

- 新增 `src/backend_time.rs`
  - 统一 `now_ts()`、`now_utc()`、`deadline_after()`、`sleep()`。
  - 补充 test-only manual wall clock handle，用于显式推进持久化时间。
- `KeyStore`
  - 新增 `new_with_time` / `open_for_request_logs_gc_with_time` internal 构造入口。
  - SQLite transient write retry helper 已改为走 `BackendTime`。
  - request logs GC、scheduled jobs、quota subject lock 等关键慢路径已接入统一时间层。
- `TavilyProxy`
  - 新增 `with_options_and_time` internal 构造入口，并把 `BackendTime` 挂到 proxy 上。
  - MCP retry-after / polling 辅助逻辑已改走 `backend_time()`。
- 慢测根治
  - `server::tests::tavily_http_research_result_reuses_key_selected_by_research_create` 改为显式 seed `last_used_at`，不再依赖 `1.1s` 真等待。
  - `tests::quota_subject_lock_retries_transient_sqlite_write_lock` 不再初始化整套重型 schema，只创建 quota lock 所需的最小表/index。
  - `tests::quarantine_key_by_id_preserves_original_created_at` 去掉 `1s` 真等待。
  - `tests::select_proxy_affinity_reuses_negative_forward_proxy_runtime_geo_metadata`、`tests::select_proxy_affinity_retraces_non_global_trace_cache_without_regions` 去掉 `1s` 真等待，改为直接控制 `geo_refreshed_at`。
- coverage verifier
  - `scripts/ci_backend_tests.py verify` 改为先 build test executable，再直接对各二进制执行 `--list`，避开 `cargo test --bin tavily-hikari -- --list` 的 300s 卡死路径。
- shard runner throughput
  - `scripts/ci_backend_tests.py run-shard` 不再对所有 shard 一律逐条 `--exact --test-threads=1` 串行执行。
  - 对 substring 匹配与 prefix 命中集合完全等价的安全前缀，改为单次批量执行；仅把少数不安全前缀残留测试回退到 `--exact`。
  - `bin-admin-api`、`bin-tavily-http`、`bin-mcp-core`、`bin-linuxdo-forward`、`bin-ha-rest` 已可 100% 走批量前缀执行；`lib-account-user` 为 `90/92`，`lib-request-rollup` 为 `91/104`。
  - shard runner 现支持 `serial_prefixes`：对共享 xray/runtime 或仍含进程级全局状态的前缀单独降回串行，其余前缀继续并发。
  - `scripts/ci_backend_tests.py` 支持 shard 级 `filtered_test_threads`，当前 `lib-request-rollup` 维持 `2` 线程，但 `tests::request_` 前缀单独串行；`lib-core` 的 `forward_proxy::tests::` / `tavily_proxy::tests::` 也单独串行，其余 shard 维持 `1`。
  - `scripts/ci_backend_test_manifest.json` 现把 `forward_proxy::tests::` 拆成独立 `lib-forward-proxy` shard，并支持 shard 级 `filtered_process_workers`；当前 `bin-admin-api` 维持 `2` worker，避免恢复高并发 flake。
- build-once shard fanout
  - `scripts/ci_backend_tests.py prepare-artifacts` 现改为一次性构建全覆盖 test targets，再按 coverage target 拆分 artifact，不再按 `lib/bin/integration` 多次重复 `cargo test --no-run`。
  - prebuilt artifact 会缓存每个 executable 的 `tests.json`，后续 `verify --prebuilt-root` / `run-shard --prebuilt-root` 不再依赖下载后重新执行 `--list`。
  - `.github/workflows/ci.yml` 现改为 `backend-shard-plan` 上传 `backend-test-artifacts`，lib/bin/integration shards 统一下载并复用。
  - `prepare-artifacts` 现仅对真正需要 shard 覆盖证明的 executable 执行 `--list`，并将 listing 并发池收敛到 2，避免把机器打爆后把尾部 executable 排队到更慢。

## Measured Evidence

- lib representative:
  - `tests::quota_subject_lock_retries_transient_sqlite_write_lock`
  - exact binary wall time from `7.545s` 降到 `2.207s`
- bin representative:
  - `server::tests::tavily_http_research_result_reuses_key_selected_by_research_create`
  - exact binary wall time from `11.89s` 降到 `4.677s`
- startup hotspot:
  - `initialize_schema()` 中 `ha_schema` 由约 `5.4s` 降到约 `0.46s`
  - 根因是重复 pool/DDL 往返，而不是 request-kind migration 本身
- shard runner representative:
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-account-user`
  - old wall time: 串行 exact runner，约 `>180s`
  - new wall time: `122.38s`
- shard runner representative:
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-admin-api`
  - old wall time: 串行 exact runner，约 `>200s`
  - new wall time: `94.03s`
- shard runner representative:
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-request-rollup`
  - old wall time: 串行 exact runner，约 `>180s`
  - new wall time: `107.38s`
- build-once artifact representative:
  - old `prepare-artifacts` wall time: `139.47s`
  - mid-stage after first consolidation: `63.25s`
  - current wall time: `70.71s`
- latest runner-tuned artifact representative:
  - `prepare-artifacts` wall time: `58.71s`
- latest hot-cache artifact representative:
  - `python3 scripts/ci_backend_tests.py prepare-artifacts --output-dir <tmp>`: `2.74s`
  - artifact footprint: `293M`
- current critical-path representatives on prebuilt artifacts:
  - `python3 scripts/ci_backend_tests.py run-shard --id bin-admin-api --prebuilt-root ...`: `94.03s`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-request-rollup --prebuilt-root ...`: `62.95s`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-core --prebuilt-root ...`: `38.61s`
  - `python3 scripts/ci_backend_tests.py run-shard --id lib-forward-proxy --prebuilt-root ...`: `46.06s`
- latest stable backend benchmark on current head:
  - `python3 scripts/ci_backend_tests.py benchmark --max-workers 8`: `169.35s`
  - breakdown: `prepare_artifacts=2.33s`, `verify=0.00s`, `shards=167.01s`

## Remaining Gaps

- 当前 slowdown 相关的真实 wall-clock 热点已迁完；后续若继续扩面，应优先审计残留历史 `Utc::now()` / `tokio::time::sleep(...)` 是否仍属于行为路径，而不是继续加 shard 掩盖。
- `BackendTime` 的 manual clock 设施已就位，但并未把所有测试都强行切成 paused runtime；DB-heavy 测试仍需按需采用显式持久化时间戳或局部可控时钟。
- `lib-request-rollup` 的关键不稳点已收敛到 `tests::request_` 前缀串行运行；若要继续压时长，下一步应优先消除这组 GC / retention 测试中残留的真实 runtime / SQLite busy-time 依赖，而不是继续扩大线程数。
- `forward_proxy::tests::send_plan_recovers_after_shared_xray_exit*` 的高负载 flaky 已确认不是生产恢复逻辑回归，而是测试把“保存配置后计划立即可见”误当成契约；现已改为等待目标 proxy key 的 endpoint/runtime 真正变为 selectable，再断言 shared xray crash 后的 relay recovery。

## References

- `./SPEC.md`
- `./HISTORY.md`
