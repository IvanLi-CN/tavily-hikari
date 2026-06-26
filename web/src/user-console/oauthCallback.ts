import type { EN } from './text'

export const USER_CONSOLE_LOGIN_START_PATH = '/auth/linuxdo'
export const OAUTH_CALLBACK_FINALIZE_TIMEOUT_MS = 12_000

export interface OAuthCallbackQueryState {
  code: string | null
  state: string | null
  error: string | null
  errorDescription: string | null
}

export type OAuthCallbackScreenState =
  | 'connecting'
  | 'success'
  | 'providerDenied'
  | 'invalidRequest'
  | 'invalidState'
  | 'inactiveUser'
  | 'timeout'
  | 'upstreamFailure'
  | 'serverError'
  | 'unsupportedProvider'

export type OAuthCallbackStepState = 'complete' | 'active' | 'pending' | 'error'
export type OAuthCallbackTone = 'info' | 'success' | 'warning' | 'danger'

export interface OAuthCallbackPanelStep {
  label: string
  state: OAuthCallbackStepState
}

export interface OAuthCallbackPanelModel {
  badge: string
  title: string
  description: string
  note: string | null
  liveMessage: string
  tone: OAuthCallbackTone
  icon: string
  busy: boolean
  showActions: boolean
  steps: OAuthCallbackPanelStep[]
  primaryActionLabel: string
  secondaryActionLabel: string
}

type OAuthCallbackText = typeof EN.oauthCallback

function formatTemplate(template: string, values: Record<string, string>): string {
  return Object.entries(values).reduce(
    (current, [key, value]) => current.replace(new RegExp(`\\{${key}\\}`, 'g'), value),
    template,
  )
}

export function parseOAuthCallbackQuery(search: string): OAuthCallbackQueryState {
  const params = new URLSearchParams(search)
  const readValue = (key: string): string | null => {
    const value = params.get(key)?.trim()
    return value ? value : null
  }
  return {
    code: readValue('code'),
    state: readValue('state'),
    error: readValue('error'),
    errorDescription: readValue('error_description'),
  }
}

export function resolveOAuthCallbackProviderLabel(
  provider: string,
  providers: { linuxdo: string },
): string {
  if (provider === 'linuxdo') return providers.linuxdo
  return provider
}

function stepSet(
  text: OAuthCallbackText,
  returned: OAuthCallbackStepState,
  verify: OAuthCallbackStepState,
  session: OAuthCallbackStepState,
): OAuthCallbackPanelStep[] {
  return [
    { label: text.steps.returned, state: returned },
    { label: text.steps.verify, state: verify },
    { label: text.steps.session, state: session },
  ]
}

export function resolveOAuthCallbackPanelModel(args: {
  state: OAuthCallbackScreenState
  providerLabel: string
  text: OAuthCallbackText
  detail?: string | null
}): OAuthCallbackPanelModel {
  const { providerLabel, text } = args
  const fill = (template: string): string => formatTemplate(template, { provider: providerLabel })
  const retryLabel = fill(text.actions.retry)
  const base = {
    badge: text.badge,
    primaryActionLabel: retryLabel,
    secondaryActionLabel: text.actions.home,
  }

  switch (args.state) {
    case 'connecting':
      return {
        ...base,
        title: fill(text.status.connecting.title),
        description: fill(text.status.connecting.description),
        note: fill(text.notes.connecting),
        liveMessage: fill(text.status.connecting.title),
        tone: 'info',
        icon: 'mdi:progress-clock',
        busy: true,
        showActions: false,
        steps: stepSet(text, 'complete', 'active', 'pending'),
      }
    case 'success':
      return {
        ...base,
        title: fill(text.status.success.title),
        description: fill(text.status.success.description),
        note: text.notes.redirecting,
        liveMessage: fill(text.status.success.title),
        tone: 'success',
        icon: 'mdi:check-circle-outline',
        busy: false,
        showActions: false,
        steps: stepSet(text, 'complete', 'complete', 'complete'),
      }
    case 'providerDenied':
      return {
        ...base,
        title: fill(text.status.providerDenied.title),
        description: fill(text.status.providerDenied.description),
        note: args.detail ? formatTemplate(text.notes.providerDetail, { detail: args.detail }) : null,
        liveMessage: fill(text.status.providerDenied.title),
        tone: 'warning',
        icon: 'mdi:close-circle-outline',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'error', 'pending', 'pending'),
      }
    case 'invalidRequest':
      return {
        ...base,
        title: text.status.invalidRequest.title,
        description: fill(text.status.invalidRequest.description),
        note: null,
        liveMessage: text.status.invalidRequest.title,
        tone: 'warning',
        icon: 'mdi:file-alert-outline',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'error', 'pending', 'pending'),
      }
    case 'invalidState':
      return {
        ...base,
        title: text.status.invalidState.title,
        description: fill(text.status.invalidState.description),
        note: null,
        liveMessage: text.status.invalidState.title,
        tone: 'warning',
        icon: 'mdi:shield-alert-outline',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'complete', 'error', 'pending'),
      }
    case 'inactiveUser':
      return {
        ...base,
        title: fill(text.status.inactiveUser.title),
        description: fill(text.status.inactiveUser.description),
        note: args.detail ? formatTemplate(text.notes.technicalDetail, { detail: args.detail }) : null,
        liveMessage: fill(text.status.inactiveUser.title),
        tone: 'danger',
        icon: 'mdi:account-off-outline',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'complete', 'complete', 'error'),
      }
    case 'timeout':
      return {
        ...base,
        title: text.status.timeout.title,
        description: fill(text.status.timeout.description),
        note: text.notes.timeout,
        liveMessage: text.status.timeout.title,
        tone: 'warning',
        icon: 'mdi:timer-sand',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'complete', 'complete', 'error'),
      }
    case 'upstreamFailure':
      return {
        ...base,
        title: fill(text.status.upstreamFailure.title),
        description: fill(text.status.upstreamFailure.description),
        note: args.detail ? formatTemplate(text.notes.technicalDetail, { detail: args.detail }) : null,
        liveMessage: fill(text.status.upstreamFailure.title),
        tone: 'danger',
        icon: 'mdi:server-network-off',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'complete', 'complete', 'error'),
      }
    case 'serverError':
      return {
        ...base,
        title: text.status.serverError.title,
        description: fill(text.status.serverError.description),
        note: args.detail ? formatTemplate(text.notes.technicalDetail, { detail: args.detail }) : null,
        liveMessage: text.status.serverError.title,
        tone: 'danger',
        icon: 'mdi:alert-circle-outline',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'complete', 'complete', 'error'),
      }
    case 'unsupportedProvider':
      return {
        ...base,
        title: text.status.unsupportedProvider.title,
        description: fill(text.status.unsupportedProvider.description),
        note: null,
        liveMessage: text.status.unsupportedProvider.title,
        tone: 'warning',
        icon: 'mdi:shape-outline',
        busy: false,
        showActions: true,
        steps: stepSet(text, 'complete', 'error', 'pending'),
      }
  }
}
