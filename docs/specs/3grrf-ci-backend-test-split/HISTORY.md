# GitHub Actions 后端测试拆分与并行提速 演进历史（#3grrf）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-06-07：建立新 spec，锁定“两段 stacked PR、先 CI 拓扑提速、后 job/matrix 并行、不减少测试数量”的实施边界。
- 2026-06-07：确认当前 `main` 无 GitHub branch protection，但 reviewer 仍依赖 `Backend Tests` 作为 owner-facing 总体 backend gate，因此拆分时必须保留稳定 aggregate check。
- 2026-06-07：确认当前 `cargo test --lib` / `cargo test --bins` 中的大量测试仍集中在共享命名空间；PR2 优先使用 shard manifest + coverage verifier，而不是先引入新 runner。
- 2026-06-07：验证了 `libtest` 的 `FILTER` 与 `--skip FILTER` 默认都是子串匹配；直接用 `cargo test FILTER` 或 `--skip FILTER` 做 shard 容易出现 overlap / false match。
- 2026-06-07：据此改为“manifest 负责测试归属，执行器先拿 test executable，再按精确测试名列表用 `--exact` 直接运行测试二进制”的方案，避免为了并行而重组大量测试源码。

## Key Reasons / Replacements

- 该主题新增的直接原因是 `CI Pipeline` 关键路径长期接近 1 小时，且结构性浪费主要来自单长 backend job、重复 frontend build 与不必要的 downstream `needs` 阻塞。
- 该 spec 不替代 release / docs-pages 相关 spec；它只约束 PR `CI Pipeline` 下的 backend split 与 safe parallelization。
- 早期实现阶段曾放弃“先把所有 `chunk_*.rs` 机械模块化再靠命名空间切 shard”的方向，因为当时 `src/tests/**` 与 `src/server/tests/**` 里存在真实跨文件 helper 依赖，贸然拆模块会破坏可见性并扩大改动面。
- 2026-06-18：测试组织进一步收口为真实语义模块 + 显式 `support` 层；`src/tests/mod.rs` 与 `src/server/tests.rs` 不再依赖 `include!(\"chunk_*.rs\")`，shard selector 也同步切到稳定的模块前缀，而不是继续绑定预算驱动的机械切片文件名。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
