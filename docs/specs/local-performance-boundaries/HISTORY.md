# 本地请求与维护成本边界：主题背景

## Lifecycle / Compatibility

本主题因 101 性能诊断而建立，保存新确认的单次工作成本、配对计量和 blocked-range eligibility 合同；它补充性能架构所有权、SQLite hardening、monitoring 和 period reconciliation，不取代这些主题。

主人已接受先交付确认的性能缺陷，保留现有计费串行合同。credit reservation 后置，需要独立收敛取消、重复结算、不确定上游费用、崩溃恢复与 HA 接管，不能通过直接移除 billing mutex 替代。

历史来源缺口的成因尚未证实；本主题只治理已确认的重复扫描与无进展重试，不声称自动修复缺失来源。原生读预算、索引定位与有效恢复进展需要同时成立。

## Related Changes

当前未关联实现 PR 或交付 commit。

## References

- [Requirements](./SPEC.md)
- [Implementation coverage](./IMPLEMENTATION.md)
