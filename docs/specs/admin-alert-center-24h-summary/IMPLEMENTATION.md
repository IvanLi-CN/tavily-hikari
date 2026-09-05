# Admin 告警中心与 24h 仪表盘告警摘要 实现状态

## 状态

- Status: 已完成
- Created: 2026-04-18
- Last: 2026-04-18

## 实现里程碑

- [x] M1: spec / contract 冻结并登记索引
- [x] M2: 后端告警读模型、catalog / events / groups API、dashboard recentAlerts 完成
- [x] M3: 前端告警中心、共享筛选、请求详情抽屉、dashboard 摘要完成
- [x] M4: Storybook、浏览器验收与视觉证据完成
- [x] M5: 快车道 PR 收口到 merge-ready

## Canonical warm recovery

- Canonical catalog, events page `1/20`, and groups page `1/20` are staged by
  independent bounded `AdminAlertsCacheWarm` reads and published as one set.
- Every durable alert projection advance moves the cache generation fence,
  including history-only slices; a deferred, cancelled, or generation-mismatched
  warm attempt publishes nothing and leaves the exact-key last-good entries intact.
