import type { Preview } from '@storybook/react-vite'
import React, { useEffect } from 'react'
import { INITIAL_VIEWPORTS } from 'storybook/viewport'

import '../src/index.css'
import { TooltipProvider } from '../src/components/ui/tooltip'
import { LanguageProvider, type Language, useLanguage } from '../src/i18n'
import { ThemeProvider, type ThemeMode, useTheme } from '../src/theme'

const DEFAULT_LOCAL_DOCS_SITE_ORIGIN = 'http://127.0.0.1:56007'
const DOCS_ORIGIN_STORAGE_KEY = 'tavily-hikari.docs-origin'
const LOCAL_DOCS_SITE_PATHS = new Set([
  '/',
  '/index.html',
  '/quick-start.html',
  '/configuration-access.html',
  '/http-api-guide.html',
  '/deployment-anonymity.html',
  '/development.html',
  '/storybook.html',
  '/storybook-guide.html',
  '/zh/',
  '/zh/index.html',
  '/zh/quick-start.html',
  '/zh/configuration-access.html',
  '/zh/http-api-guide.html',
  '/zh/deployment-anonymity.html',
  '/zh/development.html',
  '/zh/storybook.html',
  '/zh/storybook-guide.html',
])

declare global {
  var __tavilyHikariStorybookDocsLinkSyncInstalled: boolean | undefined
}

const observedDocsRoots = new WeakSet<Document | ShadowRoot>()

function isValidOrigin(rawValue: string | null): rawValue is string {
  if (!rawValue) return false
  try {
    new URL(rawValue)
    return true
  } catch {
    return false
  }
}

function getLocalDocsSiteOrigin(): string {
  if (typeof window === 'undefined') return DEFAULT_LOCAL_DOCS_SITE_ORIGIN

  const docsOriginFromUrl = new URLSearchParams(window.location.search).get('docsOrigin')
  if (isValidOrigin(docsOriginFromUrl)) {
    window.localStorage.setItem(DOCS_ORIGIN_STORAGE_KEY, docsOriginFromUrl)
    return docsOriginFromUrl
  }

  const docsOriginFromStorage = window.localStorage.getItem(DOCS_ORIGIN_STORAGE_KEY)
  if (isValidOrigin(docsOriginFromStorage)) return docsOriginFromStorage

  return import.meta.env.VITE_DOCS_SITE_ORIGIN || DEFAULT_LOCAL_DOCS_SITE_ORIGIN
}

function isDocsSitePath(pathname: string): boolean {
  for (const candidate of LOCAL_DOCS_SITE_PATHS) {
    if (pathname === candidate || pathname.endsWith(candidate)) return true
  }
  return false
}

function resolveDocsSiteLink(url: URL): URL | null {
  if (url.origin !== window.location.origin) return null
  if (!isDocsSitePath(url.pathname)) return null
  if (import.meta.env.DEV && LOCAL_DOCS_SITE_PATHS.has(url.pathname)) {
    return new URL(`${url.pathname}${url.search}${url.hash}`, getLocalDocsSiteOrigin())
  }
  return url
}

function getDocsSiteTarget(anchor: HTMLAnchorElement): URL | null {
  const cachedTarget = anchor.dataset.hikariDocsTarget
  if (isValidOrigin(cachedTarget)) return new URL(cachedTarget)
  return resolveDocsSiteLink(new URL(anchor.href, window.location.origin))
}

function redirectToDocsSite(event: MouseEvent, anchor: HTMLAnchorElement): void {
  if (event.defaultPrevented) return
  if (event.button !== 0) return
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return

  const rewrittenTarget = getDocsSiteTarget(anchor)
  if (!rewrittenTarget) return

  event.preventDefault()
  event.stopPropagation()
  event.stopImmediatePropagation()
  window.top?.location.assign(rewrittenTarget.toString())
}

function rewriteDocsLinks(root: Document | ShadowRoot): void {
  for (const anchor of root.querySelectorAll<HTMLAnchorElement>('a[href]')) {
    const rewrittenTarget = resolveDocsSiteLink(new URL(anchor.href, window.location.origin))
    if (!rewrittenTarget) continue
    anchor.href = rewrittenTarget.toString()
    anchor.dataset.hikariDocsTarget = rewrittenTarget.toString()
    anchor.target = '_top'
    const relParts = new Set(anchor.rel.split(/\s+/).filter(Boolean))
    relParts.add('noopener')
    relParts.add('noreferrer')
    anchor.rel = Array.from(relParts).join(' ')
    anchor.onclick = (event) => {
      redirectToDocsSite(event, anchor)
    }
    if (anchor.dataset.hikariDocsLinkBound !== 'true') {
      anchor.addEventListener('click', (event) => redirectToDocsSite(event, anchor), true)
      anchor.dataset.hikariDocsLinkBound = 'true'
    }
  }
}

function installDocsNavigationInterception(targetDocument: Document): void {
  const interceptDocumentClick = (event: MouseEvent) => {
    if (event.defaultPrevented) return
    if (event.button !== 0) return
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return

    const rawTarget = event.target
    if (!(rawTarget instanceof Element)) return

    const anchor = rawTarget.closest<HTMLAnchorElement>('a[href]')
    if (!anchor) return

    redirectToDocsSite(event, anchor)
  }

  targetDocument.addEventListener('click', interceptDocumentClick, true)
}

function observeStorybookDocument(targetDocument: Document): void {
  if (observedDocsRoots.has(targetDocument)) {
    rewriteDocsLinks(targetDocument)
    return
  }

  const sync = () => rewriteDocsLinks(targetDocument)
  sync()
  installDocsNavigationInterception(targetDocument)
  new MutationObserver(sync).observe(targetDocument, {
    childList: true,
    subtree: true,
  })
  observedDocsRoots.add(targetDocument)
}

function installDocsLinkSync(): void {
  if (
    typeof document === 'undefined'
    || globalThis.__tavilyHikariStorybookDocsLinkSyncInstalled
  ) {
    return
  }

  observeStorybookDocument(document)

  const syncPreviewIframe = () => {
    const previewIframe = document.querySelector<HTMLIFrameElement>('#storybook-preview-iframe')
    if (!previewIframe) return
    const previewDocument = previewIframe.contentDocument
    if (previewDocument) observeStorybookDocument(previewDocument)
    if (previewIframe.dataset.hikariDocsSyncBound !== 'true') {
      previewIframe.addEventListener('load', syncPreviewIframe)
      previewIframe.dataset.hikariDocsSyncBound = 'true'
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener(
      'DOMContentLoaded',
      () => {
        syncPreviewIframe()
      },
      { once: true },
    )
  }

  syncPreviewIframe()
  globalThis.__tavilyHikariStorybookDocsLinkSyncInstalled = true
}

function SyncGlobals(props: {
  language: Language
  themeMode: ThemeMode
  children: React.ReactNode
}): JSX.Element {
  const { language, setLanguage } = useLanguage()
  const { mode, setMode } = useTheme()

  useEffect(() => {
    if (props.language !== language) setLanguage(props.language)
  }, [props.language, language, setLanguage])

  useEffect(() => {
    if (props.themeMode !== mode) setMode(props.themeMode)
  }, [props.themeMode, mode, setMode])

  return <>{props.children}</>
}

const viewportOptions = {
  // Sorted by width (ascending): devices + full breakpoints mixed together.
  '0390-device-iphone-14': {
    ...(INITIAL_VIEWPORTS.iphone14 ?? INITIAL_VIEWPORTS.iphonex ?? {}),
    name: 'iPhone 14 portrait (390px)',
  },
  '0430-device-iphone-14-pro-max': {
    ...(INITIAL_VIEWPORTS.iphone14promax ?? INITIAL_VIEWPORTS.iphone12promax ?? {}),
    name: 'iPhone 14 Pro Max portrait (430px)',
  },
  '0744-device-ipad-mini': {
    name: 'iPad mini portrait (744px)',
    styles: { width: '744px', height: '1133px' },
    type: 'tablet',
  },
  '0767-breakpoint-small-max': {
    name: 'Small max (767px)',
    styles: { width: '767px', height: '900px' },
    type: 'mobile',
  },
  '0768-device-ipad': {
    ...(INITIAL_VIEWPORTS.ipad ?? {}),
    name: 'iPad portrait (768px)',
  },
  '0912-device-surface-pro': {
    name: 'Surface Pro portrait (912px)',
    styles: { width: '912px', height: '1368px' },
    type: 'tablet',
  },
  '0920-breakpoint-content-compact-max': {
    name: 'Content compact max (920px)',
    styles: { width: '920px', height: '900px' },
    type: 'desktop',
  },
  '1024-device-ipad-pro-12': {
    ...(INITIAL_VIEWPORTS.ipad12p ?? {}),
    name: 'iPad Pro 12.9\" portrait (1024px)',
  },
  '1080-device-small-laptop': {
    name: 'Small laptop (1080px)',
    styles: { width: '1080px', height: '900px' },
    type: 'desktop',
  },
  '1100-breakpoint-admin-stack-max': {
    name: 'Admin stack max (1100px)',
    styles: { width: '1100px', height: '900px' },
    type: 'desktop',
  },
  '1200-device-laptop-13': {
    name: '13-inch laptop (1200px)',
    styles: { width: '1200px', height: '900px' },
    type: 'desktop',
  },
  '1440-device-desktop': {
    name: 'Desktop baseline (1440px)',
    styles: { width: '1440px', height: '900px' },
    type: 'desktop',
  },
}

const preview: Preview = {
  tags: ['autodocs'],
  parameters: {
    viewport: {
      options: viewportOptions,
    },
  },
  globalTypes: {
    language: {
      name: 'Language',
      description: 'UI language',
      defaultValue: 'en',
      toolbar: {
        icon: 'globe',
        items: [
          { value: 'en', title: 'English' },
          { value: 'zh', title: '中文' },
        ],
        dynamicTitle: true,
      },
    },
    themeMode: {
      name: 'Theme',
      description: 'UI theme',
      defaultValue: 'dark',
      toolbar: {
        icon: 'mirror',
        items: [
          { value: 'light', title: 'Light' },
          { value: 'dark', title: 'Dark' },
          { value: 'system', title: 'System' },
        ],
        dynamicTitle: true,
      },
    },
  },
  decorators: [
    (Story, context) => {
      const language = (context.globals.language ?? 'en') as Language
      const themeMode = (context.globals.themeMode ?? 'dark') as ThemeMode
      if (typeof document !== 'undefined') installDocsLinkSync()
      return (
        <LanguageProvider>
          <ThemeProvider>
            <TooltipProvider delayDuration={120} skipDelayDuration={250}>
              <SyncGlobals language={language} themeMode={themeMode}>
                <Story />
              </SyncGlobals>
            </TooltipProvider>
          </ThemeProvider>
        </LanguageProvider>
      )
    },
  ],
}

export default preview
