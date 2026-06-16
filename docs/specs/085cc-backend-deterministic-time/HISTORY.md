# 后端 deterministic time 演进历史（#085cc）

> 这里记录关键演进原因，避免把“为什么这么做”散落到 PR 评论与临时日志里。

## Decision Trace

- 2026-06-15：确认 `3grrf` 已经把 backend CI topology 和 shard coverage verifier 建好；当前慢测问题不再是“怎么拆更多 shard”，而是“测试本身依赖真实时间”。
- 2026-06-15：定位到一个结构性热点不是单个 sleep，而是 `initialize_schema()` 中 HA schema 初始化通过 pool 反复执行 DDL/trigger，单次启动可吃掉约 `5.4s`。
- 2026-06-15：据此将 HA schema 初始化改为单连接批量执行，先切掉最重 startup hotspot，再继续移除测试中的真实等待。
- 2026-06-15：确认把整条测试 runtime 直接改成 `start_paused` 会冻结 `sqlx` 相关 timeout/等待，导致 `PoolTimedOut` 一类伪回归；后续策略改为“局部可控时钟 + 显式持久化时间戳”，而不是对整类 DB 测试一刀切 paused runtime。
- 2026-06-15：`scripts/ci_backend_tests.py verify` 曾稳定卡死在 `cargo test --locked --all-features --bin tavily-hikari -- --list`；改为 `--no-run` 后直接对 test executable 做 `--list`，恢复 coverage 证明能力。
- 2026-06-16：确认当前剩余大头不再是“真实 sleep”，而是 shard runner 逐条 `--exact --test-threads=1` 造成的 test executable 重复启动与过滤开销；因此把 `run-shard` 提升为“安全前缀批量执行 + 少量 exact fallback”的混合模式。
- 2026-06-16：在前缀批量执行后，新的关键路径收敛到 `forward_proxy::tests::` 干扰和 `bin-admin-api` 过高进程并发；因此把 `forward_proxy::tests::` 独立成 `lib-forward-proxy` shard，并给 shard runner 增加 per-shard `filtered_process_workers`，用更小的局部并发换取稳定的总墙钟下降。

## Boundary Notes

- 本主题不替代 `3grrf`。`3grrf` 负责 CI job topology、artifact reuse、stable aggregate gate、manifest-driven shard coverage；`085cc` 负责让这些 shard 里的测试不再靠真实 wall-clock 慢吞吞跑完。
- 本主题也不替代 `2wdrp`。`2wdrp` 约束生产语义里的 SQLite transient lock hardening；`085cc` 约束时间基础设施与测试写法。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
