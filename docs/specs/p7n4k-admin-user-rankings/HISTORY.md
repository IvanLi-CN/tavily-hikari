# Admin 用户排行演进历史（#p7n4k）

> 这里记录会影响后续理解“为什么实现收敛到当前形态”的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-06-19: 新增独立 `/admin/rankings` 模块，锁定三个滚动时间窗（`24h / 7d / 30d`）与双榜指标（`primary_success`、`business_credits`）。
- 2026-06-19: 后端合同固定为同形 HTTP 快照 + 独立 SSE；排行 row 固定返回 `rank`、`value`、`userId/displayName/username/avatarUrl`。
- 2026-06-19: 数据路径确定为用户级 rollup + partial bucket 补扫 + 10 秒 snapshot cache/singleflight，避免实时榜单每轮回扫 30 天原始日志。
- 2026-06-19: 前端从“图外身份列 + 图内柱条”的拆分方案收敛到“单一 chart surface”，以更接近常规排行榜图表的呈现方式内嵌 rank、头像 fallback 与单一显示名。
- 2026-06-19: 根据验收反馈，去除 secondary identity 与重复用户信息，Storybook 默认数据扩展为完整 `TOP20`，确保视觉证据与接口合同一致。
- 2026-06-19: 根据 critique 收口，页面从“六张榜同时平铺”调整为“时间窗切换 + 当前窗口双榜”，并新增实时状态条、断连重试与 DOM 语义 fallback。
- 2026-06-19: 根据最新验收反馈，头像策略从“缺失时首字母圆牌”收敛为“真实头像优先，缺失或加载失败时稳定 mock avatar”，避免整榜视觉过假；横轴同步取消共享固定刻度，改为每榜按当前最大值自适应。
- 2026-06-20: 根据最新验收反馈，交互从“时间窗切换 + 双榜”改为“6 个单选 tab + 三榜内容区”；tab 固定覆盖 `24h / 7d / 30d / 主要调用 / 积分 / IP`，默认 `24h`。
- 2026-06-20: 新增 `IP` 排行维度，定义为滚动时间窗内用户唯一 `client_ip` 数；沿用可见请求口径，不附加成功或计费约束。
- 2026-06-20: 修复 `rankings` 导航图标缺失的根因，补齐 `mdi:trophy-outline` 离线图标注册与 story/runtime 导航一致性。
- 2026-06-21: 根据最新视觉验收反馈，榜单实现继续从“ECharts + DOM overlay”混合方案收口为纯 `ECharts custom series` 输出；旧的 `.admin-ranking-chart-overlay` 与 `.admin-ranking-row-label` 假标签层被彻底移除。
- 2026-06-22: owner-facing 视觉证据从混用的 Storybook / live / chrome 过程截图收敛为统一 `web demo` 证据链，并清理重复资产与临时截图，避免同一排行模块继续残留两套验收口径。
- 2026-06-25: admin 信息架构新增 `分析` 父模块后，排行从独立一级模块收拢为 `分析 -> 排行` 子模块；canonical 路由迁到 `/admin/analysis/rankings`，旧 `/admin/rankings` 保留为兼容别名。
- 2026-06-25: 根据最新验收反馈，彻底否定“双轴 tabs”解读，合同锁定为六个独立单选 tab；路由改为单一 `tab` 查询参数，缺失或非法值统一规范化为 `last24h`。
- 2026-06-25: 根据“返回过来后数据不能丢”的要求，排行 snapshot 真相源上提到 runtime；从用户详情返回、浏览器前进后退与 tab 切换都复用已有 snapshot，只做后台刷新，不再回骨架首屏。
- 2026-06-25: 根据交互验收，排行项新增 click 跳转用户详情，以及 hover / focus 同用户跨三榜联动高亮，联动范围只限当前可见 tab 下的三张榜。

## Key Reasons / Replacements

- 用户明确要求“只允许做 charts”，因此最终实现不再保留图表外的独立身份列或重复文本块。
- 用户明确拒绝一个 row 上展示多份身份信息，因此昵称/用户名分层显示被收敛为单一显示名。
- 用户要求 `TOP20` 必须真实可见，因此 Storybook 默认场景必须使用完整 20 行 mock 数据，而不是截断示例。
- critique 指出六榜同时展示的扫描负担过高，且 canvas-only 信息层不利于可访问性，因此最终切到单时间窗主视图，并为榜单补充语义 DOM 同步层。
- 用户进一步明确“六个 tab 是单选的”，但内容区每次仍需同时展示三张榜，因此页面收敛为“单个 active tab 决定三张卡片的分组视角”，而不是组合态单榜视图。
- 用户将新增维度定义为“IP”，且要求它与积分、主要调用并列，因此原双榜合同被替换为稳定三榜合同。
- 用户继续指出“看起来不像成熟图表库实现”，因此最终证据与实现都必须证明：排行页只保留单一 chart surface，而不是图表下方再叠一层业务侧标签 DOM。
- 用户明确要求截图必须来自 `web demo` 而不是 Storybook，因此最终 owner-facing 视觉证据统一切到 demo 路由，旧的 story/live 中间图不再保留在 spec 资产里。
- admin 运营入口现在需要把排行、用量与压力放到同一分析语义下，因此排行不再单独占据一级导航位，避免 admin 模块粒度继续分叉。
- 用户明确指出“这是六个独立 Tab”，因此旧的“指标 tab 只切单选态、不切内容”定义被彻底废弃，避免后续再次误读成双轴模型。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
