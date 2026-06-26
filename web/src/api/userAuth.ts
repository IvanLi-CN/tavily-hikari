import { requestJson } from './runtime'

export type LinuxDoFinalizeOutcome =
  | 'success'
  | 'invalid_state'
  | 'registration_paused'
  | 'inactive_user'
  | 'upstream_failure'
  | 'server_error'

export interface LinuxDoFinalizeResult {
  outcome: LinuxDoFinalizeOutcome
  provider: 'linuxdo'
  redirectTo: string | null
  detail: string | null
}

export function finalizeLinuxDoAuth(
  code: string,
  state: string,
  signal?: AbortSignal,
): Promise<LinuxDoFinalizeResult> {
  return requestJson('/auth/linuxdo/finalize', {
    method: 'POST',
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code, state }),
    signal,
  })
}
