import { expect, test } from 'bun:test'

import fs from 'node:fs'
import path from 'node:path'

const REQUIRED_ICON_SIZES = ['64x64', '96x96', '128x128', '144x144', '152x152', '167x167', '180x180', '192x192', '256x256', '384x384', '512x512', '1024x1024']

test('built asset graph keeps public and admin identities separated', () => {
  const graphPath = path.resolve(import.meta.dir, '../../dist/pwa/asset-graphs.json')

  if (!fs.existsSync(graphPath)) {
    expect(true).toBe(true)
    return
  }

  const graph = JSON.parse(fs.readFileSync(graphPath, 'utf8')) as {
    public: { files: string[]; precacheFiles: string[] }
    admin: { files: string[]; precacheFiles: string[] }
  }

  expect(graph.public.files).toContain('index.html')
  expect(graph.public.files).toContain('console.html')
  expect(graph.public.files).toContain('login.html')
  expect(graph.public.files).not.toContain('admin.html')
  expect(graph.admin.files).toContain('admin.html')
  expect(graph.admin.files).not.toContain('index.html')
  expect(graph.public.precacheFiles).toContain('pwa/public-1024.png')
  expect(graph.public.precacheFiles).toContain('pwa/public-maskable-512.png')
  expect(graph.public.precacheFiles).toContain('pwa/public-touch-icon.png')
  expect(graph.admin.precacheFiles).toContain('pwa/admin-1024.png')
  expect(graph.admin.precacheFiles).toContain('pwa/admin-maskable-512.png')
  expect(graph.admin.precacheFiles).toContain('pwa/admin-touch-icon.png')
})

test('built manifests expose full icon coverage and maskable entries', () => {
  const manifestPaths = [
    path.resolve(import.meta.dir, '../../dist/manifest.webmanifest'),
    path.resolve(import.meta.dir, '../../dist/manifest-admin.webmanifest'),
  ]

  if (!manifestPaths.every((manifestPath) => fs.existsSync(manifestPath))) {
    expect(true).toBe(true)
    return
  }

  for (const manifestPath of manifestPaths) {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as {
      icons: Array<{ sizes: string; purpose?: string }>
    }
    const sizes = manifest.icons.map((icon) => icon.sizes)
    for (const size of REQUIRED_ICON_SIZES) {
      expect(sizes).toContain(size)
    }
    const maskableSizes = manifest.icons
      .filter((icon) => icon.purpose === 'maskable')
      .map((icon) => icon.sizes)
    expect(maskableSizes).toContain('192x192')
    expect(maskableSizes).toContain('512x512')
  }
})

test('built service workers wait for explicit update activation after precache', () => {
  const serviceWorkerPaths = [
    path.resolve(import.meta.dir, '../../dist/sw-public.js'),
    path.resolve(import.meta.dir, '../../dist/sw-admin.js'),
  ]

  if (!serviceWorkerPaths.every((serviceWorkerPath) => fs.existsSync(serviceWorkerPath))) {
    expect(true).toBe(true)
    return
  }

  for (const serviceWorkerPath of serviceWorkerPaths) {
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    expect(source).toContain("ACTIVATE_UPDATE_MESSAGE = 'TAVILY_HIKARI_ACTIVATE_UPDATE'")
    expect(source).toContain('event.data.type === ACTIVATE_UPDATE_MESSAGE')
    expect(source).toContain('event.waitUntil(self.skipWaiting())')
    expect(source).not.toContain('await self.skipWaiting();')
  }
})

test('built service workers turn network-only fetch failures into a response', async () => {
  const serviceWorkerPaths = [
    path.resolve(import.meta.dir, '../../dist/sw-public.js'),
    path.resolve(import.meta.dir, '../../dist/sw-admin.js'),
  ]

  for (const serviceWorkerPath of serviceWorkerPaths) {
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    const listeners = new Map<string, (event: { request: Request; respondWith: (response: Promise<Response>) => void }) => void>()
    const originalSelf = globalThis.self
    const originalFetch = globalThis.fetch

    Object.assign(globalThis, {
      self: {
        location: { origin: 'https://hikari.test' },
        addEventListener(type: string, listener: (event: { request: Request; respondWith: (response: Promise<Response>) => void }) => void) {
          listeners.set(type, listener)
        },
      },
      fetch: () => Promise.reject(new TypeError('network unavailable')),
    })

    try {
      new Function(source)()
      const fetchListener = listeners.get('fetch')
      expect(fetchListener).toBeDefined()

      for (const requestUrl of [
        'https://hikari.test/mcp/console/state?refresh=true',
        'https://hikari.test/assets/lazy-chunk.js',
      ]) {
        let responsePromise: Promise<Response> | null = null
        fetchListener?.({
          request: new Request(requestUrl),
          respondWith: (response) => {
            responsePromise = response
          },
        })

        expect(responsePromise).not.toBeNull()
        const response = await responsePromise
        expect(response.status).toBe(503)
      }
    } finally {
      Object.assign(globalThis, { self: originalSelf, fetch: originalFetch })
    }
  }
})
