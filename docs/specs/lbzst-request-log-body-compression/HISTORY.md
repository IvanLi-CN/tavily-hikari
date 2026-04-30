# Request Logs Body 压缩历史

## 关键决策

- 使用 zstd 作为唯一压缩 codec；`NULL` codec 保持 legacy/plain 兼容。
- 压缩粒度是单个 body 字段，而不是整行或整条日志。
- 热路径统计不再依赖压缩正文，新增派生列保存压缩前即可确定的分类结果。
- 历史压缩不自动 `VACUUM`，避免在线维护窗口外造成大锁与 I/O 峰值。
- 自动迁移失败不阻断代理请求；失败信息通过 scheduled job message 暴露。
