import React from 'react'
import ReactDOM from 'react-dom/client'
import AdminDashboard from './AdminDashboard'
import { TooltipProvider } from './components/ui/tooltip'
import { LanguageProvider } from './i18n'
import { ThemeProvider } from './theme'
import './index.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <ThemeProvider>
        <TooltipProvider delayDuration={120} skipDelayDuration={250}>
          <AdminDashboard />
        </TooltipProvider>
      </ThemeProvider>
    </LanguageProvider>
  </React.StrictMode>,
)
