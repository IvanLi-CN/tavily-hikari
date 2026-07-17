import { Icon } from '../lib/icons'
import { buildOctoRillReleaseLink, formatVersionDisplay } from '../lib/releaseLinks'

import type { VersionInfo } from '../api'

const REPO_URL = 'https://github.com/IvanLi-CN/tavily-hikari'

export interface UserConsoleFooterStrings {
  title: string
  githubAria: string
  githubLabel: string
  loadingVersion: string
  errorVersion: string
  tagPrefix: string
}

export function buildUserConsoleFooterRelease(version: VersionInfo | null): {
  href: string
  label: string
} | null {
  return buildOctoRillReleaseLink(version?.backend)
}

export default function UserConsoleFooter({
  strings,
  versionState,
}: {
  strings: UserConsoleFooterStrings
  versionState:
    | { status: 'loading' }
    | { status: 'error' }
    | { status: 'ready'; value: VersionInfo | null }
}): JSX.Element {
  const release = versionState.status === 'ready'
    ? buildUserConsoleFooterRelease(versionState.value)
    : null
  const versionLabel = versionState.status === 'ready'
    ? formatVersionDisplay(versionState.value?.backend)
    : null

  return (
    <footer className="app-footer user-console-footer">
      <span>{strings.title}</span>
      <span className="footer-meta">
        <a
          href={REPO_URL}
          className="footer-link"
          target="_blank"
          rel="noreferrer"
          aria-label={strings.githubAria}
        >
          <Icon icon="mdi:github" width={18} height={18} className="footer-link-icon" />
          <span>{strings.githubLabel}</span>
        </a>
      </span>
      <span className="footer-meta">
        {release ? (
          <>
            {strings.tagPrefix}
            <a href={release.href} className="footer-link" target="_blank" rel="noreferrer">
              {release.label}
            </a>
          </>
        ) : versionLabel ? (
          <>
            {strings.tagPrefix}
            <span>{versionLabel}</span>
          </>
        ) : versionState.status === 'error' ? (
          strings.errorVersion
        ) : (
          strings.loadingVersion
        )}
      </span>
    </footer>
  )
}
