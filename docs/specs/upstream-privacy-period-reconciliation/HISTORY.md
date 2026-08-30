# 上游身份隐私与分段积分对账演进历史（#3s7ku）

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-08-02: 候选选择改为有界索引分页；管理员状态采用 observation unknown/null 合同；全局本地
  429 压力按 `2/5/10/30` 分钟持久化退避，并与 delayed representative job 复用。

- 2026-07-31: reconciliation 纳入 remote-I/O slot 与 20 秒总预算；全局 429 退避和 delayed
  representative job 作为同一持久化生命周期恢复。

- 2026-07-14: 默认模式锁定为 `accessToken`，不支持原始项目派生或按项目查询用量。
- 2026-07-14: 业务日采用服务器现有业务时区，并固定三个窗口 `00-11 / 11-22 / 22-24`。
- 2026-07-14: 结算只执行一次；Research 最长等待 24 小时，超时后 degraded 结算且不自动复核。
- 2026-07-15: 去掉 API/MCP rebalance 百分比放量控件；新流量是否全量走 rebalance 只由两个开关决定，兼容百分比字段统一归一化为 `0|100`。
- 2026-07-16: 拆开 shadow compare 与 precise cutover 门禁；compare-only 不再等待遗留 `upstream_mcp` session 排空，系统状态主相位改为“仅对比”。
- 2026-07-16: 新增隐藏路由 `/admin/system-settings/mcp-session-bindings` 及其 warning/stat 卡入口，用于查询与释放异常 `upstream_mcp` session 绑定记录。
- 2026-07-20: compare-only 的 `新方案 24h` 改成 confirmed absolute value / unavailable 双态合同；equal-delta 不再折叠为空。
- 2026-07-23: compare-only 的 `新方案 24h` 再调整为 hybrid 合同：已对账部分使用 shadow、未对账部分保留本地估算；`shadowDailyAvailability` 改为 `confirmed | projected | null`，compare-only 不再回空值。
- 2026-07-23: reconciliation 候选调度新增 recent/backlog 双车道与 `12/8` 默认预算，优先收敛今日+昨日窗口，再回填旧 backlog。
- 2026-07-20: reconciliation 补充 enqueue reuse / exhaustion 与 run started / completed 诊断信号，系统状态页新增最近运行、最近 shadow 调整、最近入队失败时间戳。
- 2026-07-22: backlog 排障保持严格 degraded 语义，不把缺值伪装成旧值；`rate_limited` 拆分为上游 429、本地 usage 限流与其他重试，并对 hot upstream Key 应用 key-scoped backoff，系统状态页展示当前时段 per-key 活动图。
- 2026-07-25: Research 不再依赖客户端结果 GET 才变为 terminal；现有 reconciliation worker 在结算前有界 sweep 已关闭窗口，并以独立 `period_reconciliation` Key cooldown 阻止 429 扇出。管理员可查看今日账号/账期/Research 覆盖和每 Key 阻塞状态。
- 主结算与 Research 的预算边界已明确分离：先完成最多 2 秒的候选 hydrate 并启动主结算，Research 只能使用同一轮剩余预算；本地预算压力与真实 upstream 429 退避也已分开持久化。
- 最终审查进一步固定远端请求启动、观察、结算与收尾的嵌套截止时间；Research 的状态写入也受同一截止时间约束。local pressure 元数据现在随 HA meta 接管恢复。
- 2026-08-30: terminal Research 从主 reconciliation 尾部预算迁到独立 durable drain。v21
  cursor 不变；每次 poll 的 outcome、逐 Key cooldown 和精确 cursor 在 drain claim 下原子接受。
- 历史 usage 投影改为版本化 lifecycle：旧库只在没有 durable candidate 时执行有界单页，空新库直接完成；
  新写入由触发器维护，不再让已完成的 no-adjustment 因 source cursor 反复入队。

## Key Reasons / Replacements

- 本 spec 替代 `34pgu` 的固定项目 UA 条款，UA 改为管理员配置且空值省略。
- 本 spec 替代 `m30lm` 与 `cp8s9` 的 `X-Project-ID` 原样上送条款，但保留本地 routing subject/亲和语义。
- 本 spec 替代 `xm3dh` 中 Rebalance MCP HTTP 固定 UA 的条款；该路径与 REST API 一样不发送 UA。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`

## Legacy Identity

- Legacy compatibility identity: `#3s7ku`.
