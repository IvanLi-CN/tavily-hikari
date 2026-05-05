# 4ua2f · Forward Proxy 共享 Xray 单实例热更新

## Summary

- 将 forward proxy 的 Xray runtime 从“每个 share-link 节点一个常驻 xray 子进程”改为“全局单个 shared xray PID + 每节点 relay handle”。
- 非 Direct 节点继续按节点维度选路，但节点的 `endpointUrl` 改为指向 shared xray 内部某个独立 relay handle 的 loopback socks 入口。
- settings save、subscription refresh、revalidate、validate、probe、trace 与真实 Tavily 请求统一复用 shared xray；配置变化通过同 PID 内增量 apply 生效。
- 被删除或变更的 relay handle 必须立刻停止新分配，但已有请求继续排空；只有 lease 归零后才允许删除 handle、端口和 runtime 文件。
- 进程启动时如果订阅刷新全失败，必须从本地 `forward_proxy_runtime` 恢复上一轮可解析的 subscription 节点，并立即同步 shared xray relay，避免重启后退化为只剩 direct/manual。
- `/health` 在启动宽限期内保持 `200 ok`；宽限期后，如果当前 forward proxy 节点需要 local relay 但 shared xray 未就绪，返回 `503 xray not ready`，响应不得包含订阅 URL、节点 URL 或 key。
- `/api/settings`、`/api/settings/forward-proxy`、`/api/stats/forward-proxy`、`/api/stats/forward-proxy/summary` 的 JSON contract 与现有 admin UI 字段保持不变。

## Functional/Behavior Spec

### Shared Xray runtime

- `XraySupervisor` 只允许维护 **1 个长期存活的 xray `run` 子进程**。
- shared xray 通过本地 API 控制面动态增删 relay handle；不得再按 endpoint 常驻 fork 多个 `xray run` 进程。
- 每个需要 local relay 的节点在 shared xray 内拥有独立的：
  - inbound tag
  - outbound tag（必要时附带该节点专属的 egress outbound tag）
  - routing rule tag
  - loopback socks 端口
- unchanged 节点复用原 handle；新增节点创建新 handle；变更节点创建新 generation handle 并立刻切换 `endpointUrl`。

### Hot update and draining

- save / refresh / revalidate 导致节点配置变化时：
  - 新请求必须立即命中新 handle。
  - 旧 handle 立刻退出 active 选择集合。
  - 旧 handle 转入 retiring，并按 lease 引用计数排空。
- retiring handle 的 lease 归零前不得移除其 rule / inbound / outbound，也不得删除 runtime 文件或回收其 loopback 端口。
- validate / probe / trace 的临时节点只能在 shared xray PID 内创建临时 handle；请求完成后必须释放 lease，并在 idle 时自动清理临时 handle 与 shared child（若 runtime 已空）。

### Client and endpoint contract

- `ForwardProxyEndpoint.endpoint_url` 继续保留“按节点可选路”的语义，但其值改为 shared xray 下的独立 loopback socks URL。
- `ForwardProxyClientPool` 继续按 `endpointUrl` 缓存 client；当节点 generation 切换导致 `endpointUrl` 变化时，新请求自动命中新 client，旧 client 仅服务仍持有 lease 的在途请求。
- Direct 节点仍走 native reqwest 直连，不纳入 shared xray 单实例约束。

### Startup recovery and health readiness

- 启动订阅刷新失败不得清空上一轮已持久化的 subscription 节点。恢复来源仅限本地 SQLite `forward_proxy_runtime` 中 `source=subscription` 且 `proxy_key` 仍可解析的非 direct 节点。
- 恢复后的节点必须进入正常 endpoint merge、runtime weight 与 shared xray sync 流程；不得复用上一进程留下的 loopback socks `endpointUrl`。
- 启动阶段完成 manager 初始化后必须主动执行 shared xray sync。xray 创建失败只反映到 readiness/runtime 状态，不应阻断进程启动。
- `/health` 的 xray readiness 只在存在 local relay 依赖时生效；没有 local relay 依赖时，宽限期后仍返回 `200 ok`。
- 启动宽限期默认 90 秒；测试可用短宽限期覆盖宽限后语义。

## Acceptance

- 在订阅展开为多 share-link 节点时，进程视角只能看到 `tavily-hikari` + **1 个 shared xray child**；不得再出现按节点常驻的 `xray run` 进程。
- 连续执行 settings save、subscription refresh、revalidate、validate 后，只要 relay runtime 仍存在，shared xray 的 PID 保持不变。
- unchanged 节点复用原 loopback endpoint；changed 节点切换到新 endpoint；removed 节点立即从 active stats/settings 中消失。
- 当旧 handle 仍有 lease 时，runtime 必须保留 retiring handle；lease 归零后 retiring handle 会被清掉，不遗留孤儿 handle、孤儿端口或 runtime 文件。
- validate-only 临时 handle 在结束后不会留下 shared child、临时 config 文件或永久滞留的 retiring entry。
- 重启且订阅源不可访问时，上一轮持久化的 subscription 节点仍出现在 forward proxy stats/settings，并触发 shared xray relay 同步。
- 启动宽限期内 `/health` 返回 `200 ok`；宽限期后，如果存在 xray relay 依赖但 shared xray 未就绪，返回 `503 xray not ready`；shared xray 就绪后返回 `200 ok`。

## Verification

- `cargo test -q xray_supervisor_ -- --nocapture`
- `cargo test -q tavily_proxy_save_and_revalidate_keep_shared_xray_pid -- --nocapture`
- `cargo test -q startup_restores_persisted_subscription_nodes_and_prewarms_xray_when_subscription_down -- --nocapture`
- `cargo test -q health_keeps_startup_grace_then_fails_when_xray_relay_is_not_ready -- --nocapture`
- `cargo test -q admin_forward_proxy_ -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
