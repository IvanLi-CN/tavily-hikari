import type { ReactNode } from 'react'

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
import type { UserTokenSummary } from '../api'
import type { EN } from './text'

interface SetupGuidePageProps {
  text: typeof EN.setup
  tokens: UserTokenSummary[]
  selectedTokenId: string | null
  onTokenChange: (tokenId: string) => void
  guide: ReactNode
}

function maskedTokenLabel(tokenId: string): string {
  return `th-${tokenId}-************************`
}

export default function SetupGuidePage({
  text,
  tokens,
  selectedTokenId,
  onTokenChange,
  guide,
}: SetupGuidePageProps): JSX.Element {
  return (
    <section className="surface panel user-console-setup-page">
      <header className="panel-header user-console-setup-header">
        <div className="user-console-setup-heading">
          <h2>{text.title}</h2>
          <p className="panel-description">{text.description}</p>
        </div>
        {selectedTokenId ? (
          <div className="user-console-setup-token-select">
            <span>{text.tokenLabel}</span>
            <Select value={selectedTokenId} onValueChange={onTokenChange}>
              <SelectTrigger aria-label={text.tokenSelectAria}>
                <SelectValue>{maskedTokenLabel(selectedTokenId)}</SelectValue>
              </SelectTrigger>
              <SelectContent align="end">
                {tokens.filter((token) => token.enabled).map((token) => (
                  <SelectItem key={token.tokenId} value={token.tokenId}>
                    {maskedTokenLabel(token.tokenId)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        ) : null}
      </header>
      {selectedTokenId ? guide : (
        <div className="empty-state user-console-setup-empty">
          <strong>{text.emptyTitle}</strong>
          <span>{text.emptyDescription}</span>
        </div>
      )}
    </section>
  )
}
