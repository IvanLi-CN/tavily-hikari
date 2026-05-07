import type { RequestLog } from '../api'
import type { AdminTranslations } from '../i18n'
import type { StatusTone } from './StatusBadge'

type Language = 'en' | 'zh'

function isApiRebalanceEffectCode(code: string | null | undefined): boolean {
  return (code ?? '').trim().startsWith('api_rebalance_')
}

export function isApiRebalanceLog(log: RequestLog): boolean {
  return isApiRebalanceEffectCode(log.binding_effect_code) || isApiRebalanceEffectCode(log.selection_effect_code)
}

export function isRebalanceGatewayLog(log: RequestLog): boolean {
  return (log.gateway_mode ?? '').trim() === 'rebalance_http' || isApiRebalanceLog(log)
}

export function rebalanceMarkerLabel(log: RequestLog, strings: AdminTranslations): string {
  const gatewayMode = log.gateway_mode?.trim()
  if (gatewayMode) return `${strings.logDetails.gatewayMode}: ${gatewayMode}`
  return isApiRebalanceLog(log) ? strings.logDetails.apiRebalanceMode : ''
}

export function RebalanceGatewayMarker(): JSX.Element {
  return (
    <span className="log-key-pill__marker" aria-hidden="true">
      <svg viewBox="0 0 16 16" focusable="false">
        <path
          d="M5 2.75a1.75 1.75 0 1 1-1 3.18V11a1 1 0 0 0 1 1h4.18a1.75 1.75 0 1 1 0 1.5H5A2.5 2.5 0 0 1 2.5 11V5.93A1.75 1.75 0 0 1 5 2.75m6 0a1.75 1.75 0 1 1-1 3.18V8a1 1 0 0 1-1 1H6.5v-1.5H8.5V5.93A1.75 1.75 0 0 1 11 2.75"
          fill="currentColor"
        />
      </svg>
    </span>
  )
}

export function apiRebalanceBindingEffectTone(code: string | null | undefined): StatusTone | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_route_bound':
      return 'success'
    case 'api_rebalance_route_reused':
      return 'neutral'
    case 'api_rebalance_route_rebound':
      return 'warning'
    default:
      return null
  }
}

export function apiRebalanceBindingEffectLabel(code: string | null | undefined, strings: AdminTranslations): string | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_route_bound':
      return strings.logs.bindingEffects.apiRebalanceBound
    case 'api_rebalance_route_reused':
      return strings.logs.bindingEffects.apiRebalanceReused
    case 'api_rebalance_route_rebound':
      return strings.logs.bindingEffects.apiRebalanceRebound
    default:
      return null
  }
}

export function apiRebalanceBindingEffectBadgeLabel(code: string | null | undefined, language: Language): string | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_route_bound':
      return language === 'zh' ? 'API绑定' : 'API bound'
    case 'api_rebalance_route_reused':
      return language === 'zh' ? 'API复用' : 'API reused'
    case 'api_rebalance_route_rebound':
      return language === 'zh' ? 'API重绑' : 'API rebound'
    default:
      return null
  }
}

export function apiRebalanceBindingEffectSummary(code: string | null | undefined, language: Language): string | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_route_bound':
      return language === 'zh' ? 'API Rebalance 为该路由创建了新的上游 Key 绑定' : 'API rebalance created a new upstream key binding for this route'
    case 'api_rebalance_route_reused':
      return language === 'zh' ? 'API Rebalance 继续复用该路由当前的上游 Key 绑定' : 'API rebalance reused the current upstream key binding for this route'
    case 'api_rebalance_route_rebound':
      return language === 'zh' ? 'API Rebalance 将该路由重新绑定到新的上游 Key' : 'API rebalance rebound this route to a different upstream key'
    default:
      return null
  }
}

export function apiRebalanceSelectionEffectTone(code: string | null | undefined): StatusTone | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_cooldown_avoided':
      return 'warning'
    case 'api_rebalance_rate_limit_avoided':
      return 'error'
    case 'api_rebalance_pressure_avoided':
      return 'success'
    default:
      return null
  }
}

export function apiRebalanceSelectionEffectLabel(code: string | null | undefined, strings: AdminTranslations): string | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_cooldown_avoided':
      return strings.logs.selectionEffects.apiRebalanceCooldownAvoided
    case 'api_rebalance_rate_limit_avoided':
      return strings.logs.selectionEffects.apiRebalanceRateLimitAvoided
    case 'api_rebalance_pressure_avoided':
      return strings.logs.selectionEffects.apiRebalancePressureAvoided
    default:
      return null
  }
}

export function apiRebalanceSelectionEffectBadgeLabel(code: string | null | undefined, language: Language): string | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_cooldown_avoided':
      return language === 'zh' ? 'API避冷却' : 'API cooldown'
    case 'api_rebalance_rate_limit_avoided':
      return language === 'zh' ? 'API避429' : 'API 429'
    case 'api_rebalance_pressure_avoided':
      return language === 'zh' ? 'API避高压' : 'API pressure'
    default:
      return null
  }
}

export function apiRebalanceSelectionEffectSummary(code: string | null | undefined, language: Language): string | null {
  switch ((code ?? '').trim()) {
    case 'api_rebalance_cooldown_avoided':
      return language === 'zh' ? 'API Rebalance 避开了仍处于冷却中的 Key' : 'API rebalance avoided a key that is still in cooldown'
    case 'api_rebalance_rate_limit_avoided':
      return language === 'zh' ? 'API Rebalance 避开了最近更容易触发限流的 Key' : 'API rebalance avoided a key that was recently more rate-limited'
    case 'api_rebalance_pressure_avoided':
      return language === 'zh' ? 'API Rebalance 避开了近期压力更高的 Key' : 'API rebalance avoided a key under higher recent pressure'
    default:
      return null
  }
}
