import React from 'react'
import ReactDOM from 'react-dom/client'
import PublicHome from './PublicHome'
import { LanguageProvider } from './i18n'
import './index.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <PublicHome />
    </LanguageProvider>
  </React.StrictMode>,
)
