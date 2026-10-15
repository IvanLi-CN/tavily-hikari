# Observability SQLite 存储边界：实现覆盖

## Current Status

- Lifecycle: active
- Implementation: 要求已确认，新增存储行为尚未实现。
- 诊断基线：backend 0.86.0 的 core 为 WAL，独立 observability sidecar 为 DELETE；合成 fixture 已复现 reader 阻塞 writer commit 的机制，线上延迟占比尚未测得。

## Implementation Coverage

| 要求                | 主要实现归属                                                                                           | 已确认缺口                                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| REQ-OBS-STORAGE-001 | `src/store/mod.rs`、`src/store/sqlite_runtime.rs`、`src/bin/support/sqlite_sidecar.rs`、migration 入口 | 仅 core options 显式启用 WAL，sidecar 通过 ATTACH 接入                                                      |
| REQ-OBS-STORAGE-002 | readonly/verification、独立 CLI 和离线 migration                                                       | 需新增 mode/readability 回归，同时保持只读入口无写行为                                                      |
| REQ-OBS-STORAGE-003 | `src/store/key_store_jobs.rs` checkpoint 与 WAL 诊断                                                   | 当前无 schema checkpoint 在应用 SQLite 3.44 作用于 attached DB，但返回值仅代表第一个；字节统计只看 core WAL |
| REQ-OBS-STORAGE-004 | `scripts/export-live-db-snapshot-to-testbox.sh`、reopen/restore 与 HA tests                            | 已采用分别 SQLite backup API；需验证未 checkpoint WAL 与崩溃恢复                                            |
| REQ-OBS-STORAGE-005 | 存储测试、兼容启动与性能 fixtures                                                                      | 正式 native 后端、startup、稳态、异常和双节点证据尚未执行                                                   |

## Verification and Rollout

- 合成读快照 fixture：DELETE writer 约 121ms 后返回 locked，WAL writer 约 0.07ms 提交；这是 SQLite 3.45.3 的机制复现，不是完整线上请求倍数。
- 正式测试使用应用绑定 SQLite，按 [testing.md](../../agents/testing.md) 选窄测试；文件并发、备份/恢复、HA 和性能负载在会话 Agent VM 运行，不在 101 施压。
- 存储初始化、checkpoint、readonly、CLI、backup 和兼容回退需一起覆盖；不能仅设置一个 PRAGMA 后以 core checkpoint 的成功验收。
- 生产模式切换和发布仍需明确部署授权；当前没有修改服务器配置、数据库或容器。

## Remaining Gaps

所有新增 REQ/VER 的实现与验收尚未完成。重点缺口为每个入口的 verified mode、多文件 checkpoint 诊断、带 WAL 的 backup/restore、异常重开及受控兼容回退。

## References

- [Requirements](./SPEC.md)
- [History](./HISTORY.md)
