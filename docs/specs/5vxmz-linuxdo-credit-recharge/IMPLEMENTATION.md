# Linux.do Credit 额度充值实现状态（#5vxmz）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现（本地验证通过）
- Lifecycle: active
- Catalog note: Linux.do Credit monthly quota recharge

## Coverage / rollout summary

- 当前主题已落地用户侧充值卡片、后端订单创建/通知闭环、账户小时/日/月额度权益叠加与管理端只读审计。
- 管理端系统设置提供充值总开关与非管理员开放调试开关；后端配置接口区分 `visible` 与 `enabled`，创建订单按系统设置 gate 拒绝不可用请求。
- 管理端新增充值记录模块；存在充值订单时展示导航，支持平铺/按用户聚合、用户/状态/时间筛选、订单时间/成交时间/退款时间/状态排序。
- 管理端退单和仅退款均通过 Linux.do Credit 全额退款接口执行，并受全局管理端 TOTP 保护；退单撤销订单权益，仅退款保留权益。外部退款成功后先持久化成功标记，若最终本地落账失败，后续同一路径重试只补本地落账，不重复调用平台退款。
- 全局管理端 TOTP secret 使用 `LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY` 加密存储，重置/解绑需要当前 TOTP；`DEV_OPEN_ADMIN` 下禁止修改充值总开关和退款动作。
- 充值记录页点击用户名跳转到用户详情页，用户详情页以表格形式展示覆盖充值周期前后一月的额度月历。
- 默认价格为 `50 LDC = 1000 积分额度 / 自然月`，当前月充值权益按每 `1000`
  积分派生 `+20` 小时、`+100` 日、`+1000` 月额度；小额测试价正数保底为
  `+1/+1/+credits`。
- 商户私钥解析支持 32-byte Ed25519 seed、PKCS#8 PEM/DER，以及 Linux.do Credit
  线上配置中出现的 48-byte 最小 Ed25519 PKCS#8 v1 DER。
- Linux.do Credit 创建订单响应按浏览器跳转模型处理：后端禁用 HTTP 自动重定向，保存并返回上游 3xx `Location` 作为支付 URL，避免服务端跟随到需要用户态认证的支付页并误判为 `403`。

## Remaining Gaps

- 退款链路仍只支持 Linux.do Credit 官方全额退款；部分退款和恢复码留待后续专题。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
