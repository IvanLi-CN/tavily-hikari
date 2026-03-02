import React from 'react'
import ReactDOM from 'react-dom/client'

import { LanguageProvider } from './i18n'
import { ThemeProvider } from './theme'
import UserConsole from './UserConsole'
import './index.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <ThemeProvider>
        <UserConsole />
      </ThemeProvider>
    </LanguageProvider>
  </React.StrictMode>,
)
