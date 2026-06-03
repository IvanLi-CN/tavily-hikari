import { useCallback, useState, type Dispatch, type SetStateAction } from 'react'
import { updateUserDebugInfoSharing, type UserDashboard } from '../api'

export function useDebugInfoSharing({
  dashboard,
  setDashboard,
  failedTemplate,
  formatError,
}: {
  dashboard: UserDashboard | null
  setDashboard: Dispatch<SetStateAction<UserDashboard | null>>
  failedTemplate: string
  formatError: (template: string, values: { message: string }) => string
}): {
  saving: boolean
  error: string | null
  toggle: (shared: boolean) => Promise<void>
} {
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const toggle = useCallback(async (shared: boolean) => {
    if (!dashboard || saving) return
    const previous = dashboard.debugInfoShared
    setSaving(true)
    setError(null)
    setDashboard((current) => (current ? { ...current, debugInfoShared: shared } : current))
    try {
      const updated = await updateUserDebugInfoSharing(shared)
      setDashboard((current) => (current ? { ...current, debugInfoShared: updated.debugInfoShared } : current))
    } catch (err) {
      setDashboard((current) => (current ? { ...current, debugInfoShared: previous } : current))
      setError(formatError(failedTemplate, { message: err instanceof Error ? err.message : String(err) }))
    } finally {
      setSaving(false)
    }
  }, [dashboard, failedTemplate, formatError, saving, setDashboard])

  return { saving, error, toggle }
}
