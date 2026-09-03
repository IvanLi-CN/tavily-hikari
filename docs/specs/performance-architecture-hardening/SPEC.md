# Tavily Hikari 性能架构渐进加固

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见
> `./HISTORY.md`。

## 背景 / 问题陈述

当前单体服务中的后台生命周期、SQLite 连接所有权、维护任务、HA 排债、对账和管理员读取路径通过
`KeyStore`、`TavilyProxy` 及若干全局 gate 交叉耦合。局部性能修复能够缓解症状，却无法稳定约束
角色切换、任务 claim、写入预算和读模型成本。本规范定义渐进式内部边界，保留单体与 SQLite，避免
以重平台迁移替代架构治理。

## 目标 / 非目标

### Goals

- 建立 revisioned writable authority 和唯一后台生命周期所有者。
- 让 SQLite pools、admission、事务与预算通过 `SqliteRuntime` 统一治理。
- 将 HA GC、reconciliation、alerts、dashboard、peer observation 与 request stats 收敛为有界接口。
- 通过 expand-contract 保持滚动升级期间的存储与 wire 兼容。
- 以依赖门禁防止生产热路径重新访问 raw pool、coalescer 或旧全局 gate。

### Non-goals

- 不拆分微服务或迁移到 PostgreSQL。
- 不修改 SQLite pool、WAL、cache 或 mmap PRAGMA。
- 不改变 HA retention、ACK、账务真值或 `410 -> baseline` 语义。
- 不全量拆解 `KeyStore` / `TavilyProxy`，只迁移确认的生产热路径。
- 不改变公开或管理员响应 shape，也不重设计前端视觉结构。

## 范围（Scope）

### In scope

- `HaRuntime`、`MaintenanceRuntime`、`SqliteRuntime` 的所有权边界。
- per-channel HA GC work、reconciliation work projection 和 typed outcomes。
- `AlertProjection`、`DashboardReadModel`、`HaPeerObservationStore`、`RequestStatsPipeline`。
- maintenance、HA、reconciliation、dashboard、alerts、peer 和 request-stats 热路径迁移。

### Out of scope

- 101 部署、生产数据清理、VACUUM、baseline、TLS 或 HA 配置操作。
- keyed journal、ACK 内 compaction 或 HA v2 wire 协议。
- 与上述热路径无关的业务 facade 重构。

## 需求（Requirements）

### MUST

- `HaRuntime` 发布单调递增 revision 的 writable authority；authority epoch 持久化在 SQLite，并与业务
  写入的最终 fence/commit 通过同一 SQLite writer serialization point 排序。旧 revision 不得继续
  claim、远端调用或写入。
- `MaintenanceRuntime` 独占 worker、reaper、`JoinSet`、调度 lease 与 instance-owned
  actual-request remote-attempt admission 的生命周期。
- `SqliteRuntime` 唯一持有生产 pools、事务 guard、admission 和操作预算。迁移后的 HA read
  session、通用 audit snapshot 与 Dashboard integrity write 不得取得裸 pooled connection；取消或
  未完成 guard 必须丢弃物理连接。
- `SqliteRuntime` 的 admission 属于单个 `KeyStore` 实例。`maintenance_control` 只可进行 claim、
  finish、continuation 与 stale recovery 等短事务，连接和 SQLite writer 预算均不超过 `100ms`；
  `maintenance_bulk` 必须在获取连接前检查至少两个实际 idle 或可立即分配的前台 pool slot、前台到达率不高于 `5 rps`、最近
  五秒无 SQLite busy/pool timeout，并持有唯一 bulk permit。拒绝必须返回 typed deferred，不得先取得
  pooled connection 或启动后台无限重试。
- HTTP 发起的 manual scheduled-job enqueue 属于 `foreground_work`，其 pool acquisition 预算不超过
  `250ms`，但不占用 bulk permit。若 `ha_outbox_gc` 在此预算内仍不能取得连接，端点返回 `503`，不伪造
  `202`、sentinel job id 或 durable row；HA 的既有 watchdog 和 worker wake 继续恢复自动 recovery debt。
  其他人工触发仍将实际 persistence failure 返回给调用方。
- HA GC、request-log GC、request-stats flush、pressure rebuild、reconciliation projection 与 Dashboard
  integrity 是 `maintenance_bulk`；GC 在每条 SQL 后重新检查 admission，压力只推迟当前 channel，不得
  冻结其余 eligible channel。request-log GC 遇到未封存的本地日时只完成一次安全检查并以既有
  five-minute continuation 让步，不得在同一 slice 重复扫描或删除派生 rollup。Dashboard 仅在有
  last-good 时因 admission、busy 或 refresh 超时直接返回该快照；
  request-stats background wake 每秒最多提交四个自适应 `25..250` logical-key transaction，并受同一
  `50ms` 启动/下一 chunk 预算约束后原子回灌未开始尾部；一旦事务已开始，runtime 必须先完成 commit
  或 rollback，不能因预算到期将该批回灌。只有 shutdown drain 可以连续处理。冷启动只允许
  一个 shared singleflight loader，server 在开始监听前可给同一 loader 一次一秒 head start；每个读取
  请求最多等待一秒。超时只结束该读取请求，不得取消仍在运行的 loader，后续读取在其完成后复用同一快照。
- 普通 Dashboard、summary、hourly window 与 rankings 读取只消费 durable request stats，不得触发
  flush 或获取写连接。pending/flushing 仅参与内部 freshness，不改变 HTTP shape。
- `ha_outbox_gc_work` 按 control、billing、runtime 独立持久化 eligibility、claim 与 continuation。
- `pending_channel_mask=0` 仅表示最近一轮 controller observation 没有剩余工作，不是永久库存断言。
  scheduler 每五分钟以 `maintenance_control` 预算只读检查 channel state 的 observation age；到期时将已过期
  observation 的 channel 加回 pending mask，再唤醒一个仍受 bulk admission 保护的 indexed channel slice。
  watchdog 不得扫描 outbox 或计算精确库存。
- HA GC 与 scheduled work 使用 typed outcome 在同一原子边界完成 claim 和 continuation。
- 相同 wire payload 的 UPDATE 不产生 HA outbox 事件；有效变化恰好产生一条兼容事件。
- reconciliation 使用持久 work projection、公平 cursor 和原子 runtime state，并区分本地压力、429、
  transport、semantic failure 与 budget exhaustion。
- Reconciliation preparation keeps control metadata in `maintenance_control`; every bulk source
  `SELECT` runs in its own 250ms native SQLite read session. A source-read deadline produces a
  claim-fenced `projection_read_budget` defer and one 30-second continuation without beginning a
  merge, completing work, or starting remote I/O.
- Main reconciliation and Research use separate durable representatives. The Research drain owns
  the indexed keyset page and exact cursor, performs at most one poll every five seconds, and keeps
  read/lease defers independent of main transport, terminal, and billing state. Its queue-time
  fairness anchor survives foreground, lease, read-budget, and control defers. After 120 eligible
  seconds it may take the next non-manual request turn: main reconciliation is ordered by
  `available_at`, while Research is ordered by that durable `queued_at` anchor; main wins an exact
  tie. An accepted Research lease-contention continuation retains its aged turn until its next
  actual HTTP start, so ordinary automatic work cannot reclaim that request opportunity despite
  continuing its local preparation. Manual priority and one actual HTTP request at a time remain
  unchanged.
- Multi-key main candidates persist successful per-key observations by current work generation.
  A run fills no more than two missing keys and uses a claim-fenced `remote_attempt_budget`
  continuation when the candidate is still incomplete; cross-key summation and terminalization wait
  until every current-generation key is present.
- Historical usage projection has a versioned lifecycle independent of candidate selection. A
  pending upgrade projects one bounded page only after durable candidates drain, while new usage
  is maintained by triggers. A completed lifecycle must not requeue a completed no-adjustment
  work generation; pre-trigger usage updates still reopen their generation, and equal
  usage/settlement timestamps remain conservatively reopenable because they have second precision.
- 告警读取全部来自可重建 `AlertProjection`。recent tail 与管理员历史使用独立 cursor/fence：Dashboard
  HTTP/SSE 只消费 `recent_coverage=ok` 的共享 read model，否则保留 last-good；管理员 Events/Groups
  仅在 complete history coverage 为 `ok` 时读取 observability sidecar。任一 cursor 追赶、stale 或
  rolling upgrade 期间保守回退既有原始查询，不能把未投影历史表示为空。
- Dashboard 的 recent alert summary 只由 projection worker 在独立的 60 秒窗口内物化。若 source
  generation 在窗口内前进，已有 summary 必须标记 stale，Dashboard HTTP/SSE 继续服务 last-good，而非
  在读取路径执行 sidecar 聚合。
- 历史 lane 的 fence 必须与 recent tail 的起点同秒衔接：tail 拥有起点秒内的记录，history 包含严格更早
  的记录，运行时始终使用复合 cursor；仅 v17 对旧的“同秒 + 空 id”历史 fence 做一次性上一秒迁移，
  以恢复该低 sentinel 的边界所有权。空闲 source probe 不得写 cursor/generation；覆盖观察只能由
  独立低频 heartbeat 更新。Dashboard summary 只允许从 sidecar 执行时间窗固定、结果有界的 SQL 聚合，禁止
  把整窗 `payload_json` 拉回进程后再分组。
- 已应用的 projection migration 不得原地修改 checksum。若发现历史 cursor/fence 边界缺口，后续加法
  migration 只能重置可重建的 history lane，由后台小片幂等重放；recent tail、原始事件与账务真相保持不变。
- 普通管理员 HA GET 只读取 peer observation cache；危险 HA 操作继续 live probe。
- 每个节点通过内部 HA probe 报告 `writable_tenure_v1` capability。planned cutover、finalize 和普通
  promote 必须实时探测全部已配置节点；任一节点 capability 缺失、unknown 或不可达时 fail closed。
  当前 active 不可达时，只有显式 `force=true` 的 emergency takeover 可绕过该 active 的远端
  capability，且必须保留既有防脑裂校验、确认和审计；其他 peer 仍按既有 emergency takeover 安全规则
  参与拒绝判断。公开 HA 状态响应保持不变。

### SHOULD

- 每个新边界提供窄接口和稳定错误分类，调用方不依赖 SQLite 实现细节。
- read model 在刷新失败时返回 last-good 与显式 stale coverage；冷启动失败返回 degraded/503。
- 迁移使用 shadow comparison、指标和状态跃迁日志证明等价后再删除旧路径。

### COULD

- 内部 observation 可增加估算字段，但必须明确 coverage 与 observed-at，不能以零伪装 unknown。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

1. Promotion 创建新 writable revision，并恰好启动一套 `MaintenanceRuntime`。
2. Demotion 先触发旧 revision 的 cancellation token，再以 SQLite 写事务递增持久 authority epoch；
   demotion 只有在该事务提交后才对外完成。业务写事务必须在同一事务内校验预期 epoch，并由 SQLite
   writer serialization 保证“旧业务 commit”与“demotion epoch commit”存在全序：旧写入要么先完成，
   要么在 epoch 变化后失败，不存在检查与 commit 间的 TOCTOU。
3. 每个 HA channel 独立 claim GC work，完成后原子记录 typed outcome 和 continuation。
4. reconciliation projection 选择有界 work page，engine 在 2 秒内开始首次远端尝试并在 20 秒内结束。
5. observability sidecar 持久化 alert projection；Dashboard builder 生成共享 `Arc` snapshot，HTTP/SSE
   仅 snapshot/subscribe。

### Edge cases / errors

- Stale generation 的完成、失败或 continuation 均被拒绝，不能覆盖新 claim。
- SQLite busy 在 250ms 内返回 typed deferred。HA continuation handoff may use only a fixed,
  bounded same-generation retry schedule before stale recovery; it must not form a background
  infinite retry loop.
- 单一 HA channel 的 eligibility 延迟不得阻塞其他 channel。
- read model cold start 无可用 last-good 时显式 degraded，不在请求线程执行重聚合。
- 滚动升级中的旧节点继续消费现有 wire payload；新字段或表仅以向后兼容方式扩展。
- 旧节点不报告 `writable_tenure_v1`，因此混跑期 planned cutover、finalize 和普通 promote 按
  fail-closed 规则拒绝；普通同步和读取不受 capability gate 影响。仅在 active 不可达时保留显式
  emergency takeover，不把 capability 缺失本身当作 peer 已失效的证据。
- authority epoch commit 在 250ms 写预算内遇到 busy 时，节点保持 cancellation 生效并进入持久
  `demoting` 状态；禁止恢复旧 runtime、promotion 或新 claim。唯一 tenure supervisor 以有界退避重试，
  直到 epoch commit 成功后完成 demotion；重启必须恢复该状态而不是重新获得 writable authority。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）             | 类型（Kind）    | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）            |
| ------------------------ | --------------- | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ------------------------ |
| `HaRuntime`              | Rust API        | internal      | New            | 本文                     | HA runtime      | server lifecycle    | revisioned authority     |
| `MaintenanceRuntime`     | Rust API        | internal      | New            | 本文                     | maintenance     | workers/reaper      | sole lifecycle owner     |
| `SqliteRuntime`          | Rust API        | internal      | New            | 本文                     | storage         | hot paths           | pools/admission/budgets  |
| `RequestStatsPipeline`   | Rust API        | internal      | New            | 本文                     | observability   | dashboard           | bounded ingestion/rollup |
| `HaPeerObservationStore` | Rust API        | internal      | New            | 本文                     | HA              | admin reads         | cached observation only  |
| `ReconciliationEngine`   | Rust API        | internal      | New            | 本文                     | billing         | maintenance         | typed outcomes           |
| `AlertProjection`        | Rust API/schema | internal      | New            | 本文                     | observability   | alerts/dashboard    | rebuildable projection   |
| `DashboardReadModel`     | Rust API        | internal      | New            | 本文                     | admin reads     | HTTP/SSE            | snapshot/subscribe       |

## 验收标准（Acceptance Criteria）

- Demotion 请求发出后 250ms 内停止 claim 并取消在途远端请求；authority epoch commit 成功后旧
  revision 不得提交新写入。epoch 写锁超时时节点保持 `demoting` fail-closed，锁释放后只完成一次 epoch
  commit，promotion 仍只启动一套 runtime。
- 两节点混跑测试中，任一已配置节点不报告 `writable_tenure_v1`、capability unknown 或不可达时，普通
  promote、finalize 与 planned cutover 均 fail closed；全部节点报告 capability 后才允许进入既有切换
  校验。
- active probe 不可达时，普通 promote 仍拒绝；显式 `force=true` emergency takeover 只有在既有防脑裂
  条件通过后才允许，并产生包含 capability bypass 原因的控制面审计事件。
- control 延迟 300 秒时 billing/runtime 仍持续推进；stale generation 无法完成新 claim。
- 相同 wire payload UPDATE 不产生事件，有效变化恰好一条，旧版本仍可消费。
- reconciliation 首次远端尝试小于 2 秒、单轮不超过 20 秒，查询成本受 page limit 约束。
- 20 个 SSE 加并发 HTTP 下每 10 秒最多一次 Dashboard build；warm Dashboard 和缓存 HA GET
  p95 小于 100ms，读路径不执行写 SQL。
- Administrator Alerts use an exact normalized query key for a five-minute last-good entry. Under
  SQLite pressure the handler returns only that key's stale result with coverage metadata; without
  a matching entry it returns `503 Retry-After: 1` instead of beginning a raw alert CTE.
- Admin Alerts acquires its bounded read session directly from `SqliteRuntime`: acquire is capped
  at `100ms`, every SQLite statement has a native `250ms` deadline, and coverage plus projected
  catalog/events/groups reads never borrow a bulk permit or raw pool connection. Canonical
  catalog, events page `1/20`, and groups page `1/20` are prewarmed by one AppState-owned
  low-priority controller. It requires one available bounded read slot, foreground activity at most
  `5 rps`, and no recent contention; it does not reserve or grow extra pool capacity, stages the
  three reads, and publishes them only when the projection generation is unchanged. HTTP never
  starts warm work: canonical misses and expired entries return `503 Retry-After: 1`, while a prior
  generation remains available as stale until the next complete publish. Warm defers use
  `5s/5s/30s` backoff and never acquire the bulk permit.
- AlertProjection 与旧结果在时间窗、过滤、分页、分组和状态跃迁上等价。
- 30 分钟生产形状基准中进程组 RSS P95 不超过 256MiB。
- architecture checker 证明目标热路径不存在 raw pool、coalescer、全局 pointer-map gate 或旧 cache。
- 单连接池取消 read snapshot、HA export 或 immediate write 后，下一次 immediate transaction 可立即
  开始；writer lock 下管理员读取在 250ms 内返回 durable 数据且不产生读取触发的 busy。

### Reproducible performance workload

- Dashboard benchmark 使用 release build、预热后的 read model、20 个持续 SSE client 和 20 个 HTTP
  client；每个 HTTP client 每秒请求一次，按 3:1 轮询 Dashboard GET 与缓存 HA GET，持续 10 分钟。
  从服务端 phase timing 计算 p95，并按 build generation 验证任意 10 秒窗口最多一次 build。
- RSS benchmark 在 Linux x86_64 隔离环境运行 release build 30 分钟，前 5 分钟预热不计入结果；其后每
  5 秒采样一次主进程及其子进程 RSS 并计算 P95。SQLite fixture 包含 100 万 request log、每个 HA
  channel 10 万 outbox row、10 万 alert event 和 100 个 reconciliation work item；stub upstream 下
  维持 5 rps（60% HTTP API、20% MCP、20% 管理员读取）及 20 个 SSE client。
- 基准报告必须记录 commit SHA、构建 profile、CPU/内存环境、fixture seed、实际请求率、采样序列和
  p50/p95/max；不得把 cgroup file cache 计入进程组 RSS。
- 10 分钟双库 snapshot 对比保留原始 Dashboard p95 和 RSS P95。Dashboard 比较使用 10ms 的绝对测量
  下限，RSS 比较在相对 10% 外允许 40MiB 的 allocator/restart 噪声带；超过该带宽才判为候选回归。
  这项短时 gate 不替代前述 30 分钟 release RSS P95 `<=256MiB` 基准。

## 验收清单（Acceptance checklist）

- [ ] 运行时与 SQLite 所有权边界已落地。
- [ ] HA GC 与 reconciliation work 已独立持久化并通过并发回归。
- [x] AlertProjection 与 DashboardReadModel 已完成 shadow 和 cutover：sidecar cursor/fence 负责
      alert coverage，AppState 共享 immutable last-good snapshot，HTTP/SSE 不重算 raw alerts。
- [ ] 所有目标热路径已通过依赖门禁。
- [ ] 全量质量门禁与生产形状基准已通过。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust: `cargo fmt --all -- --check`、`cargo test`、
  `cargo clippy --all-targets --all-features -- -D warnings`。
- Web: `bun --cwd web test`、`bun --cwd web run test:source-budgets`、
  `bun --cwd web run build`。
- 并发与 SQLite 场景使用隔离测试环境和 stub/sandbox upstream。

### UI / Storybook

- 本 initiative 不改变视觉结构；若管理员状态实现改变可见状态，更新既有 stories 与交互覆盖。
- 视觉证据仅来自 aggregate 最终 SHA，且遵守 owner approval gate。

### Quality checks

- 每个 child PR 的 checks、review 与 integration CI 必须绑定同一 head SHA。
- aggregate 不得存在未解决 P0/P1/P2 finding。

## Visual Evidence

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：expand-contract 混跑阶段若发生角色切换，旧 runtime 可能违反新 authority 合同，因此明确禁止。
- 风险：持久 projection 的 shadow 等价性不足会误导管理员读取，cutover 前必须证明覆盖一致。
- 假设：单体 + SQLite 能在有界 admission 和 projection 架构下满足既定 SLO。

## Related ADRs

- [ADR 0001: HA Planned Cutover Control Plane](../../adr/0001-ha-planned-cutover-control-plane.md)
- [ADR 0002: Scoped SQLite and Remote Admission](../../adr/0002-scoped-sqlite-and-remote-admission.md)
- [ADR 0004: Research Uses an Independent Durable Drain](../../adr/0004-reconciliation-research-drain.md)

## 参考（References）

- `../../../CONTEXT.md`
