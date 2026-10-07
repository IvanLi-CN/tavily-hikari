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
- 过期热游标钳制到当前热窗口，五分钟终点从实际起点计算，历史检查点独立保留。未排队的初始热页或新闭合
  热段在分页边界优先，GC 阻塞日其次；其有进展时一秒续跑，普通历史/滚动重审保持原节奏和慢写退避。
- GC 阻塞日的有进展续跑会在释放 SQLite bulk permit 前登记下一轮 fair ticket，避免后续滚动维护持续越过
  清理续跑，同时保留既有前台、连接池和 contention admission 保护。
- 前台压力、连接池压力和近期 SQLite contention 让完整性任务退避五分钟；独占 bulk admission 被占用时每五秒
  重查，避免慢写或竞争下反复争用 SQLite。
- GC 阻塞日幂等标记 `gc_blocking`，升级旧库默认置零，重复登记保留分页游标。全日已读完但收尾未完成
  的任务可在重启后完成日汇总、封存与队列收口，收口前不放行原始日志删除。
- 硬重启会丢弃未完成工作项的聚合 checkpoint，并用 durable source revision 重新判断分页是否需要重做；GC
  阻塞日 recovery fence 持久化并在后续运行复用，mutable source row 更新会触发对应日期重新回审。保留原始日志的 sealed 日如发现分钟统计与 seal 分歧，会排入逐片日回审；只有所有片完成后才
  重写该日 rollup 与 seal。日回审每片覆盖 5 分钟，普通日每分钟最多一片；GC 阻塞日按进展续跑，新的热片始终优先；被取消的现有源行
  修改会递增片版本，使下一次审计重新读取。
- 已验证本地日封存 JSON seal。保留期内的迟到数据修复会刷新 daily rollup 与 seal；GC 删除原始日志前
  检查最早可见候选日的 seal 及分钟、日级 rollup 一致性，已过期日的 daily rollup 可以由 seal 校验并恢复。
  仅含被抑制 retry shadow 的日期不属于 dashboard 事实源，不会无故阻塞日志 GC。
- 日级 seal 现在同时保存 source fence 与 durable source version；已完成日期的外部更新/删除会创建或重置 GC-blocking re-audit，fenced GC 则在同一短事务内复核并抑制自身删除 trigger。晚到可见 INSERT 会推进目标 fence 并重新打开已完成的固定目标。
- 已封存日经过授权 GC 后，可见源的 `MAX(id)` 下降不会被误判为外部变化；新增或更新使 fence/version 前移仍会阻塞并触发回审。
- 固定目标恢复通过目标日范围调用 GC，普通在线 GC 仍按全局最早候选日运行；目标过期计数包含可见和被抑制的源日志，避免只删可见行后留下目标债务。
- 旧库若缺少已删除源贡献基线，恢复不会用保留行重建较小的 daily rollup，而是保持 GC 阻塞并等待可验证的来源。
- recovery bootstrap 会为旧版 rollup 表补齐 recovery 依赖的 `valuable_failure_429_count` 列；本地日边界由 SQLite local-time 日期计算下一次零点，覆盖 DST 边界。
- 请求统计 coalescer 现在有可等待的关闭协议；服务在 graceful shutdown 返回后最多等待 20 秒 drain，
  Compose 给出 30 秒容器终止宽限。

## 验证状态

- 完整性回归覆盖过期热窗口、新热段抢先、源数据重建缺失/差异封存、账本保持不变、旧队列迁移及日末重启收尾。
- 新登记的 GC 阻塞日会在已有普通历史/滚动分页之前创建工作项；既有热片仍优先，普通页的游标和累计结果保留。
  GC 同时检查候选日尚未完成的源工作项和 rebalance 恢复，三份旧汇总一致也不能提前删除其源日志。
- 旧版单库离线登记使用带运行时短 busy budget 的单条原子 UPSERT，避免同文件双别名的立即事务互锁。

- Rust：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。
- Web：`bun test`、`bun run build`、`bun run build-storybook`。
- 视觉：Storybook mock-only 的积分并排柱状图与重叠面积图已完成非空像素检查；owner 已授权提交 PR 图片证据。

## 状态

- Status: 已完成
- Created: 2026-04-07
- Last: 2026-10-07

## 里程碑

- [x] M1: spec 冻结与索引登记
- [x] M2: 后端 hourly bucket 聚合与 overview/snapshot 扩展
- [x] M3: DashboardOverview 图表模式、图例切换与 i18n
- [x] M4: Storybook / 前端测试 / 后端测试补齐
- [x] M5: 趋势窗口纠偏、面积图补充、缺口留空视觉证据、review-loop 与快车道收敛
- [x] M6: 本地统计完整性审计、日级 seal、在线修复状态与关闭收口
