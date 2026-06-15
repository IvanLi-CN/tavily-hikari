# History：Release `amd64` smoke 启动加固与 `v0.36.7` 回填（#stxfn）

## 关键演进

- 2026-04-07：创建 spec，冻结“只修 release harness、不改 PR 门禁、回填 `v0.36.7`”的执行边界。
- 2026-04-07：完成 smoke 脚本抽离、动态端口与失败诊断加固，并确认旧提交 backfill 需要显式恢复缺失 helper。
- 2026-06-15：observability sidecar 上线后，release smoke 的 SQLite 断言因仍直连 core DB 而失效；本轮将断言抽成 sidecar-aware Python helper，并把 backfill 恢复列表扩展到新 helper，关闭这条 release-only 回归链路。

## 相关规范

- `docs/specs/2wdrp-sqlite-write-lock-hardening/SPEC.md`
- `docs/specs/2wdrp-sqlite-write-lock-hardening/IMPLEMENTATION.md`
