# GitHub Actions 后端测试拆分与并行提速 演进历史（#3grrf）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-10-07：request-statistics recovery CLI 增加独立 integration shard 后，固定十六 lane 的总估算超过原 120 秒容量 6 秒；将单 lane 估算预算校准为 125 秒，保留所有测试估算的 headroom 与既有并发拓扑。
- 2026-06-07：建立新 spec，锁定“两段 stacked PR、先 CI 拓扑提速、后 job/matrix 并行、不减少测试数量”的实施边界。
- 2026-06-07：确认当前 `main` 无 GitHub branch protection，但 reviewer 仍依赖 `Backend Tests` 作为 owner-facing 总体 backend gate，因此拆分时必须保留稳定 aggregate check。
- 2026-06-07：确认当前 `cargo test --lib` / `cargo test --bins` 中的大量测试仍集中在共享命名空间；PR2 优先使用 shard manifest + coverage verifier，而不是先引入新 runner。
- 2026-06-07：验证了 `libtest` 的 `FILTER` 与 `--skip FILTER` 默认都是子串匹配；直接用 `cargo test FILTER` 或 `--skip FILTER` 做 shard 容易出现 overlap / false match。
- 2026-06-07：据此改为“manifest 负责测试归属，执行器先拿 test executable，再按精确测试名列表用 `--exact` 直接运行测试二进制”的方案，避免为了并行而重组大量测试源码。
- 2026-08-20：Actions 原生计时显示 backend plan 到 aggregate 的稳定区间仍接近十分钟，且旧 fan-out 把编译环境、web artifact 和大体积 executable 重复带入每个 shard。后续 topology 改为一次 `ci-test` 编译、checksum-addressed bundle 和最多 16 个 LPT lanes；五分钟目标只约束该 backend 区间，不扩展为新的 required performance gate。
- 2026-08-20：首次 16-lane 运行证明辅助二进制需要保持 Cargo 的 sibling-name 前缀，并暴露出 alert projection 的并行干扰与几个被低估的长 shard。bundle 改为单副本的 `source-name-SHA256` 文件名；manifest 随实测权重细分 rollup integrity、alert projection、reconciliation，并以 shard 自身的线程上限隔离敏感测试。
- 2026-08-20：后续原生计时显示 account lifecycle 在全局单线程下拖慢 lane；执行器将请求线程数视为上限而非覆盖值。CI 传入两线程，只有 manifest 显式允许的 shard 使用两线程，alert projection 等敏感 shard 保持单线程。
- 2026-08-20：reporting shard 中的后台 flush 协调测试在同一 test process 内触发超时；该 prefix 改为逐条串行 `--exact`，隔离进程级后台状态，不修改测试断言、生产超时或重试语义。
- 2026-08-20：affinity domain 先前被默认单线程限制而形成 lane 长尾；该 shard 明确允许两条测试线程，已知敏感 shard 仍保持单线程。prepare job 使用 Ubuntu `mold` linker 替代未带来收益的 `lld`，缩短冷构建链接阶段而不改变 Cargo test profile 或覆盖集合。
- 2026-08-20：coverage verifier 在下一轮执行中捕获 rollup storage 与 integrity selector 的重叠；移除旧 selector 后由契约测试固定该互斥边界。CI prepare 的一次性构建显式使用四个 Cargo jobs，开发默认资源仍保持 `2/1/2`。
- 2026-08-20：Actions 原生步骤计时表明完整 executable bundle 的下载只占少量时间，而 lane checkout
  位于关键路径。bundle 因此携带 runner、manifest 与 `src`/`tests` source snapshot，并把 snapshot 链接到
  compile-time manifest directory，使运行期读取 repository sources 的既有测试不再要求 checkout。
- 2026-08-24：当前 head 的 review 发现 no-checkout lane 无法满足测试二进制编译期固定的
  `CARGO_MANIFEST_DIR`。lane 恢复默认 workspace 的 source checkout，仅用于提供 `src`/`tests`；
  不恢复 Rust/Bun/toolchain 安装，也不重新编译 bundle。
- 2026-08-20：MCP rebalance 审计 flush 测试在与其它 MCP prefix 进程并行时超过其既有等待预算；该 prefix
  作为整体改为串行执行。admin resources group 保持原有两进程上限，同时显式允许两条测试线程以缩短其最长 lane。
- 2026-08-20：原生 Actions 数据显示 16-way fan-out 在共享 workflow 配额下产生多分钟启动排队，而十个
  lanes 会把一个组合 lane 拉成长尾；CI 因此收敛为十二个 LPT lanes。prepare 仅提高 CI 专用 Cargo 编译并发
  至八并使用 512 codegen units，开发默认 `2/1/2` 不变。
- 2026-08-21：原生计时进一步暴露 MCP billing 与 research 前缀合并后的单 lane 长尾；manifest 将 MCP
  覆盖拆成 billing、rebalance-session、rebalance-control、research、system 五个互斥 shard，并恢复固定十六 lane 装箱以保持单 lane 预算。
- 2026-08-21：原生冷构建数据显示八个 Cargo jobs 与 512 codegen units 增加 prepare 尾部；恢复四个
  CI 专用 Cargo jobs 与继承 test profile 的默认 codegen 配置，开发期资源边界保持不变。
- 2026-08-21：`user_business_calls_1h::user_` 在同一过滤进程并行执行时出现共享状态竞争；该互斥
  语义前缀改为单进程串行过滤，保持覆盖与断言不变。
- 2026-08-21：`mcp_rebalance_and_follow_up::mcp_` 的实测长尾超过 manifest 估计；按 session/control
  语义拆成两个互斥 shard，避免单个测试进程拖慢 lane，同时保持完整 bin-main coverage。
- 2026-08-21：`bin-ha-rest-lifecycle` 与 `server_http_contract` 的顺序执行叠加出新的 lane 长尾；提高
  两个原子 shard 的 LPT 权重后将其稳定装入不同 lane，保持每个 coverage target 的完整归属。
- 2026-08-21：最新原生 lane 计时继续校准 affinity、reconciliation、LinuxDo、reporting 与 server HTTP
  contract 权重，并下调已拆分 MCP shard 的旧估计；LPT 输出保持十六个非空 lane 且估算上限低于 120 秒。
- 2026-08-21：HA lifecycle 原子 shard 在原生 runner 上仍形成单体长尾；按 HA lifecycle/state 语义再拆为
  两个互斥 shard，并提高 server HTTP 与 rollup integrity 权重，避免长 shard 组合在同一 lane。
- 2026-08-22：原生 lane 计时确认 reconciliation 的 upstream test process 在同一 shard 内排队；覆盖按
  maintenance、upstream-reconciliation、projection 语义拆开，并以带余量的实测权重重新装箱。prepare
  同时记录 compile、test-list discovery、bundle staging 三段耗时，作为诊断证据而非新的性能门禁。
- 2026-08-22：后续原生计时确认 admin resources 与 request rollup storage 仍是单 lane 长尾；分别按
  identity/observability/settings 和 storage/request-log-retention/scheduled-maintenance 语义细分。静态
  prefix-union 合同固定旧覆盖集合，artifact 上传降低压缩等级以缩短关键路径的上传 CPU 时间。
- 2026-08-22：admin dashboard SSE refresh 在与其它 identity filter 进程共用时触发既有事件等待超时；
  改为独立的单进程、单线程精确 shard，并由 identity 的 exact exclusion 保持唯一归属。Actions 计时还
  发现 operational maintenance 被低估；按实测提高其权重并仅为该 shard 允许四个过滤进程，使 LPT 重新
  保持在 120 秒预算内。
- 2026-08-22：Actions 步骤计时确认 prepare 的 165 秒几乎完全是完整测试可执行文件的冷编译，test-list
  discovery 和 bundle staging 合计不足一秒。Plan 因此缓存 profile-scoped `target/ci-test`；缓存不复用后端
  测试 bundle，也不把缓存容量或保留策略写进仓库。
- 2026-08-23：缓存命中后原生计时将 prepare 编译降至约 94 秒，但 MCP research 与 HA serving 仍同处最长
  lane。research 依据 batch 子前缀拆成 protocol/batch 两个互斥串行 shard；runner 仅在父/子 libtest
  substring 匹配集合经验证等于 manifest starts-with 归属时使用 `--skip`，否则保持 exact fallback，避免以
  更快的过滤命令换取漏跑或重叠。
- 2026-08-23：后续完整 CI 的 MCP 拆分已将原 lane 从 173 秒降到 93 秒，但 request-log retention 与
  rollup lifecycle 的顺序组合仍使 aggregate 比五分钟多一秒。以该轮所有原子 shard 的实测耗时加八秒余量
  重校 LPT 权重，并把 retention 的 GC maintenance 与 serial policy 前缀拆成独立 shard，消除这条组合尾部。
- 2026-08-23：重复计时中 reporting 的 request-stats background slice 在两线程 test process 内发生
  `PoolTimedOut`。该测试拆为单独的单进程、单线程 shard，主 reporting shard 精确排除它；不改变断言、
  等待预算或任何测试的归属集合。
- 2026-08-23：另一次重复计时显示 26-test upstream reconciliation filter 在同一进程内偶发延长至 147 秒。
  该 shard 改以 coverage verifier 所列出的精确测试名运行，每个进程只运行一个单线程测试；CI 调用方可并行
  至四个进程，低资源本地默认仍是一进程。测试集合、断言与时间语义保持不变。
- 2026-08-23：scheduled duplicate-claim 与 reporting admitted-flush 在各自两线程过滤进程中出现
  `PoolTimedOut`。二者各自成为单进程、单线程的精确 shard，父 prefix 只排除对应全名；不改变断言、等待预算、
  重试或覆盖归属。
- 2026-08-23：LinuxDo/forward lane 的 dashboard overview filter 单独运行 26 个测试时达到约 107 秒，和
  其它 forward checks 叠加后形成长尾。该 dashboard prefix 拆为独立的两线程语义 shard，并按实测提高剩余
  LinuxDo shard 权重。
- 2026-08-23：native Actions shard telemetry 显示旧权重高估大部分短 shard，却低估账户/用户 identity
  组合 shard；后者与 MCP protocol 串行后超过单 lane 预算。identity coverage 因此拆为四个互斥语义组，
  其中 business-call 的既有串行 prefix 独立保留，全部 LPT 权重按实测重校准。
- 2026-08-23：第二个 native cold sample 显示 request-rollup lifecycle、HA lifecycle 与 MCP batch
  各自仍在单 lane 预算内，却同时波动并被 LPT 串到同一 lane。权重改取重复样本的上界并将三者分散，
  不改变其 selector、进程上限或测试语义。
- 2026-08-23：后续 sample 的 `alerts_and_ha::ha_` 单一过滤进程执行 24 个测试达到 175 秒，超过任何
  lane 预算。该 prefix 按事件与恢复语义改为两个互斥 shard，复用既有独立测试进程模型；state base 保留
  其它 HA lifecycle selector，coverage verifier 继续阻止漏跑和重叠。
- 2026-08-23：HA event/recovery 拆分后，原 175 秒 prefix 收敛为两个短 shard。随后 telemetry 捕获到
  account/LinuxDo、business-call、MCP rebalance-control 与若干组合 lane 的真实峰值；LPT 权重按这些峰值
  重校准，使每个高波动 shard 不再与另一高波动 shard 串行。
- 2026-08-23：dashboard overview 与 admin observability 的同 lane 组合达到 124 秒，虽未超过 backend
  墙钟预算却超过单 lane 预算。两者按新峰值重新加权并分散到不同 lane；其它调整只收回已由重复样本
  证明的冗余余量。
- 2026-08-23：后续 cold sample 发现 admin identity 与 operational maintenance 的峰值也会使组合 lane
  超预算。两者改与低波动 shard 配对；state base、HA event/recovery、support 与单条 contract shard 的
  余量按重复观测收回，使固定十六 lane 装箱保持在 120 秒上限内。
- 2026-08-23：`tavily_http_search` 的 40 个测试在同一两线程过滤进程中出现 184 秒长尾。该 prefix
  改由 exact dispatcher 在 CI 的四进程上限内逐条运行；测试集合、断言和生产 SQLite 控制预算不变。
  同时提高该原子 shard 的 LPT 权重，使其不再与 forward-proxy 串行。
- 2026-08-23：缓存命中的完整原生计时显示 MCP research protocol 的旧 64 秒权重显著高于其 18.89 秒实际
  执行时间，反而把 operational/reporting/session 串在同一 lane。按带余量的实测值校准为 28 秒，使 LPT
  分散该组合；selector、进程上限、测试集合与断言保持不变。
- 2026-08-23：随后冷缓存完整运行中 MCP billing 波动至 74.36 秒，与 rollup integrity 串行后使单 lane
  达到 123 秒。其权重提高到 81 秒，使 LPT 改与两个短 shard 配对；selector、进程上限、测试集合与断言
  保持不变。
- 2026-08-23：另一次冷缓存运行显示 operational maintenance、MCP research batch 与 MCP affinity 被串为
  141 秒 lane。operational 的重复样本上界据此提升至 110 秒；已因独立并行处理而不再长尾的 dashboard
  overview 与 Tavily HTTP 分别回校为 51/68 秒，腾出 LPT 容量分散该组合。测试归属、selector、进程上限与
  断言保持不变。
- 2026-08-23：MCP billing 的 32 条测试在单一过滤进程中偶发耗时 152 秒。该既有语义 prefix 改由 exact
  dispatcher 在 CI 的四进程上限内逐条运行；低资源调用仍受一进程上限约束。测试集合、断言与生产时间语义
  保持不变。

## Key Reasons / Replacements

- 该主题新增的直接原因是 `CI Pipeline` 关键路径长期接近 1 小时，且结构性浪费主要来自单长 backend job、重复 frontend build 与不必要的 downstream `needs` 阻塞。
- 该 spec 不替代 release / docs-pages 相关 spec；它只约束 PR `CI Pipeline` 下的 backend split 与 safe parallelization。
- 早期实现阶段曾放弃“先把所有 `chunk_*.rs` 机械模块化再靠命名空间切 shard”的方向，因为当时 `src/tests/**` 与 `src/server/tests/**` 里存在真实跨文件 helper 依赖，贸然拆模块会破坏可见性并扩大改动面。
- 2026-06-18：测试组织进一步收口为真实语义模块 + 显式 `support` 层；`src/tests/mod.rs` 与 `src/server/tests.rs` 不再依赖 `include!(\"chunk_*.rs\")`，shard selector 也同步切到稳定的模块前缀，而不是继续绑定预算驱动的机械切片文件名。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`

## Legacy Identity

- Legacy compatibility identity: `#3grrf`.

## Stable minimal web fixture

- The CI fixture now uses a stable profile-local path and preserves mtimes for unchanged files. This keeps
  the build-script environment input stable across cache hits while retaining normal asset-change tracking.

## Exact prepared bundle cache

- Cargo rebuilds test executables after a fresh checkout even when a restored `target` directory is content
  compatible. The plan therefore caches the smaller, checksum-verified v2 backend bundle by every input that
  affects compilation or sharding. Exact hits bypass Cargo entirely; target caching remains only for the
  cold-build dependency fallback.
