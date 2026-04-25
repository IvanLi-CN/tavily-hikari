import type { LogOperationalClass } from '../api'
import type { StatusTone } from '../components/StatusBadge'

export type SupportedLanguage = 'en' | 'zh'

export function normalizeOperationalClass(
  value: string | null | undefined,
): LogOperationalClass | null {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'success':
      return 'success'
    case 'neutral':
      return 'neutral'
    case 'client_error':
      return 'client_error'
    case 'upstream_error':
      return 'upstream_error'
    case 'system_error':
      return 'system_error'
    case 'quota_exhausted':
      return 'quota_exhausted'
    default:
      return null
  }
}

export function operationalClassTone(value: string | null | undefined): StatusTone {
  switch (normalizeOperationalClass(value)) {
    case 'success':
      return 'success'
    case 'neutral':
      return 'neutral'
    case 'client_error':
      return 'warning'
    case 'upstream_error':
      return 'error'
    case 'system_error':
      return 'error'
    case 'quota_exhausted':
      return 'warning'
    default:
      return 'neutral'
  }
}

export function operationalClassLabel(
  value: string | null | undefined,
  language: SupportedLanguage,
): string {
  switch (normalizeOperationalClass(value)) {
    case 'success':
      return language === 'zh' ? '成功' : 'Success'
    case 'neutral':
      return language === 'zh' ? '中性' : 'Neutral'
    case 'client_error':
      return language === 'zh' ? '客户端错误' : 'Client Error'
    case 'upstream_error':
      return language === 'zh' ? '上游错误' : 'Upstream Error'
    case 'system_error':
      return language === 'zh' ? '系统错误' : 'System Error'
    case 'quota_exhausted':
      return language === 'zh' ? '限额' : 'Quota Exhausted'
    default:
      return value?.trim() || (language === 'zh' ? '未知' : 'Unknown')
  }
}

export function failureKindGuidance(
  kind: string | null | undefined,
  language: SupportedLanguage,
): string | null {
  switch ((kind ?? '').trim()) {
    case 'upstream_gateway_5xx':
      return language === 'zh'
        ? '这是上游网关临时故障，建议稍后重试，并检查上游连通性或代理节点健康。'
        : 'This is a temporary upstream gateway failure. Retry later and inspect upstream connectivity or proxy health.'
    case 'upstream_rate_limited_429':
      return language === 'zh'
        ? '这是 Tavily 限流，建议降低频率、切换其他 Key，或等待限流窗口恢复。'
        : 'Tavily is rate limiting this traffic. Reduce request rate, switch keys, or wait for the limit window to reset.'
    case 'upstream_account_deactivated_401':
      return language === 'zh'
        ? '该 Key 可能已失效、被撤销或账户停用，建议更换可用 Key 并检查 Tavily 后台状态。'
        : 'The key may be invalid, revoked, or tied to a deactivated account. Replace it and check the Tavily account state.'
    case 'transport_send_error':
      return language === 'zh'
        ? '这是链路发送失败，建议检查 DNS、TLS、代理链路和上游可达性。'
        : 'This request failed before getting an upstream response. Check DNS, TLS, proxy routing, and upstream reachability.'
    case 'mcp_accept_406':
      return language === 'zh'
        ? '客户端需要同时接受 application/json 与 text/event-stream，请修正 Accept 请求头。'
        : 'The client must accept both application/json and text/event-stream. Fix the Accept header negotiation.'
    case 'tool_argument_validation':
      return language === 'zh'
        ? '这是客户端参数或 schema 不匹配，请检查工具参数名、必填字段和输入类型。'
        : 'The request payload does not match the current tool schema. Check argument names, required fields, and input types.'
    case 'unknown_tool_name':
      return language === 'zh'
        ? '客户端调用了不存在的工具名，请先重新获取 tools/list 再重试。'
        : 'The client called a tool name that is not advertised. Refresh tools/list before retrying.'
    case 'invalid_search_depth':
      return language === 'zh'
        ? 'search_depth 参数不合法，请改用当前支持的枚举值。'
        : 'The search_depth value is invalid. Use one of the currently supported enum values.'
    case 'invalid_country_search_depth_combo':
      return language === 'zh'
        ? 'country 与当前 search_depth 组合不受支持，请调整参数搭配。'
        : 'The selected country/search_depth combination is not supported. Adjust the parameter pair.'
    case 'research_payload_422':
      return language === 'zh'
        ? 'research 请求体不符合上游要求，请检查字段结构与必填项。'
        : 'The research payload does not match the upstream contract. Validate the body structure and required fields.'
    case 'query_too_long':
      return language === 'zh'
        ? 'query 超出上游长度限制，请缩短输入后重试。'
        : 'The query exceeds the upstream length limit. Shorten the input before retrying.'
    case 'mcp_method_405':
      return language === 'zh'
        ? '这是 MCP transport 层返回的 405，请结合请求类型与上游响应判断是否属于控制面行为。'
        : 'This is an MCP transport-level 405. Inspect the request kind and upstream response before treating it as a caller error.'
    case 'mcp_path_404':
      return language === 'zh'
        ? '客户端访问了不存在的 MCP 路径，请检查 endpoint 配置。'
        : 'The client hit a non-existent MCP path. Check the configured endpoint path.'
    default:
      return null
  }
}

export function operationalClassGuidance(
  operationalClass: string | null | undefined,
  failureKind: string | null | undefined,
  language: SupportedLanguage,
): string | null {
  const failureGuidance = failureKindGuidance(failureKind, language)
  if (failureGuidance) {
    return failureGuidance
  }

  switch (normalizeOperationalClass(operationalClass)) {
    case 'neutral':
      return language === 'zh'
        ? '这是 MCP 控制面或非计费请求，默认保留审计可见性，但不代表线上事故。'
        : 'This is MCP control-plane or other non-billable traffic. It remains visible for auditability, but it is not an incident by itself.'
    case 'client_error':
      return language === 'zh'
        ? '这是客户端输入、协议或工具参数问题，建议先检查调用方请求。'
        : 'This is a client-side request, protocol, or tool-argument issue. Inspect the caller payload first.'
    case 'upstream_error':
      return language === 'zh'
        ? '这是上游返回的外部错误，建议结合 Tavily 状态、限流与连通性继续排查。'
        : 'This is an upstream-originated failure. Check Tavily status, rate limits, and upstream connectivity.'
    case 'system_error':
      return language === 'zh'
        ? '这是代理链路或系统侧异常，建议查看服务日志、网络链路与内部报错。'
        : 'This is a proxy or system-side failure. Inspect service logs, network connectivity, and internal errors.'
    case 'quota_exhausted':
      return language === 'zh'
        ? '这是额度耗尽，不属于系统故障。请检查当前窗口的业务额度配置。'
        : 'This is quota exhaustion rather than a system failure. Check the configured business quota window.'
    default:
      return null
  }
}
