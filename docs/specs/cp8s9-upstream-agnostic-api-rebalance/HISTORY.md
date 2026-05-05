# 上游不可知 API 负载均衡演进历史（#cp8s9）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-05-04: 新增本 spec，将普通 API 选路从 Tavily `X-Project-ID` 专属亲和推广为 Hikari 通用 API rebalance 能力。
- 2026-05-05: 实现落地为 `api_rebalance_http` full-pool selector；`X-Hikari-Routing-Key` 成为 Hikari 自有 routing subject，`X-Project-ID` 降级为 Tavily adapter 兼容 fallback。

## Key Reasons / Replacements

- MCP Rebalance 已证明 full-pool + cooldown/pressure 避让能缓解热点 key。
- `m30lm` 的项目亲和仍保留为 Tavily adapter 兼容输入，但通用 selector 不再以 Tavily header 作为核心语义。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
