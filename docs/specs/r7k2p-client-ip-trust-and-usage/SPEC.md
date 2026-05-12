# 7 天用户调用 IP 统计与可信客户端 IP 设置

## Goal

统计最近 7 天去重调用 IP 数，并为管理员提供可信代理 CIDR + 客户端 IP 请求头顺序配置，以及近期请求中的实际头值辅助确认。

## Scope

- 可信代理 CIDR 与有序 IP 头配置持久化。
- 仅在可信代理命中时解析 `clientIp`，并记录 `remoteAddr`、`clientIpSource`、可信状态与 IP 头快照。
- IP 头快照保存已配置头与安全预设头的并集，便于管理员在启用 Cloudflare、EdgeOne 等头名前回看近期样本。
- 近期请求 IP 诊断只展示有下游/API/tool 调用诊断价值的记录；空 IP 的 rebalance 本地 MCP 控制面日志不得挤占样本列表。
- 默认仅信任 loopback 代理地址；私网或容器网段必须由管理员确认后显式加入。
- 客户端 IP 头配置拒绝 `authorization`、`cookie`、API key 等敏感头名，避免误配置后落库秘密值。
- 用户维度最近 7 天 `COUNT(DISTINCT clientIp)`。
- 系统设置对话框、近期请求 IP 诊断、用户列表/详情 IP 数展示。

## Non-goals

- 不做自动封禁或阈值拦截。
- 不记录 Authorization、Cookie 等非 IP 敏感头完整值。

## Acceptance

- 后端可安全解析并落盘 IP 审计字段。
- 管理端可编辑可信代理 CIDR 与头顺序，可通过每个头名独立按钮快速追加通用反代、Cloudflare、EdgeOne 等常用头名，并查看近期观测值。
- 近期观测值列表不会被 rebalance 本地 `initialize`、`tools/list`、`ping` 等空 IP 控制面日志刷屏。
- 用户列表/详情可展示最近 7 天去重 IP 数。
- Recent Requests 可展示 IP 诊断信息与头值快照。

## Visual Evidence

- source_type: storybook_canvas
  story_id_or_title: Admin/SystemSettingsModule/ClientIpDialogWithObservedValues
  scenario: trusted client IP settings dialog with observed header values
  evidence_note: verifies the dialog exposes trusted proxy CIDRs, ordered client IP headers, and recent observed header values.
  image:
  ![Trusted client IP settings dialog](./assets/client-ip-settings-dialog.png)

- source_type: storybook_canvas
  story_id_or_title: Admin/Components/AdminRecentRequestsPanel/LazyDetailsGallery
  scenario: recent request detail expanded with IP diagnostics
  evidence_note: verifies recent request details show remoteAddr, resolved clientIp, source header, trusted state, and IP header snapshots.
  image:
  ![Recent request IP diagnostics](./assets/recent-requests-ip-diagnostics.png)
