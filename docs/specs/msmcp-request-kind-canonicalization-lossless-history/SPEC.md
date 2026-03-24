# Request Kind 白名单 Canonical 化与无损历史整理（#msmcp）

## 状态

- Status: 已实现（待审查）
- Created: 2026-03-24
- Last: 2026-03-24

## 背景 / 问题陈述

- `request_kind_key/label/detail` 目前混入了 `mcp:raw:/mcp/search`、`api:raw:/api/custom/raw`、`mcp:tool:acme-lookup` 这类 path / tool-name 编码值。
- 这些动态值把“分类字段”和“原始请求事实”耦合在一起，导致历史日志 facet 爆炸、白名单失守、管理页筛选持续暴露历史坏路径。
- 现有日志表已经分别保存了 `method/path/query/request_body/response_body`，因此 request kind 应回归为稳定 canonical 分类，原始细节另行保留。

## 目标 / 非目标

### Goals

- 把 `request_kind_*` 扶正为主 canonical 分类字段，只允许白名单稳定 key 落库与对外返回。
- 为 `request_logs` 与 `auth_token_logs` 新增 `legacy_request_kind_key/label/detail`，无损保留旧主字段快照。
- 新写入与查询 facet 全部切到 canonical 分类，不再产生 path/tool-name 动态 key。
- 保留原始 `method/path/query/request_body/response_body` 事实字段，并把必要的 path / method / tool-name 细节放入 canonical `request_kind_detail`。
- 提供独立 backfill binary，按批把两张日志表的历史主字段 canonical 化，同时保留 legacy 快照，避免启动迁移重写大表。

### Non-goals

- 不删除历史日志，不压缩日志，不篡改请求/响应事实字段。
- 不在本次兼容窗口内删除 `legacy_request_kind_*`。
- 不改变计费真相源：`business_credits`、`counts_business_quota`、`failure_kind`、`key_effect_*` 仍按现有语义工作。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `msmcp-request-kind-canonicalization-lossless-history` 索引。
- `src/analysis.rs`
  - 固化 canonical request kind catalog。
  - 去掉 path/tool-name 动态主 key 生成。
  - 提供 legacy alias -> canonical 映射与 request/request-log canonical 化辅助函数。
- `src/models.rs`
  - 为 `RequestLogRecord`、`TokenLogRecord` 增加 `legacy_request_kind_*` 字段。
- `src/store/mod.rs`
  - 两张日志表 schema 新增 `legacy_request_kind_*`。
  - 查询、facet、过滤统一按 canonical kind 工作。
  - 新增历史 canonical backfill 所需的持久化/游标辅助能力。
- `src/server/{dto,proxy}.rs`
  - DTO/view 暴露 canonical `requestKind*` 与新增 `legacyRequestKind*` 审计字段。
  - `request_kind` 查询参数兼容 legacy alias，但默认命中 canonical 结果。
- `src/bin/request_kind_canonical_backfill.rs`
  - 新增独立维护二进制，分批扫描并 canonical 化两张日志表。
- `web/src/**`
  - 第一方 Admin / Key / Token 日志面板统一消费 canonical request kind catalog，不再展示历史 per-path / per-tool 爆炸项。
- `src/tests/**`
  - 补齐 canonical 分类、legacy alias 过滤、历史回填、无损快照、前端 catalog 回归测试。

### Out of scope

- 第三方客户端的独立适配说明。
- 兼容窗口结束后的 legacy 列清理。
- 任何与 request kind 无关的运营指标重算。

## Canonical 分类合同

### API canonical keys

- `api:search`
- `api:extract`
- `api:crawl`
- `api:map`
- `api:research`
- `api:research-result`
- `api:usage`
- `api:unknown-path`

### MCP canonical keys

- `mcp:search`
- `mcp:extract`
- `mcp:crawl`
- `mcp:map`
- `mcp:research`
- `mcp:batch`
- `mcp:initialize`
- `mcp:ping`
- `mcp:tools/list`
- `mcp:resources/*`
- `mcp:prompts/*`
- `mcp:notifications/*`
- `mcp:unsupported-path`
- `mcp:unknown-payload`
- `mcp:unknown-method`
- `mcp:third-party-tool`

### Canonical detail 规则

- `api:unknown-path`：`request_kind_detail` 保存原 path。
- `mcp:unsupported-path`：`request_kind_detail` 保存原 path。
- `mcp:unknown-payload`：`request_kind_detail` 保存原 path 或更具体的 payload hint。
- `mcp:unknown-method`：`request_kind_detail` 保存原 MCP method。
- `mcp:third-party-tool`：`request_kind_detail` 保存第三方 tool name。
- 其余稳定 kind 仅在 `mcp:batch` 等确有必要时保留 detail。

### 禁止项

- `mcp:raw:/mcp/*`
- `api:raw:*`
- `mcp:tool:*`
- 任何把 path、tool name、自由文本直接编码进主 `request_kind_key` 的写入

## 数据库 / API 合同

- `request_logs`、`auth_token_logs` 新增：
  - `legacy_request_kind_key TEXT`
  - `legacy_request_kind_label TEXT`
  - `legacy_request_kind_detail TEXT`
- 主 `request_kind_key/label/detail` 输出 canonical 真相。
- 若历史主字段与 canonical 结果不一致：
  - 优先把旧主字段快照写入 `legacy_request_kind_*`
  - 再将主字段改写为 canonical 值
- 日志 API item 新增：
  - `legacyRequestKindKey`
  - `legacyRequestKindLabel`
  - `legacyRequestKindDetail`
- `requestKindOptions` 只返回 canonical 聚合项。
- `request_kind` 过滤参数接受 legacy alias，但后端必须先 canonical 化再查询。

## 回填策略

- 使用独立维护二进制 `request_kind_canonical_backfill`，不在启动流程里做全量重写。
- 两张表分别按 `id` 升序批处理，并用 meta 高水位游标支持断点续跑。
- 每批处理规则：
  - 计算该行 canonical request kind。
  - 若主字段与 canonical 三元组不同且 legacy 快照为空，则先写入 `legacy_request_kind_*`。
  - 将主字段回写成 canonical 三元组。
  - 不改 `method/path/query/request_body/response_body/failure_kind/business_credits/key_effect_*`。
- binary 必须幂等：重复运行不覆盖已存在的 legacy 快照，也不重复改写已 canonical 化的行。

## 验收标准（Acceptance Criteria）

- Given 新请求进入系统
  When 产生 request kind
  Then 主 `request_kind_key` 只能是白名单 canonical key，不再出现 `mcp:raw:/mcp/search`、`api:raw:*`、`mcp:tool:*`。
- Given 历史样本 `mcp:raw:/mcp/search`
  When 被 canonical backfill 处理
  Then 主字段改成 `mcp:unsupported-path`，detail 保留 `/mcp/search`，旧值可通过 `legacyRequestKind*` 取回。
- Given 历史样本 `api:raw:/api/custom/raw`
  When 被 canonical backfill 处理
  Then 主字段改成 `api:unknown-path`，detail 保留 `/api/custom/raw`，旧值可通过 `legacyRequestKind*` 取回。
- Given 历史样本 `mcp:tool:acme-lookup`
  When 被 canonical backfill 处理
  Then 主字段改成 `mcp:third-party-tool`，detail 保留 `acme-lookup`，旧值可通过 `legacyRequestKind*` 取回。
- Given Admin / Key / Token 日志筛选
  When 打开请求类型下拉
  Then 不再看到 `/mcp/search`、`/mcp/extract`、`mcp:tool:*` 这类离散爆炸项，只显示 canonical 聚合项。
- Given 传入 legacy alias `request_kind=mcp:raw:/mcp/search`
  When 查询日志分页
  Then 结果应命中 canonical `mcp:unsupported-path` 对应的同一结果集。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- 必要的前端测试 / 构建通过
- review-loop clear
- latest PR 进入 merge-ready

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 canonical catalog、legacy 快照字段与 spec 合同
- [x] M2: 完成后端分类、查询、DTO/view 与兼容 alias 收口
- [x] M3: 完成前端 catalog / badge / 筛选回归
- [x] M4: 新增 backfill binary 并补齐幂等/断点续跑测试
- [ ] M5: 本地验证、spec-sync、review-loop 与 merge-ready PR 收口

## 实现对齐说明

- 主 `request_kind_*` 已切换为 canonical 真相源；两张日志表均新增 `legacy_request_kind_*`，并在读写路径中保留旧值快照。
- 后端分类器与 SQL 聚合规则已统一到同一白名单 catalog；`mcp:raw:*`、`api:raw:*`、`mcp:tool:*` 不再作为主分类继续生成或聚合。
- 日志 DTO / API 已新增 `legacyRequestKind*` 审计字段；`requestKindOptions` 与 `request_kind` 过滤都按 canonical key 工作，同时兼容 legacy alias。
- 第一方前端 catalog、badge 与筛选逻辑已切换到 canonical 口径，不再把历史 per-path / per-tool 脏值展示为独立请求类型。
- 独立二进制 `request_kind_canonical_backfill` 已实现批量、幂等、可断点续跑的历史 canonical 化流程，并保留 legacy 快照。
- `request_logs` 的 legacy 快照列现在会在启动迁移尾部再次自愈补齐；`request_kind_canonical_backfill` 也会在执行前自检两张日志表的 legacy 列，避免历史库因缺列卡死回填。

## 变更记录

- 2026-03-24: 落地 canonical request kind catalog、legacy 快照列、兼容过滤、独立 backfill binary，并补齐后端/前端回归测试与本地验证。
- 2026-03-24: 补齐 `request_logs` legacy 快照列的启动迁移自愈与 backfill 缺列自检，覆盖共享测试机复制的生产历史库形态。

## 风险 / 假设

- 风险：历史日志量较大，backfill 必须通过批处理与 meta 游标保证可中断、可恢复。
- 风险：若有仓库外消费者依赖旧 `request_kind` 动态值，canonical 输出切换后会出现兼容差异，因此查询参数需要保留 legacy alias 映射。
- 假设：原始请求事实字段已经足够支撑审计，不需要额外保存 path/tool 的第二份事实列。
- 假设：兼容窗口内允许第一方 UI 完全切到 canonical 主字段，而不是继续将脏值作为主筛选项。

## 参考（References）

- `src/analysis.rs`
- `src/store/mod.rs`
- `src/server/dto.rs`
- `src/server/proxy.rs`
- `web/src/tokenLogRequestKinds.ts`
