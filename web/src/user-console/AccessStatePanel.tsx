import { Icon } from '../lib/icons'
import type { EN } from './text'

type AccessText = Pick<typeof EN, 'unavailable' | 'loggedOut' | 'loginRequired'>

interface AccessStatePanelProps {
  state: 'unavailable' | 'logged_out' | 'login_required'
  text: AccessText
  onHome: () => void
}

export default function AccessStatePanel({ state, text, onHome }: AccessStatePanelProps): JSX.Element {
  const model = state === 'unavailable'
    ? { icon: 'mdi:account-off-outline', copy: text.unavailable, action: onHome }
    : state === 'logged_out'
      ? { icon: 'mdi:logout-variant', copy: text.loggedOut, action: () => { window.location.href = '/auth/linuxdo' } }
      : { icon: 'mdi:account-arrow-right-outline', copy: text.loginRequired, action: () => { window.location.href = '/auth/linuxdo' } }

  return (
    <section className="surface panel access-panel">
      <div className="console-unavailable-state">
        <div className="console-unavailable-icon" aria-hidden="true">
          <Icon icon={model.icon} width={22} height={22} />
        </div>
        <div className="console-unavailable-copy">
          <h2>{model.copy.title}</h2>
          <p>{model.copy.description}</p>
        </div>
        <div className="table-actions console-unavailable-actions">
          <button type="button" className="btn btn-primary" onClick={model.action}>
            {'home' in model.copy ? model.copy.home : model.copy.action}
          </button>
        </div>
      </div>
    </section>
  )
}
