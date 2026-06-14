# Tavily Hikari 基于 EdgeOne 源站切换的单活主备高可用改造

## Summary

Tavily Hikari 的高可用方案采用单活主备热备，而不是一主多从负载均衡。任一时刻只有一个实例处理完整业务，EdgeOne 当前源站 `IP:port` 或源站组 ID 是 active master 的权威标识。standby 持续同步 active 的 SQLite 数据；active 故障时，standby 通过 EdgeOne API 切换源站到自己，先进入 `provisional_master`，恢复基础 API/MCP 服务，再由管理员确认后进入 `full_master`。

## Goals

- 只暴露一个业务域名，并使用 EdgeOne 源站切换完成 active 节点切换。
- 容忍 active 或 standby 任一节点离线 24 小时。
- 自动 failover 只恢复基础 API/MCP、鉴权和 quota 扣减。
- 注册、充值、配置写入、上游 key 管理等高风险写入必须等管理员 finalize。
- 旧主恢复后只补传可幂等合并数据，不覆盖新主配置类状态。

## Non-Goals

- 不实现一主多从负载均衡。
- 不实现跨从额度租约或 token 配额派发。
- 不合并多从 rebalance 映射状态。
- 不依赖 EdgeOne 免费版的原生负载均衡能力。

## EdgeOne Control Plane

- `DescribeAccelerationDomains` 用于查询加速域名当前源站，并判断当前 active 节点。
- `ModifyAccelerationDomain` 用于将源站切换到目标节点 `IP:port`。
- 节点切换必须记录 operation、请求、响应、错误和操作者审计。
- 首个上线门槛是验证 EdgeOne 是否接受带端口的 origin；若不支持，主备节点必须监听相同端口。

## Node State Machine

- `full_master`：完整服务，允许业务 API、MCP、控制台写入、注册和充值。
- `provisional_master`：自动 failover 后状态，只允许基础业务 API/MCP、鉴权和 quota 扣减；控制台显示降级，只读为主。
- `standby`：只同步、健康检查、可被 promote；不处理外部业务写入。
- `recovery`：旧主恢复后的状态，只补传可合并数据，禁止重新抢主。

## Data Sync And Recovery

- HA 同步的目标是保活服务，不复制完整历史分析库。
- standby 每 5-15 秒从 active 拉取状态基线或 outbox 增量事件，目标 RPO `<=15s`。
- 禁止通过 HA 同步传输全量 SQLite 数据库文件。
- 状态基线与事件流使用 versioned zstd NDJSON，基线压缩后上限 `64MiB`，事件批次压缩后上限 `4MiB`。
- active 持久化 `ha_outbox` 事件，standby 按递增 seq 幂等应用并回报 peer watermark；outbox 保留窗口为 72 小时，超过窗口必须重新拉状态基线。
- 基线和事件只允许同步接管基础 API/MCP 所需状态：API keys、auth tokens、用户身份、token/key/user 绑定、MCP 当前会话必要状态、quota 当前状态与聚合 bucket、控制面配置、公告、LinuxDo 充值订单/权益、账本历史、forward proxy 配置、代理节点 override、上游 key 与代理节点绑定/亲和关系。
- 禁止同步 `request_logs`、`auth_token_logs`、请求体、响应体、path/query/IP/header 明细、dashboard recent logs、OAuth login 临时态、Web session、forward proxy runtime/attempts/hourly weight 和节点本地观测噪声。
- recovery 只允许导入幂等账本事件，不导入调用记录，不覆盖新主当前权威状态。
- recovery 完成后 quota 与 usage 聚合必须可继续滚动更新。

## API Contract

- `GET /api/admin/ha/status` 返回当前节点状态、EdgeOne 源站、同步水位、recovery 状态。
- `GET /api/ha/status` 返回可公开给用户控制台的降级摘要，不包含 secret 或 expected origin。
- `GET /api/admin/ha/status` 还要返回当前/预期源站类型、本地默认源站、本地覆盖源站和当前 EdgeOne target。
- `PUT /api/admin/ha/source` 保存当前服务节点私有源站配置，可在 `IP/域名` 与 `源站组` 间切换，并可选择保存后立即应用到 EdgeOne。
- `GET`/`PUT /api/admin/ha/snapshot` 是废弃接口，必须返回 `410 Gone`，不得读写 SQLite 数据库文件。
- `GET /api/admin/ha/baseline` 仅内部或管理员认证可调用，在 active/provisional 节点输出 zstd NDJSON 状态基线，并在响应头返回 high watermark。
- `GET /api/admin/ha/events?after=<seq>&limit=<n>` 仅内部或管理员认证可调用，输出 `after` 之后的 zstd NDJSON outbox 事件。
- `POST /api/admin/ha/events/ack` 仅内部或管理员认证可调用，记录 standby 已应用的 outbox seq。
- `POST /api/admin/ha/promote` 将当前 standby 切为 `provisional_master`，可带 `force` 用于强制接管。
- `POST /api/admin/ha/finalize` 管理员确认后进入 `full_master`。
- `POST /api/admin/ha/recovery/import` 导入旧主 recovery 账本批次，仅允许内部或管理员认证调用；调用记录字段必须被拒绝。

## Runtime Configuration

- `HA_MODE=single|active_standby`
- `NODE_ID`
- `HA_SOURCE_KIND=direct|origin_group`
- `HA_SOURCE_ORIGIN_GROUP_ID`
- `NODE_PUBLIC_SCHEME=http|https|follow`
- `NODE_PUBLIC_HOST`
- `NODE_PUBLIC_PORT`
- `EDGEONE_ZONE_ID`
- `EDGEONE_DOMAIN`
- `EDGEONE_EXPECTED_ORIGIN_SCHEME=http|https|follow`
- `EDGEONE_EXPECTED_ORIGIN_HOST`
- `EDGEONE_EXPECTED_ORIGIN_PORT`
- 节点私有源站保存值优先于 Env/CLI 默认值，但只作用于当前实例，不参与 HA 同步。`EDGEONE_EXPECTED_ORIGIN_*` 仍只表示直连预期源站。
- `EDGEONE_SECRET_ID`
- `EDGEONE_SECRET_KEY`
- `HA_SYNC_SOURCE_URL`（standby 拉取 active 的内部 URL）
- `HA_INTERNAL_TOKEN`
- `HA_SYNC_INTERVAL_SECS`

## UI Contract

- 用户控制台在 failover、provisional、recovery、同步滞后时显示降级警告。
- 管理员控制台的完整 HA 服务节点管理面板只出现在系统设置的高可用二级界面，包含节点清单、角色、源站、健康状态、同步水位、promote/finalize 操作和 EdgeOne 当前源站摘要。
- HA 管理页还要提供当前节点源站配置入口，允许在 `IP/域名` 与 `源站组` 之间切换，并在 active/provisional 时支持保存后切换 EdgeOne 到此源站。
- 管理员业务页面在 `full_master` 正常态不得显示 HA 面板；在 failover、standby、recovery 或写入受限时，只显示紧凑异常提示并链接到系统设置的高可用界面，不直接执行 promote/finalize。
- `provisional_master` 阶段必须明确提示注册、充值和配置写入仍被禁用。

## Visual Evidence

PR: include

![Normal admin dashboard without HA panel on desktop](./assets/ha-normal-dashboard-desktop.png)

PR: include

![Normal admin dashboard without HA panel on mobile](./assets/ha-normal-dashboard-mobile.png)

PR: include

![Abnormal HA compact alert on dashboard desktop](./assets/ha-compact-alert-desktop.png)

PR: include

![Abnormal HA compact alert on dashboard mobile](./assets/ha-compact-alert-mobile.png)

PR: include

![System settings HA service-node page on desktop](./assets/ha-settings-page-desktop.png)

PR: include

![System settings HA service-node page on mobile](./assets/ha-settings-page-mobile.png)

PR: include

![System settings HA source configuration entry on desktop](./assets/ha-settings-origin-panel.png)

PR: include

![System settings HA action buttons contained in the table](./assets/ha-action-buttons-contained.png)

PR: include

![HA source settings dialog using an EdgeOne origin group](./assets/ha-source-dialog-polished.png)

PR: include

![HA source settings dialog using a direct IP/domain origin](./assets/ha-source-dialog-direct-polished.png)

PR: include

![HA source settings dialog using an EdgeOne origin group with selected-source summary](./assets/ha-source-dialog-origin-group-polished.png)

PR: include

![HA source settings dialog on a standby node with save-only action](./assets/ha-source-dialog-standby-save-only.png)

## Acceptance

- `standby/recovery` 禁止外部业务写入。
- `provisional_master` 允许 API/MCP/quota，禁止注册、充值、配置写入。
- `finalize` 后恢复完整功能。
- EdgeOne 当前源站与本节点 origin 一致时，节点可识别自己为 active。
- EdgeOne API 失败、源站不匹配、并发 operation 不产生双 active。
- 旧主 recovery batch 重复导入幂等。
- 双节点 mock EdgeOne 验收必须覆盖 `pre -> failover -> recovery`：单入口业务流量、standby
  fencing、状态基线、outbox 增量 catch-up、standby promote、provisional gating、finalize 后 full
  write、旧主账本 recovery 和重复导入幂等。
- 大量调用记录和大请求/响应正文不得进入 HA baseline、events 或 recovery payload。
