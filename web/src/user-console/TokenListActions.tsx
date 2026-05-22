import type { TokenSecretCopyState } from '../components/TokenSecretField'
import type { EN } from './text'

type TokenText = typeof EN.tokens

interface TokenListActionsProps {
  tokenId: string
  text: TokenText
  copyState: TokenSecretCopyState
  onScheduleWarmSecret: (tokenId: string) => void
  onCancelWarmSecret: (tokenId: string) => void
  onWarmSecret: (tokenId: string) => void
  onCopy: (tokenId: string, anchorEl: HTMLElement) => void
  onDetail: (tokenId: string) => void
  onReset: (tokenId: string) => void
  isCopyIntentKey: (key: string) => boolean
  canReset: boolean
  className?: string
}

export default function TokenListActions({
  tokenId,
  text,
  copyState,
  onScheduleWarmSecret,
  onCancelWarmSecret,
  onWarmSecret,
  onCopy,
  onDetail,
  onReset,
  isCopyIntentKey,
  canReset,
  className = '',
}: TokenListActionsProps): JSX.Element {
  const copyClass = `btn btn-outline btn-sm ${copyState === 'copied' ? 'btn-success' : copyState === 'error' ? 'btn-warning' : ''}`
  const copyLabel = copyState === 'copied' ? text.copied : copyState === 'error' ? text.copyFailed : text.copy

  return (
    <div className={`table-actions ${className}`}>
      <button
        type="button"
        className={copyClass}
        onPointerEnter={() => onScheduleWarmSecret(tokenId)}
        onPointerLeave={() => onCancelWarmSecret(tokenId)}
        onBlur={() => onCancelWarmSecret(tokenId)}
        onPointerDown={() => onWarmSecret(tokenId)}
        onKeyDown={(event) => {
          if (!isCopyIntentKey(event.key)) return
          onWarmSecret(tokenId)
        }}
        onClick={(event) => onCopy(tokenId, event.currentTarget)}
      >
        {copyLabel}
      </button>
      <button type="button" className="btn btn-primary btn-sm" onClick={() => onDetail(tokenId)}>
        {text.detail}
      </button>
      {canReset ? (
        <button type="button" className="btn btn-warning btn-sm" onClick={() => onReset(tokenId)}>
          {text.reset}
        </button>
      ) : null}
    </div>
  )
}
