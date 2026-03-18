# HTTP APIs

## `GET /api/settings`

`forwardProxy` payload 新增：

```json
{
  "egressSocks5Enabled": false,
  "egressSocks5Url": ""
}
```

## `PUT /api/settings/forward-proxy`

请求体新增：

```json
{
  "proxyUrls": [],
  "subscriptionUrls": [],
  "subscriptionUpdateIntervalSecs": 3600,
  "insertDirect": true,
  "egressSocks5Enabled": false,
  "egressSocks5Url": "",
  "skipBootstrapProbe": false
}
```

响应体保持 `ForwardProxySettingsResponse`，新增：

```json
{
  "egressSocks5Enabled": false,
  "egressSocks5Url": ""
}
```

## SSE Progress Phases

- `validate_egress_socks5`
- `save_settings`
- `apply_egress_socks5`
- `refresh_subscription`
- `bootstrap_probe`
- `refresh_ui`

启用成功必须至少发出一次 `validate_egress_socks5` 与一次 `apply_egress_socks5`。
关闭流程不发 `validate_egress_socks5`，但仍必须发 `apply_egress_socks5`。
