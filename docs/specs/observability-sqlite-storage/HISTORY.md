# Observability SQLite 存储边界：主题背景

## Lifecycle / Compatibility

101 性能诊断确认 distinct sidecar 的 DELETE journal 与 core WAL 不一致，并复现 sidecar reader 对 writer commit 的阻塞。主人接受将 WAL 单独交付；本主题保存其初始化、只读、checkpoint、快照与兼容合同。

现有 performance-architecture-hardening 的 PRAGMA non-goal 保留，本主题独立拥有新增存储边界。选择原因见规范的 Related ADRs；仅 journal 变化不能推断线上全部延迟已经消除。

legacy same-file 和独立 sidecar 都需保持可识别的初始化路径；已经应用的新 migration 可能使原旧二进制拒绝启动，兼容构建与向前修复是回退合同的一部分。

## Related Changes

当前未关联实现 PR 或交付 commit。

## References

- [Requirements](./SPEC.md)
- [Implementation coverage](./IMPLEMENTATION.md)
