# Observability SQLite 存储边界

## Context and Scope

独立 observability sidecar 承担派生投影、请求诊断和维护观测；其 reader/writer 行为不能仅凭 core 数据库的 journal mode、checkpoint 返回值或 WAL 文件大小判断。本主题明确 writable 初始化、readonly/legacy 入口、两个 schema 的 checkpoint、快照与兼容恢复边界。

本主题独立于 [performance architecture](../performance-architecture-hardening/SPEC.md) 的“不修改 WAL PRAGMA”范围，新增 sidecar 存储合同。它不改变账务真值、synchronous 持久性等级、SQLite 单 writer 规则、连接池容量或 HA 数据复制协议，也不定义新的跨文件原子快照。

## Terms and Interfaces

- core 数据库保存既有业务和控制事实；observability 数据库保存既有派生数据，含义由 [CONTEXT.md](../../../CONTEXT.md) 与原有迁移合同决定。
- distinct sidecar 为与 core 不同的真实文件；legacy same-file 为既有兼容布局，不能为其自动新建第二个数据库。
- 入口包括 runtime pool、forced pool、新连接、独立 SQLite CLI helper 与离线 migration；readonly/dry-run 必须拥有显式的读取条件与错误边界。
- checkpoint 诊断采用固定 schema 类别 main/observability，禁止用任意路径作 metrics label。

## Requirements

### REQ-OBS-STORAGE-001：受控可写初始化

distinct writable sidecar MUST 在现有受控启动/迁移锁边界配置为 WAL 并核验实际返回模式，覆盖 runtime/forced pool、新连接、CLI helper 和离线 migration。legacy same-file 不建立另一文件、不重复设置同一数据库。

模式设置失败 MUST 在有界初始化预算内给出脱敏、明确的错误，不得将实际 DELETE mode 报成已满足 WAL 合同。必须保留现有 synchronous 等级、事务语义和单 writer 争用；WAL 不是增加 writer 并发的手段。

### REQ-OBS-STORAGE-002：只读与 dry-run

readonly/dry-run MUST 不切换 journal、不执行 checkpoint、不创建缺失 sidecar。缺少可读取条件时必须返回明确错误；不得为了绕过实时 WAL 条件而擅自使用 immutable 忽略已提交内容。只读视图必须遵守既有完整性和 migration verification 规则。

### REQ-OBS-STORAGE-003：按 schema 核验 checkpoint 与增长

checkpoint MUST 明确执行并解释 main 与 observability 的各自结果，分别记录 busy、剩余 frame 和文件增长。未指定 schema 的聚合调用或只读取 core WAL 字节不能证明 sidecar 已排空；长期 reader 下 busy/未排空结果必须可识别。

正常混合读写与持续恢复结束后，系统 MUST 有可验证的 checkpoint 收敛与 WAL 增长边界。长期 reader 可以使 WAL 暂时增长，但不得被正常 core 结果掩盖；诊断不承诺强制打断 reader 或无条件截断 WAL。

### REQ-OBS-STORAGE-004：快照与异常恢复

core 与 observability MUST 各自通过 SQLite backup API 导出，验证尚未 checkpoint 的 committed frame 已包含在对应快照；不得直接拷贝主文件而遗漏 WAL。两个独立导出不承诺同一跨文件原子时刻，既有业务账务真值和派生重建关系必须保留。

异常退出、重开、restore 和 HA 节点独立启动/接管 MUST 正确读取 committed 数据。schema/index/derived sidecar 的初始化继续节点本地治理，不新增业务 outbox 或文件复制协议。

### REQ-OBS-STORAGE-005：迁移和兼容回退证据

验收 MUST 分别报告 journal 切换的启动耗时、reader/writer 争用、两个文件的 WAL 增长、checkpoint 结果和前台延迟。控制环境使用应用实际 SQLite 与合成数据；只证明读快照不阻塞某次提交不能替代完整存储验收。

ledger 或存储合同升级后，回退 MUST 使用认识已应用 ledger 的兼容构建或受控向前修复。模式反向切换需经过显式存储变更边界；不得承诺索引或 PRAGMA 可逆就意味着原旧镜像可安全启动，也不得用旧快照覆盖其后新增的业务账务。

## Verification

### VER-OBS-STORAGE-001

- covers: `REQ-OBS-STORAGE-001`, `REQ-OBS-STORAGE-002`
- 方法：新库、已有 DELETE、已有 WAL、legacy same-file、缺失 sidecar、readonly/dry-run、多 pool、新连接、CLI、离线 migration、重开和模式切换失败。
- 通过条件：可写 distinct 文件实际为 WAL，错误有界且明确，legacy 不新建文件；只读入口不改模式、不 checkpoint、不创建文件，读取结果不遗漏已提交 WAL 内容。

### VER-OBS-STORAGE-002

- covers: `REQ-OBS-STORAGE-001`, `REQ-OBS-STORAGE-003`, `REQ-OBS-STORAGE-005`
- 方法：保持 sidecar read snapshot，执行前台 writer；另测两个 writer 的争用、main/observability 独立 checkpoint、长期 reader 以及持续混合恢复后的收敛。
- 通过条件：reader 保持期间 writer 可以提交；两个 writer 仍正确争用且有界。每个 schema 的 busy/剩余 frame/文件增长分别可见，释放 reader 并完成正常 checkpoint 后 WAL 收敛，无把 core 结果误当 sidecar 结果的诊断。

### VER-OBS-STORAGE-003

- covers: `REQ-OBS-STORAGE-004`, `REQ-OBS-STORAGE-005`
- 方法：带未 checkpoint committed frame 的 backup/restore、异常退出后重开、已有 ledger 兼容构建和两个 HA 节点独立初始化/接管。
- 通过条件：备份和恢复包含各文件 committed 数据，业务账务与派生重建关系正确，无新增跨文件原子性或复制承诺；回退路径认识已应用 ledger，未覆盖新业务事实。

## Related ADRs

- [ADR 0010: Observability SQLite WAL](../../adr/0010-observability-sqlite-wal.md)

## References

- [Implementation coverage](./IMPLEMENTATION.md)
- [History and compatibility](./HISTORY.md)
- [Local work cost boundaries](../local-performance-boundaries/SPEC.md)
- [SQLite hardening and migration](../sqlite-write-lock-hardening/SPEC.md)
