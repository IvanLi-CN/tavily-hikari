import React from 'react'
import ReactDOM from 'react-dom/client'
import AdminLogin from './pages/AdminLogin'
import { LanguageProvider } from './i18n'
import './index.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <AdminLogin />
    </LanguageProvider>
  </React.StrictMode>,
)

