import { Icon } from '../lib/icons'
import type { OAuthCallbackPanelModel, OAuthCallbackStepState } from './oauthCallback'

interface OAuthCallbackPanelProps {
  model: OAuthCallbackPanelModel
  onRestart: () => void
  onHome: () => void
}

function stepStateIcon(state: OAuthCallbackStepState): string {
  if (state === 'complete') return 'mdi:check-circle'
  if (state === 'active') return 'mdi:progress-clock'
  if (state === 'error') return 'mdi:alert-circle'
  return 'mdi:circle-outline'
}

export default function OAuthCallbackPanel({
  model,
  onRestart,
  onHome,
}: OAuthCallbackPanelProps): JSX.Element {
  return (
    <section
      className="surface panel access-panel oauth-callback-panel"
      role="region"
      aria-label={`${model.badge} ${model.title}`}
    >
      <div className={`oauth-callback-hero oauth-callback-hero-${model.tone}`}>
        <div className="oauth-callback-hero-copy">
          <span className="oauth-callback-badge">{model.badge}</span>
          <div className="oauth-callback-title-row">
            <div className={`oauth-callback-icon-shell oauth-callback-icon-shell-${model.tone}`}>
              <Icon
                icon={model.icon}
                width={24}
                height={24}
                aria-hidden="true"
                className={model.busy ? 'oauth-callback-icon-spin' : undefined}
              />
            </div>
            <div className="oauth-callback-title-copy">
              <h2>{model.title}</h2>
              <p>{model.description}</p>
            </div>
          </div>
          {model.note ? (
            <p className="oauth-callback-note">{model.note}</p>
          ) : null}
        </div>
        <div className={`oauth-callback-status-pill oauth-callback-status-pill-${model.tone}`}>
          <span className={`oauth-callback-status-dot oauth-callback-status-dot-${model.tone}`} />
          <span>{model.liveMessage}</span>
        </div>
      </div>

      <ol className="oauth-callback-step-list" aria-label={model.badge}>
        {model.steps.map((step, index) => (
          <li
            key={`${step.label}:${index}`}
            className={`oauth-callback-step oauth-callback-step-${step.state}`}
          >
            <div className={`oauth-callback-step-icon oauth-callback-step-icon-${step.state}`}>
              <Icon
                icon={stepStateIcon(step.state)}
                width={18}
                height={18}
                aria-hidden="true"
                className={step.state === 'active' ? 'oauth-callback-icon-spin' : undefined}
              />
            </div>
            <span>{step.label}</span>
          </li>
        ))}
      </ol>

      {model.showActions ? (
        <div className="table-actions oauth-callback-actions">
          <button type="button" className="btn btn-primary" onClick={onRestart}>
            {model.primaryActionLabel}
          </button>
          <button type="button" className="btn btn-outline" onClick={onHome}>
            {model.secondaryActionLabel}
          </button>
        </div>
      ) : null}

      <p className="sr-only" role="status" aria-live="polite">
        {model.liveMessage}
      </p>
    </section>
  )
}
