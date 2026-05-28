# Linux.do Credit 额度充值实现状态（#5vxmz）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现（本地验证通过）
- Lifecycle: active
- Catalog note: Linux.do Credit monthly quota recharge

## Coverage / rollout summary

- 当前主题已落地用户侧充值卡片、后端订单创建/通知闭环、账户小时/日/月额度权益叠加与管理端只读审计。
- 管理端系统设置提供充值总开关与非管理员开放调试开关；后端配置接口区分 `visible` 与 `enabled`，创建订单按系统设置 gate 拒绝不可用请求。
- 默认价格为 `50 LDC = 1000 积分额度 / 自然月`，当前月充值权益按每 `1000`
  积分派生 `+20` 小时、`+100` 日、`+1000` 月额度；小额测试价正数保底为
  `+1/+1/+credits`。
- 商户私钥解析支持 32-byte Ed25519 seed、PKCS#8 PEM/DER，以及 Linux.do Credit
  线上配置中出现的 48-byte 最小 Ed25519 PKCS#8 v1 DER。

## Remaining Gaps

- 待根据后续线上观察微调充值文案或布局。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
