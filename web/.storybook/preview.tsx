import type { Preview } from '@storybook/react-vite'
import React, { useEffect } from 'react'

import '../src/index.css'
import { LanguageProvider, type Language, useLanguage } from '../src/i18n'
import { ThemeProvider } from '../src/theme'

function SyncGlobals(props: { language: Language; children: React.ReactNode }): JSX.Element {
  const { language, setLanguage } = useLanguage()

  useEffect(() => {
    if (props.language !== language) setLanguage(props.language)
  }, [props.language, language, setLanguage])

  return <>{props.children}</>
}

const preview: Preview = {
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
  },
  decorators: [
    (Story, context) => {
      const language = (context.globals.language ?? 'en') as Language
      return (
        <LanguageProvider>
          <ThemeProvider>
            <SyncGlobals language={language}>
              <Story />
            </SyncGlobals>
          </ThemeProvider>
        </LanguageProvider>
      )
    },
  ],
}

export default preview
