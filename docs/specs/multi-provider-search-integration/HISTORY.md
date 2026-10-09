# 多供应商搜索接入演进历史（SerpApi / Exa / Brave）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，
> 规范正文仍以 `./SPEC.md` 为准。上游合同与决策依据见 `.omo/drafts/serpapi-exa-brave-providers.md`
> 与 `docs/adr/0005-multi-provider-upstream-abstraction.md`。

## Decision Trace

- 2026-09-24: 计划与验收合同冻结（`.omo/plans/serpapi-exa-brave-providers.md`，32 todos + F1-F4，
  Metis 修订 M1-M24）。方案落定：纯增量接入；SerpApi MCP 托管直通；Exa/Brave 本地 facade；
  共享 credit 钱包 + 按操作计价；SerpApi `/account` 主动同步、Brave 响应头被动采集、Exa 本地计数；
  分段对账保持 Tavily-only。
- 2026-09-25 起: T1-T18 依次落地（ADR 0005、ProviderSpec 注册表、分类矩阵、头白名单与脱敏、
  主账本 v45 与 sidecar provider 列、provider-scoped 池、admin API、路由认证、三条 REST façade、
  SerpApi MCP 直通、Exa/Brave 本地 facade、健康矩阵与告警）。
- 2026-09-29: T20 读面 provider 维度落地（summary / logs facets+filter / token 日志；
  rollup 保持 provider-agnostic）。
- 2026-09-30 / 10-01: T21-T26 控制台、demo、技能包落地。T19（SerpApi `/account` 车道 +
  provider 守卫）代码完成、G1-G4 green，因 WSL 验证通道（hikari-f）主机故障
  （REGDB_E_CLASSNOTREG，需管理员修复）停在 G5/G6，按“never commit red/unverified”契约**未提交**。
- 2026-10-09: T27 交付设计文档 `docs/multi-provider-search-proxy.md` 与本 spec 主题；
  `rebalance-mcp-gateway/SPEC.md` 追加 additive amendment，把已提交代码里的 provider-scoped
  `upstream_operation` 词表纳入既有 closed enum 契约。

## Key Reasons / Replacements

- 供应商差异集中在少数真实接缝（URL、密钥注入、耗尽信号、工具命名、同步来源），
  因此用 ProviderSpec 注册表 + 分类表吸收，而不是在请求路径散落 if/elif。
- Tavily 计价/对账机制经多轮加固，必须字节稳定：所有新面走旁路（新模块、新路由、新列）。
- Exa 托管 MCP 有 Cloudflare 阻断观测 + 第三方分析上报，与高匿姿态冲突，故仅本地 facade（ADR 0005）。
- Brave 无托管 MCP，且 ToS 禁止长期保留搜索结果，保留策略交由操作者用现有 body-retention 杠杆控制。
- SerpApi 429 双重语义（月度耗尽 vs 小时限速）必须按报文分流，否则会把限速误判为耗尽。
- Brave 无效密钥是 422（非 401），分类必须按 `error.code` + `meta.errors[].loc` 分流，
  否则会把参数校验错误误罚密钥。
- `X-Project-ID`、分段对账与 432 低余额降级是 Tavily 专属身份/计费机制，绝不能外溢到第三方上游。

## Follow-ups（明确记录，v1 之外）

1. FU-1（M8）: 新供应商低余额降级对齐（SerpApi `plan_searches_left` 阈值 + 续费日月窗口）
   不在 v1；`api_key_low_quota_depletions` 保持 Tavily-only（负向断言已固定）。
2. FU-2（M12）: Exa 团队级 QPS 天花板（provider 级冷却章，复用既有 backoff scope）
   不在 v1；当前仅按 key 遵守 `Retry-After`，天花板在文档/FAQ 说明。
3. FU-3（T18 D15-1）: user/token primary affinity 行缺 provider 维度；
   需 v46+ migration（或按商亲和表）后由 `acquire_key_for_provider` 读写 provider-scoped 行；
   Tavily 以默认 `tavily` 保持字节稳定。过渡期 SerpApi MCP initialize 使用 provider-scoped LRU
   车道，不写 token-primary 亲和。
4. FU-4: HA peer 同步的 session DTO 未带 provider tag；SerpApi follow-up 目前经本地 store
   解析 provider（跨节点场景待 DTO 扩展后收敛）。
5. FU-5（T22）: `/api/keys/batch` 无 provider 字段；非 Tavily 批量新增等价于顺序单 key 调用
   （管理端批量输入一次只对一个 provider 生效）。
6. FU-6（M18）: DashboardOverview 的 credit 对比图按 provider 分段属于 rollup 后续项；
   当前严格遵守“不同单位不混入同一序列”的禁令（本地估算与上游采样不跨商相加）。
7. FU-7（T14/T19 FU-B）: Brave 月窗口 `Limit=0`（无限额）在 `record_quota_sync_sample` 的
   `i64` 非空接缝上无法表达 NULL；已记录决策为“跳过写入”，绝不持久化 `0`，
   新建 key 的 NULL `quota_limit` 保持不变。若要消除该限制需先改接缝/列约束。
8. FU-8: 本地 facade（Exa/Brave）尚无独立 `gateway_mode` 词表（当前 provider 行的
   `gateway_mode` 为 NULL，hosted passthrough 记录 `upstream_mcp`）；是否引入
   `local_facade` 等词表待本 SPEC amendment 的 ratification 决定。
9. FU-9（FU-E）: 测试环境必须 unset `HTTP(S)_PROXY`；否则本机代理会把 127.0.0.1 探针的
   transport 分类翻转为 semantic（基线证明在 HEAD 同样失败）。CI 加固任务（T29-T32）需固化该前置。

## Status of pending lanes

- `quota_sync/serpapi`（T19）: 设计冻结、代码完成；G1 RED→GREEN、G2 goldens、G3 clippy、
  G4 budgets green；G5 verify/shards 因测试搬移未重跑、G6 QA 未开始；未提交。
  证据：`.omo/evidence/task-19-serpapi-exa-brave-providers.txt`。
- 公共 docs-site（T28）与最终验证波（T29-T32、F1-F4）: 未完成，属本主题后续。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../../multi-provider-search-proxy.md`
- `.omo/drafts/serpapi-exa-brave-providers.md`
- `.omo/evidence/task-19-serpapi-exa-brave-providers.txt`

## Legacy Identity

- None（新主题，无历史 Legacy ID）。
