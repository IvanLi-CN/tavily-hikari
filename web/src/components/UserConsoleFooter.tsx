import { Icon } from '@iconify/react'

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
  const raw = version?.backend.trim() ?? ''
  if (raw.length === 0) {
    return null
  }

  // Only stable semver builds map cleanly to a GitHub release tag.
  if (!/^v?\d+\.\d+\.\d+$/.test(raw)) {
    return null
  }

  const tag = raw.startsWith('v') ? raw : `v${raw}`
  return {
    href: `${REPO_URL}/releases/tag/${tag}`,
    label: tag,
  }
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
    ? versionState.value?.backend?.trim() || null
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
            <span>{versionLabel.startsWith('v') ? versionLabel : `v${versionLabel}`}</span>
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
