import { Icon } from '@iconify/react'

import type { VersionInfo } from '../api'

const REPO_URL = 'https://github.com/IvanLi-CN/tavily-hikari'

export interface UserConsoleFooterStrings {
  title: string
  githubAria: string
  githubLabel: string
  loadingVersion: string
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

  const clean = raw.replace(/-.+$/, '')
  const tag = clean.startsWith('v') ? clean : `v${clean}`
  return {
    href: `${REPO_URL}/releases/tag/${tag}`,
    label: `v${raw}`,
  }
}

export default function UserConsoleFooter({
  strings,
  version,
}: {
  strings: UserConsoleFooterStrings
  version: VersionInfo | null
}): JSX.Element {
  const release = buildUserConsoleFooterRelease(version)

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
        ) : (
          strings.loadingVersion
        )}
      </span>
    </footer>
  )
}
