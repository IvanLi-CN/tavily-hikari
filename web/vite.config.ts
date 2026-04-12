import { defineConfig, loadEnv, type ProxyOptions } from 'vite'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'

const rootDir = fileURLToPath(new URL('.', import.meta.url))
function withForwardAuth(target: string, forwardAuthValue: string): Partial<ProxyOptions> {
  return {
    target,
    changeOrigin: true,
    configure: (proxy) => {
      proxy.on('proxyReq', (proxyReq) => {
        proxyReq.setHeader('Remote-Email', forwardAuthValue)
      })
    },
  }
}

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, rootDir, '')
  const forwardAuthValue = env.VITE_FORWARD_EMAIL || process.env.VITE_FORWARD_EMAIL || 'admin@example.com'
  const proxyTarget = env.VITE_PROXY_TARGET || process.env.VITE_PROXY_TARGET || 'http://127.0.0.1:58087'

  return {
    root: rootDir,
    plugins: [
      react(),
      {
        name: 'rewrite-short-routes',
        configureServer(server) {
          server.middlewares.use((req, _res, next) => {
            const url = req.url ?? ''
            const parsed = new URL(url, 'http://localhost')
            const pathname = parsed.pathname
            if (
              (pathname === '/admin' || pathname.startsWith('/admin/')) &&
              pathname !== '/admin.html'
            ) {
              req.url = `/admin.html${parsed.search}`
            }
            if (
              (pathname === '/console' || pathname.startsWith('/console/'))
              && pathname !== '/console.html'
            ) {
              req.url = `/console.html${parsed.search}`
            }
            if (pathname === '/login' || pathname === '/login/') {
              req.url = `/login.html${parsed.search}`
            }
            if (pathname === '/registration-paused' || pathname === '/registration-paused/') {
              req.url = `/registration-paused.html${parsed.search}`
            }
            next()
          })
        },
      },
    ],
    server: {
      host: '127.0.0.1',
      port: 55173, // high port to avoid conflicts
      strictPort: true,
      proxy: {
        '/api': withForwardAuth(proxyTarget, forwardAuthValue),
        '/mcp': {
          target: proxyTarget,
          changeOrigin: true,
        },
        '/health': withForwardAuth(proxyTarget, forwardAuthValue),
      },
    },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
      rollupOptions: {
        input: {
          main: resolve(rootDir, 'index.html'),
          admin: resolve(rootDir, 'admin.html'),
          console: resolve(rootDir, 'console.html'),
          login: resolve(rootDir, 'login.html'),
          registrationPaused: resolve(rootDir, 'registration-paused.html'),
        },
      },
    },
  }
})
