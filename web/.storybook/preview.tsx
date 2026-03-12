import type { Preview } from '@storybook/react-vite'
import React, { useEffect } from 'react'
import { INITIAL_VIEWPORTS } from 'storybook/viewport'

import '../src/index.css'
import { LanguageProvider, type Language, useLanguage } from '../src/i18n'
import { ThemeProvider, type ThemeMode, useTheme } from '../src/theme'

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
      return (
        <LanguageProvider>
          <ThemeProvider>
            <SyncGlobals language={language} themeMode={themeMode}>
              <Story />
            </SyncGlobals>
          </ThemeProvider>
        </LanguageProvider>
      )
    },
  ],
}

export default preview
