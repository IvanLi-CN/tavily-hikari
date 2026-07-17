import { Icon } from '../lib/icons'
import { buildOctoRillReleaseLink, formatVersionDisplay } from '../lib/releaseLinks'

const REPO_URL = 'https://github.com/IvanLi-CN/tavily-hikari'

export default function PublicHomeFooter({
  versionLabel,
  version,
}: {
  versionLabel: string
  version: string | null
}): JSX.Element {
  const release = buildOctoRillReleaseLink(version)
  const displayVersion = formatVersionDisplay(version)

  return (
    <footer className="surface public-home-footer">
      <a className="footer-gh" href={REPO_URL} target="_blank" rel="noreferrer">
        <Icon icon="mdi:github" width={18} height={18} aria-hidden="true" style={{ color: '#2563eb' }} />
        <span>GitHub</span>
      </a>
      <div className="footer-version">
        <span>{versionLabel}</span>
        {release ? (
          <a href={release.href} target="_blank" rel="noreferrer">
            <code>{release.label}</code>
          </a>
        ) : (
          <code>{displayVersion ?? '—'}</code>
        )}
      </div>
    </footer>
  )
}
