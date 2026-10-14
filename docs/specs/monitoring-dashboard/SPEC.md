# Monitoring dashboard：运行时观测集成

## Context and Scope

Tavily Hikari 是 Rust/Axum 代理服务，React 管理台和 SQLite 业务持久化已经具备业务汇总与结构化 runtime logs，但当前没有 Prometheus exporter、仓库内 Grafana provisioning 或面向长期诊断的统一观测合同。

101 上已经部署共享 Prometheus、Grafana 和可选 Tempo stack；本项目需要接入这套实例，同时保持仓库在其它容器编排、单机和本地环境中的通用性。

现有 `GET /api/public/metrics/v1/tavily-hikari` 是公开展示用的聚合 JSON 接口，只返回批准的业务汇总，不承担 Prometheus 抓取职责。

## 目标

- 从真实 source event 产生有界、低基数的 Counter、Gauge 和 classic Histogram，覆盖代理请求、upstream、SQLite、后台任务和运行时资源。
- 提供可选、内部鉴权的 `/metrics` exporter；观测未启用或观测 backend 故障时，代理请求、ACK、任务和业务 SQLite 写入继续遵守既有 contract。
- 将 Prometheus scrape、recording rules、Grafana datasource、dashboard 和 alert rules 作为仓库内可审查的 provisioning 配置。
- 以四个固定排障入口交付第一版 dashboard：Overview、Proxy、Storage、Runtime。
- 为 101 提供实例适配清单，但不把域名、内网地址、secret 值、宿主端口、retention 或性能预算写成项目默认值。

## 非目标

- 不把 Prometheus 历史写入业务 SQLite，也不以业务表回填 runtime metrics。
- 不修改既有 public metrics API 的路径、响应字段、缓存、CORS 或匿名访问语义。
- 首期不在 Hikari React 管理台再造 Prometheus 图表，不把 Grafana UI 变成配置真相。
- 首期不强制 tracing、Tempo、hotpath 或常驻 profiler；这些能力必须拥有独立 spec、凭据、限额和验证合同。
- 不把 user/token/key/IP/request-id、原始 URL/query/body、SQL 文本/参数或完整错误文本放入 label 或 metric payload。

## 领域边界

- 服务 exporter 负责从 source event 更新进程内有界 recorder。
- Prometheus 负责 scrape、时间序列历史和 recording rules。
- Grafana 负责查询、展示和 alert rule evaluation；dashboard、datasource 和 rule provisioning 由仓库文件管理。
- 业务 SQLite 继续保存请求、账本、配额、任务和审计事实；runtime metrics 不取代这些事实。
- 101 的 `obs` stack 是一个部署实例，不是本项目的唯一运行方式；项目必须支持其它 Prometheus/Grafana 部署或完全关闭观测。

## 接口与配置合同

### `GET /metrics`

- 仅当 `OBSERVABILITY_ENABLED=true` 且 `METRICS_BIND` 配置有效时提供。
- 使用 Prometheus text exposition；首期应用指标只使用 classic Counter、Gauge 和 Histogram。
- 必须使用 `Authorization: Bearer <token>`，token 从 `METRICS_TOKEN_FILE` 读取；缺失、不可读或不匹配时返回 401/403。
- 该 route 只能在内部网络或本机监听；不通过 Traefik 公网暴露，也不得复用 public metrics API 的 CORS 逻辑。
- exporter 初始化失败不得阻塞业务服务启动；失败原因只能进入脱敏 runtime log，不能回显 secret 或请求内容。

### 配置项

- `OBSERVABILITY_ENABLED`：默认 `false`，显式开启 runtime metrics。
- `METRICS_BIND`：启用后必须显式指定监听地址和端口，默认只允许 loopback 或 RFC1918/private 地址；项目不固化某个宿主端口。
- `METRICS_ALLOW_UNSPECIFIED_BIND`：默认 `false`；仅在部署网络本身负责隔离且经过部署审查时，显式允许 `0.0.0.0`/`::` 绑定。
- `METRICS_TOKEN_FILE`：只读 Bearer token 文件路径；应用与 Prometheus 的凭据必须分离时由编排层分别挂载。
- `OBSERVABILITY_ENVIRONMENT`：固定环境枚举，只接受 `development`、`staging`、`production` 或 `test`；观测启用时缺失或非法值会 fail-closed 并保持业务 listener 可用。
- `OBSERVABILITY_INSTANCE`：固定部署槽位，只接受 `primary`、`secondary`、`standby`、`local`、`test` 或 `hikari-<numeric ordinal>`；观测启用时缺失或非法值会 fail-closed 并保持业务 listener 可用。
- `OBSERVABILITY_ROUTE_ALLOWLIST`：可选的 route family allowlist；未配置时使用代码内固定集合。

## Requirements

### REQ-MONITORING-001：运行时开关与业务隔离

系统 MUST 允许在不改变业务 route 和持久化语义的情况下开启或关闭 exporter；关闭观测后业务测试结果、public metrics API 和 runtime logs 的既有 contract 保持不变。

### REQ-MONITORING-002：真实 source event

系统 MUST 仅在真实请求阶段、upstream 尝试、SQLite 工作、后台任务或资源采样事件发生时更新指标，不得用业务表回填、周期随机样本或 dashboard 查询结果伪造 metrics。

### REQ-MONITORING-003：有界低基数

系统 MUST 对 metric 名称、label key、label value 和动态 recorder 数量设定代码级上限；展示层或 Grafana 查询的 limit 不能替代采集侧限制。

### REQ-MONITORING-004：指标协议

首期应用 metrics MUST 使用 `hikari_` 命名空间和 classic Histogram；PromQL、recording rules、dashboard 和 tests MUST 使用与 exporter 一致的 bucket 语义和单位。

### REQ-MONITORING-005：label allowlist

首期只允许 `service`、`environment`、`instance`、`route_family`、`method`、`status_class`、`outcome`、`operation`、`phase`、`workload_class`、`channel`、`reason` 和 `upstream_class`；每个动态维度必须来自代码内固定枚举或 deployment allowlist。

### REQ-MONITORING-006：公开接口分离

系统 MUST 保持 `/api/public/metrics/v1/tavily-hikari` 作为匿名、聚合、展示用 JSON API；不得把 exporter 指标、内部延迟、资源、重试、队列或存储状态加入该接口。

### REQ-MONITORING-007：仓库 provisioning

仓库 MUST 提供 Prometheus scrape job、recording rules、Grafana datasource、dashboard provider、dashboard JSON 和 alert rules 的文件化配置；Grafana UI 修改不得成为唯一配置真相。

### REQ-MONITORING-008：固定 dashboard

首期 MUST 提供 `Hikari Overview`、`Hikari Proxy`、`Hikari Storage` 和 `Hikari Runtime` 四个固定 dashboard，使用稳定 UID、只读 datasource 和可替换的 deployment variables。Dashboard MUST 保留 `service`、`environment`、`instance` 过滤，并提供 target health、样本量、错误率、P50/P95/P99、route/upstream/phase/storage/runtime 分解和 recorder 丢样本状态，使值班人员可以从症状定位到责任边界，而不是只能确认 exporter 有数据。

### REQ-MONITORING-009：告警语义

首期 MUST 提供 exporter unavailable、scrape stale/低样本、错误率、请求 P95、SQLite wait 和 memory headroom 六类可配置规则；规则必须先检查 freshness 和样本量，再解释阈值，通知 contact point 由部署实例管理。

### REQ-MONITORING-010：部署通用性

仓库 MUST 提供可选 `ops/observability` 配置和 Compose overlay/profile；部署不得依赖 101 的域名、IP、secret 路径或宿主端口。101 适配只需把 Hikari 加入 `obs` 私网、增加带 Bearer file 的 scrape target、挂载应用 token 并设置 environment/instance。

### REQ-MONITORING-011：故障降级

Prometheus、Grafana、exporter scrape、recording rule、dashboard query 或可选 trace backend 停止时，代理请求、ACK、后台任务和业务 SQLite 持久化 MUST 继续完成；观测丢弃、刷新失败和 exporter 降级必须可通过有界指标或脱敏日志识别。

### REQ-MONITORING-012：可验证性

实现 MUST 验证鉴权、schema、label cardinality、敏感字段拒绝、Counter reset、Histogram 分位数、freshness、样本量、Prometheus target、recording rule、dashboard query、组件停机隔离和观测开销；“容器启动”不构成完成证明。

## 指标语义

首版指标名称、类型、单位、labels、枚举和禁止字段见 [`METRICS.md`](./METRICS.md)。请求阶段和 SQLite workload 必须保留现有 runtime logs 的因果边界，但 metrics 只记录聚合计数、时长和状态，不复制日志全文。

## 交付分期

- **M1：应用 instrumentation**：有界 recorder、metrics catalog、`/metrics` token auth、配置开关和业务隔离。
- **M2：仓库观测配置**：Prometheus scrape/recording rules、Grafana datasource/dashboard/alerts provisioning、schema/cardinality/query tests。
- **M3：101 实例适配**：更新实例侧 deployment card/Compose/secret mapping，接入 `obs` 网络，验证 target freshness、dashboard query、故障隔离和真实浏览器渲染。
- **M4：可选诊断扩展**：tracing 或 hotpath 各自建立独立 spec、凭据和容量验收，不阻塞 M1-M3。

## Verification

- **VER-MONITORING-001**：本地 HTTP smoke 和单元测试覆盖正确、缺失、错误及不可读 token 的 exporter 行为，covers: `REQ-MONITORING-001`, `REQ-MONITORING-012`。
- **VER-MONITORING-002**：关闭 `OBSERVABILITY_ENABLED` 后 exporter 不监听，业务 health、public metrics API、请求处理和持久化回归通过，covers: `REQ-MONITORING-001`, `REQ-MONITORING-006`, `REQ-MONITORING-011`。
- **VER-MONITORING-003**：metrics fixture、allowlist 测试和敏感字段扫描验证名称、类型、单位、labels、未知值和 recorder 边界，covers: `REQ-MONITORING-002`, `REQ-MONITORING-003`, `REQ-MONITORING-004`, `REQ-MONITORING-005`。
- **VER-MONITORING-004**：Prometheus 配置、recording rules、Counter reset、classic Histogram 分位数和 freshness/sample-count fixture 可重复执行，covers: `REQ-MONITORING-004`, `REQ-MONITORING-007`, `REQ-MONITORING-009`, `REQ-MONITORING-012`。
- **VER-MONITORING-005**：四个 dashboard 的 datasource UID、dashboard UID、变量筛选、诊断面板、无样本/低样本/stale/错误状态和告警加载在隔离 provisioning 与实际渲染检查中验证，covers: `REQ-MONITORING-007`, `REQ-MONITORING-008`, `REQ-MONITORING-009`。
- **VER-MONITORING-006**：mock traffic 与 exporter interruption smoke 验证代理请求、ACK、后台任务和 SQLite 写入继续完成；101 的 target、Grafana rendering 和 retention/阈值在 owner 授权的部署验收中验证，covers: `REQ-MONITORING-010`, `REQ-MONITORING-011`, `REQ-MONITORING-012`。

## Visual Evidence

本轮视觉证据使用 pinned Grafana `11.2.0` 与 Prometheus `2.55.1` 的隔离本机进程，file-SD 指向 mock/sandbox exporter，未连接 101 或官方 Tavily endpoint。Prometheus target 为 UP，Grafana datasource UID 为 `hikari-prometheus`，四个 dashboard 均由仓库 provisioning 自动加载并显示真实查询曲线；Overview、Proxy、Storage、Runtime 分别渲染 11、9、8、11 个面板，包含变量筛选、目标健康、分位数、路由/upstream/SQLite/后台任务和 exporter 压力信号。

- [Hikari Overview](./assets/hikari-overview.png)
- [Hikari Proxy](./assets/hikari-proxy.png)
- [Hikari Storage](./assets/hikari-storage.png)
- [Hikari Runtime](./assets/hikari-runtime.png)

## 相关文档

- [`METRICS.md`](./METRICS.md)
- [`IMPLEMENTATION.md`](./IMPLEMENTATION.md)
- [`HISTORY.md`](./HISTORY.md)
- [`ADR 0009`](../../adr/0009-runtime-metrics-ownership-boundary.md)
- [`runtime-performance-observability/SPEC.md`](../runtime-performance-observability/SPEC.md)
- [`blog-runtime-public-api/SPEC.md`](../blog-runtime-public-api/SPEC.md)

## Related ADRs

- [ADR 0009](../../adr/0009-runtime-metrics-ownership-boundary.md)
