# 多供应商搜索接入实现状态（SerpApi / Exa / Brave）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，
> 避免这些细节散落到 PR / Git 历史里。设计正文见 `../../multi-provider-search-proxy.md`。

## Current Status

- Implementation: 主体已落地（T1-T18、T20-T26 已提交在本地 main 栈，尚未推送）；
  T19（配额同步车道）代码完成但**未提交**，因 WSL 验证通道故障停在 G5/G6 门禁。
- Lifecycle: active
- Catalog note: 三类新供应商 REST façade + MCP 入口、provider-scoped 密钥池、共享钱包计价、
  健康矩阵、读面 provider 维度、控制台与技能包均已交付；`quota_sync/serpapi` 主动同步车道
  等待验证通道恢复后重跑门禁并提交。

## Coverage / rollout summary

- Provider 注册表：`src/providers/{id,spec}.rs`（ProviderId、ProviderSpec、计价函数、恢复策略）。
- 路由与认证：`src/server/provider_facade.rs`（7 个 REST 操作、3 个 MCP 入口、子路径拒绝、
  原生槽矩阵与剥离）；`src/server/serve.rs:566` merge。
- REST 执行层：`src/tavily_proxy/proxy_provider_facade.rs`（通用骨架）、
  `proxy_provider_facade_serpapi.rs`、`proxy_provider_facade_exa.rs`、`proxy_provider_facade_brave.rs`（含被动采集）。
- MCP：`proxy_provider_facade_serpapi_mcp.rs`（托管直通 + 会话翻译 + 工具计费）、
  `src/server/local_mcp_facade.rs`（本地框架）、`provider_mcp_brave.rs`、`provider_mcp_exa.rs`。
- 分类与隐私：`src/analysis/provider_failure_classification.rs`、per-provider 头白名单与脱敏、
  X-Project-ID provider 门控。
- 池与恢复：provider-scoped acquire / 亲和 / backoff / quarantine；`reset_monthly` 按 RecoveryPolicy。
- 数据：主账本 v45（provider 列）与 sidecar `request_logs.provider` ALTER 路径。
- 读面：summary / logs facets+filter / token 日志 provider 维度（`src/store/key_store_request_logs_provider_read.rs`）。
- 控制台：admin keys/logs/dashboard、用户指南、demo mode、i18n en+zh（T21-T25）。
- Skills：7 个新技能包 + best-practices 对比章节（T26 已落地）。
- T19（未提交）：`src/tavily_proxy/proxy_quota_sync_provider_adapters.rs`、
  `src/server/schedulers_provider_quota_sync.rs`、`reconciliation_quota_sync_api.rs` 包装器、
  待同步选择 provider 过滤、对账/research 守卫、mock_serpapi 计数旋钮、测试文件。

## Remaining Gaps

- T19：G5（verify/run-shard 在测试搬移后未重跑）与 G6（真实 QA）未完成；未提交。
- T19 附带记录：`key_store_jobs.rs` 未编辑（新 job type 优先级落到默认桶 6；
  全局 stale_control_recovery 覆盖崩溃恢复），已在证据中记录。
- T18 记录的 D15-1（user/token primary affinity 缺 provider 维度）保持开放，见 HISTORY FU-3。
- 公共 docs-site（T28）与最终验证波（T29-T32、F1-F4）未完成。

## Related Changes

- Rust（新增）：
  - `src/providers/`
  - `src/server/provider_facade.rs`、`provider_mcp_{serpapi,brave,exa}.rs`、`local_mcp_facade.rs`
  - `src/tavily_proxy/proxy_provider_facade*.rs`
  - `src/analysis/provider_failure_classification.rs`
  - `src/store/key_store_request_logs_provider_read.rs`
- Rust（修改，T19 未提交）：
  - `src/tavily_proxy/reconciliation_quota_sync_api.rs`、`proxy_quota_sync_and_jobs.rs`
  - `src/server/schedulers.rs`、`schedulers_remote_attempt.rs`
  - `src/store/key_store_dashboard_and_job_read_models.rs`、`key_store_upstream_reconciliation*.rs`
- 测试：
  - `tests/provider_facades_contract.rs`、`tests/provider_mcp_e2e.rs`
  - `src/tests/provider_*`（分类 / 池 / 隐私 / 读面 / 健康矩阵 / 配额适配器）
  - `src/server/tests/provider_*`（路由 / admin / 读面 / 配额调度）
- Web：`web/src/api/providers.ts`、admin keys/logs/dashboard、user console guides、demo mode、i18n。
- Docs/Skills：`docs/adr/0005-*`、`docs/multi-provider-search-proxy.md`、
  `docs/specs/multi-provider-search-integration/*`、`docs/specs/rebalance-mcp-gateway/SPEC.md`（Amendment）、
  `skills/{serpapi,exa,brave}-hikari-*`。

## Validation

- `cargo test --locked --test provider_facades_contract`
- `cargo test --locked --test provider_mcp_e2e`
- `cargo test --locked --lib provider_health_matrix`
- `cargo test --locked --test server_http_contract`（Tavily 不变性）
- `cargo test --locked --test mcp_billing_regression`（Tavily 不变性）
- `cargo clippy --locked -j 2 -- -D warnings`
- `bunx --bun dprint check 'docs/**'`
- T19（未提交部分）：`cargo test --locked --lib provider_quota_sync_adapters`、
  `cargo test --locked --bin tavily-hikari provider_quota_sync_dispatch`
  （停止前均为 green；搬移后的注册测试待重跑，见 G5）。
- 证据：`.omo/evidence/task-{1..26}-serpapi-exa-brave-providers.*`。

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../multi-provider-search-proxy.md`
- `../../adr/0005-multi-provider-upstream-abstraction.md`
