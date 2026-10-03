# Admin 仪表盘请求趋势图表实现状态（#h2698）

## 当前实现

- `DashboardHourlyRequestWindow` 使用服务器本地时区对齐的 5 分钟桶，同时服务滚动 25
  小时柱图与最近 6 小时面积图。
- 请求结果与请求类型继续来自 `dashboard_request_rollup_buckets`；积分扩展复用同表的本地
  估算字段以及 `api_key_quota_sync_samples` 的有界样本差分。
- 前端六个模式由 `DashboardTrendPanel` 与 `dashboardHourlyCharts` 统一维护，偏好保存在管理端
  dashboard 的版本化 localStorage key 中。

## 当前变更

- 用 `积分 / 面积图 · 积分` 替换两张较昨日图。
- 为每个 5 分钟桶增加本地估算与 nullable 上游实扣数据。
- 更新 Storybook 稳定状态、交互测试与视觉证据。
- `request_logs` 保留期内新增持久化完整性工作项：按已有可见性时间索引受限分页聚合，在事务外完成源读，
  仅将确认存在差异的分钟桶放入短写替换事务。替换前受限 flush coalescer，并用 source-fence repair
  barrier 隔离迟到增量：已被源聚合覆盖的增量不重复写入，较晚增量重新入队并要求重审。
- 工作项、热窗口待审范围和确认缺口状态进入 overview/SSE；前端把未验证的 5 分钟槽作为 `null`
  绘制，而不是将其误报为零；完整性状态尚未建立时整段窗口同样留空。首次热窗口优先，历史每 60 秒最多
  一片，循环重审仅打开当前工作片的缺口。
- 过期热游标钳制到当前热窗口，五分钟终点从实际起点计算，历史检查点独立保留。新增热段在分页边界
  优先，GC 阻塞日其次；其有进展时一秒续跑，普通历史/滚动重审保持原节奏和慢写退避。
- GC 阻塞日幂等标记 `gc_blocking`，升级旧库默认置零，重复登记保留分页游标。全日已读完但收尾未完成
  的任务可在重启后完成日汇总、封存与队列收口，收口前不放行原始日志删除。
- 硬重启会丢弃未完成工作项的聚合 checkpoint 并以新 source fence 重新分页，不能复用只在进程内有效的
  版本标记。保留原始日志的 sealed 日如发现分钟统计与 seal 分歧，会排入逐片日回审；只有所有片完成后才
  重写该日 rollup 与 seal。日回审每片覆盖 5 分钟，普通日每分钟最多一片；GC 阻塞日按进展续跑，新的热片始终优先；被取消的现有源行
  修改会递增片版本，使下一次审计重新读取。
- 已验证本地日封存 JSON seal。保留期内的迟到数据修复会刷新 daily rollup 与 seal；GC 删除原始日志前
  检查最早可见候选日的 seal 及分钟、日级 rollup 一致性，已过期日的 daily rollup 可以由 seal 校验并恢复。
  仅含被抑制 retry shadow 的日期不属于 dashboard 事实源，不会无故阻塞日志 GC。
- 请求统计 coalescer 现在有可等待的关闭协议；服务在 graceful shutdown 返回后最多等待 20 秒 drain，
  Compose 给出 30 秒容器终止宽限。

## 验证状态

- 完整性回归覆盖过期热窗口、新热段抢先、源数据重建缺失/差异封存、账本保持不变、旧队列迁移及日末重启收尾。
- 旧版单库离线登记使用带运行时短 busy budget 的单条原子 UPSERT，避免同文件双别名的立即事务互锁。

- Rust：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。
- Web：`bun test`、`bun run build`、`bun run build-storybook`。
- 视觉：Storybook mock-only 的积分并排柱状图与重叠面积图已完成非空像素检查；owner 已授权提交 PR 图片证据。

## 状态

- Status: 已完成
- Created: 2026-04-07
- Last: 2026-07-24

## 里程碑

- [x] M1: spec 冻结与索引登记
- [x] M2: 后端 hourly bucket 聚合与 overview/snapshot 扩展
- [x] M3: DashboardOverview 图表模式、图例切换与 i18n
- [x] M4: Storybook / 前端测试 / 后端测试补齐
- [x] M5: 趋势窗口纠偏、面积图补充、缺口留空视觉证据、review-loop 与快车道收敛
- [x] M6: 本地统计完整性审计、日级 seal、在线修复状态与关闭收口
