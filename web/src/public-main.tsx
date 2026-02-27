import React from 'react'
import ReactDOM from 'react-dom/client'
import { LanguageProvider } from './i18n'
import PublicHome from './PublicHome'
import { ThemeProvider } from './theme'
import './index.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <ThemeProvider>
        <PublicHome />
      </ThemeProvider>
    </LanguageProvider>
  </React.StrictMode>,
)
