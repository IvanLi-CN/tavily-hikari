import React from 'react'
import ReactDOM from 'react-dom/client'
import { installDemoRuntime } from './api/demo'
import { LanguageProvider } from './i18n'
import RegistrationPaused from './pages/RegistrationPaused'
import { ThemeProvider } from './theme'
import './index.css'

installDemoRuntime()

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <ThemeProvider>
        <RegistrationPaused />
      </ThemeProvider>
    </LanguageProvider>
  </React.StrictMode>,
)
