export default function DebugInfoSharingToggle({
  shared,
  disabled,
  saving,
  error,
  text,
  onChange,
}: {
  shared: boolean
  disabled: boolean
  saving: boolean
  error: string | null
  text: {
    debugSharing: string
    debugSharingHint: string
    debugSharingSaving: string
  }
  onChange: (shared: boolean) => void
}): JSX.Element {
  return (
    <div className="access-stat user-console-debug-sharing">
      <label className="inline-flex items-start gap-3 text-sm">
        <input
          id="user-console-debug-info-sharing"
          name="user_console_debug_info_sharing"
          type="checkbox"
          checked={shared}
          disabled={disabled}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span>
          <span className="access-stat-title">{text.debugSharing}</span>
          <span className="block text-xs text-muted-foreground">{text.debugSharingHint}</span>
        </span>
      </label>
      {(saving || error) && (
        <p className="mt-2 text-xs" role="status" aria-live="polite">
          {saving ? text.debugSharingSaving : error}
        </p>
      )}
    </div>
  )
}
