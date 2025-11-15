import { defineConfig, type ProxyOptions } from 'vite'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'

const rootDir = fileURLToPath(new URL('.', import.meta.url))
const forwardAuthValue = process.env.VITE_FORWARD_EMAIL ?? 'admin@example.com'

function withForwardAuth(): Partial<ProxyOptions> {
  return {
    target: 'http://127.0.0.1:58087',
    changeOrigin: true,
    configure: (proxy) => {
      proxy.on('proxyReq', (proxyReq) => {
        proxyReq.setHeader('Remote-Email', forwardAuthValue)
      })
    },
  }
}

// https://vitejs.dev/config/
export default defineConfig({
  root: rootDir,
  plugins: [react()],
  server: {
    host: '127.0.0.1',
    port: 55173, // high port to avoid conflicts
    strictPort: true,
    proxy: {
      '/api': withForwardAuth(),
      '/mcp': {
        target: 'http://127.0.0.1:58087',
        changeOrigin: true,
      },
      '/health': withForwardAuth(),
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: resolve(rootDir, 'index.html'),
        admin: resolve(rootDir, 'admin.html'),
      },
    },
  },
})
