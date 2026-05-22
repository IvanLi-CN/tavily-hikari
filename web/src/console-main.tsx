import React from 'react'
import ReactDOM from 'react-dom/client'

import { installDemoRuntime } from './api/demo'
import { TooltipProvider } from './components/ui/tooltip'
import { LanguageProvider } from './i18n'
import { ThemeProvider } from './theme'
import UserConsole from './UserConsole'
import './index.css'

installDemoRuntime()

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <ThemeProvider>
        <TooltipProvider delayDuration={120} skipDelayDuration={250}>
          <UserConsole />
        </TooltipProvider>
      </ThemeProvider>
    </LanguageProvider>
  </React.StrictMode>,
)
