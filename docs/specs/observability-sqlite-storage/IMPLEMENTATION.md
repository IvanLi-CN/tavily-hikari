# Observability SQLite 存储边界：实现覆盖

## Current Status

- Lifecycle: active
- Implementation: sidecar WAL、双 schema checkpoint、只读入口与分别备份路径已在父分支落地；当前头的异常恢复、HA 与正式性能证据仍待收口。
- 当前基线：父分支 `prd/101-performance-hardening`，commit `699a065cfe27deabb30d401b8f9de57f9dd0bcff`，基线 `b1bb1043823b9260b18389854e6b90b2cdedbd70`。本地、Agent VM 与 101 不执行性能或对比负载。

## Implementation Coverage

| 要求                | 主要实现归属                                                                                           | 已确认缺口                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| REQ-OBS-STORAGE-001 | `src/store/mod.rs`、`src/store/sqlite_runtime.rs`、`src/bin/support/sqlite_sidecar.rs`、migration 入口 | writable sidecar 初始化、WAL 核验与单 writer 边界已落地；需完成跨入口回归                    |
| REQ-OBS-STORAGE-002 | readonly/verification、独立 CLI 和离线 migration                                                       | readonly/dry-run 不切模式、不 checkpoint、不创建文件的路径已落地；需补齐全部入口证据         |
| REQ-OBS-STORAGE-003 | `src/store/key_store_jobs.rs` checkpoint 与 WAL 诊断                                                   | main/observability 双 schema checkpoint 与剩余 frame/WAL 诊断已落地；需完成 attached DB 回归 |
| REQ-OBS-STORAGE-004 | `scripts/export-live-db-snapshot-to-testbox.sh`、reopen/restore 与 HA tests                            | 分别 SQLite backup/restore 与异常重开路径已落地；需完成未 checkpoint WAL 和 HA 验收          |
| REQ-OBS-STORAGE-005 | 存储测试、兼容启动与性能 fixtures                                                                      | 当前头的 native 后端、startup、异常恢复、双节点与 GitHub-hosted A8 证据仍待收口              |

## Verification and Rollout

- 合成读快照 fixture：DELETE writer 约 121ms 后返回 locked，WAL writer 约 0.07ms 提交；这是历史机制复现，不是完整线上请求倍数，也不构成当前头 A8 证据。
- 正式测试使用应用绑定 SQLite，按 [testing.md](../../agents/testing.md) 选窄测试；性能/对比只在 GitHub-hosted mock runner，文件并发、备份/恢复和 HA 证据按授权环境收集，不在 101 施压。
- 存储初始化、checkpoint、readonly、CLI、backup 和兼容回退需一起覆盖；不能仅设置一个 PRAGMA 后以 core checkpoint 的成功验收。
- 生产模式切换和发布仍需明确部署授权；当前没有修改服务器配置、数据库或容器。

## Remaining Gaps

重点剩余缺口为每个入口的 verified mode 证据、带 WAL 的 backup/restore、异常重开、双节点接管及受控兼容回退；当前文档不表示已部署生产。

## References

- [Requirements](./SPEC.md)
- [History](./HISTORY.md)
