# 用户控制台单页合并演进历史（#2nx74）

## Decision Trace

- 2026-03-12: `/console` 收敛为账户概览与 Token 列表的单页，保留旧 hash 的定位兼容。
- 2026-07-07: landing 降低嵌套卡片边界，保留充值区域作为同页可见内容。
- 2026-07-12: 恢复因独立 billing 页引入而移除的概览右侧完整充值卡；billing 页继续负责完整权益和自然月明细。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
