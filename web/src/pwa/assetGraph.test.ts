import { expect, test } from 'bun:test'

import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const REQUIRED_ICON_SIZES = ['64x64', '96x96', '128x128', '144x144', '152x152', '167x167', '180x180', '192x192', '256x256', '384x384', '512x512', '1024x1024']
const HASHED_ICON_PATTERN = /^pwa\/(?:public|admin)(?:-maskable-\d+|-\d+|-touch-icon)-[a-f0-9]{12}\.png$/

type IconGraph = {
  any: Record<string, string>
  maskable: Record<string, string>
  touch: string
}

type PwaGraph = {
  manifest: string
  serviceWorker: string
  files: string[]
  precacheFiles: string[]
  icons: IconGraph
}

type AssetGraph = {
  public: PwaGraph
  admin: PwaGraph
}

function iconPaths(graph: IconGraph): string[] {
  return [...Object.values(graph.any), ...Object.values(graph.maskable), graph.touch]
}

function assertHashedIcon(distDir: string, relativePath: string): void {
  expect(relativePath).toMatch(HASHED_ICON_PATTERN)
  const content = fs.readFileSync(path.join(distDir, relativePath))
  const digest = createHash('sha256').update(content).digest('hex').slice(0, 12)
  expect(relativePath.endsWith(`-${digest}.png`)).toBe(true)
}

function readAssetGraph(): { distDir: string; graph: AssetGraph } | null {
  const distDir = path.resolve(import.meta.dir, '../../dist')
  const graphPath = path.join(distDir, 'pwa/asset-graphs.json')
  if (!fs.existsSync(graphPath)) return null
  return {
    distDir,
    graph: JSON.parse(fs.readFileSync(graphPath, 'utf8')) as AssetGraph,
  }
}

test('built asset graph keeps public and admin identities separated', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }
  const { distDir, graph } = build

  expect(graph.public.files).toContain('index.html')
  expect(graph.public.files).toContain('console.html')
  expect(graph.public.files).toContain('login.html')
  expect(graph.public.files).not.toContain('admin.html')
  expect(graph.admin.files).toContain('admin.html')
  expect(graph.admin.files).not.toContain('index.html')

  for (const identity of [graph.public, graph.admin]) {
    expect(identity.precacheFiles).toContain(identity.manifest)
    for (const iconPath of iconPaths(identity.icons)) {
      assertHashedIcon(distDir, iconPath)
      expect(identity.precacheFiles).toContain(iconPath)
    }
  }

  const publicIcons = iconPaths(graph.public.icons)
  const adminIcons = iconPaths(graph.admin.icons)
  expect(new Set([...publicIcons, ...adminIcons]).size).toBe(publicIcons.length + adminIcons.length)
  expect(graph.public.precacheFiles).not.toContain(graph.admin.manifest)
  expect(graph.admin.precacheFiles).not.toContain(graph.public.manifest)
})

test('built manifests expose full icon coverage and maskable entries', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const [identity, expected] of [
    ['public', { graph: build.graph.public, id: '/', scope: '/', startUrl: '/', name: 'Tavily Hikari' }],
    ['admin', { graph: build.graph.admin, id: '/admin/', scope: '/admin/', startUrl: '/admin/', name: 'Tavily Hikari Admin' }],
  ] as const) {
    const manifestPath = path.join(build.distDir, expected.graph.manifest)
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as {
      id: string
      name: string
      start_url: string
      scope: string
      icons: Array<{ src: string; sizes: string; purpose?: string }>
    }
    expect(manifest).toMatchObject({
      id: expected.id,
      name: expected.name,
      start_url: expected.startUrl,
      scope: expected.scope,
    })
    expect(expected.graph.manifest).toBe(identity === 'public' ? 'manifest.webmanifest' : 'manifest-admin.webmanifest')

    const manifestIconPaths = manifest.icons.map((icon) => icon.src.slice(1)).sort()
    expect(manifestIconPaths).toEqual(iconPaths(expected.graph.icons).filter((iconPath) => !iconPath.includes('touch-icon')).sort())
    for (const iconPath of manifestIconPaths) {
      assertHashedIcon(build.distDir, iconPath)
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

test('built HTML points to the matching manifest without a legacy touch-icon override', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const [htmlFile, manifestPath] of [
    ['index.html', 'manifest.webmanifest'],
    ['console.html', 'manifest.webmanifest'],
    ['login.html', 'manifest.webmanifest'],
    ['registration-paused.html', 'manifest.webmanifest'],
    ['admin.html', 'manifest-admin.webmanifest'],
  ]) {
    const html = fs.readFileSync(path.join(build.distDir, htmlFile), 'utf8')
    expect(html).toContain(`rel="manifest" href="/${manifestPath}"`)
    expect(html).not.toContain('apple-touch-icon')
  }
})

test('built service workers wait for explicit update activation after precache', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const identity of [build.graph.public, build.graph.admin]) {
    const serviceWorkerPath = path.join(build.distDir, identity.serviceWorker)
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    expect(source).toContain("ACTIVATE_UPDATE_MESSAGE = 'TAVILY_HIKARI_ACTIVATE_UPDATE'")
    expect(source).toContain('event.data.type === ACTIVATE_UPDATE_MESSAGE')
    expect(source).toContain('event.waitUntil(self.skipWaiting())')
    expect(source).not.toContain('await self.skipWaiting();')
    expect(source).not.toContain('self.clients.claim()')
    expect(source).toContain("cache.addAll(PRECACHE_URLS.map((url) => new Request(new URL(url, self.location.origin), { cache: 'reload' }))")
    expect(source).toContain(`/${identity.manifest}`)
    for (const precachedFile of identity.precacheFiles) {
      expect(source).toContain(`/${precachedFile}`)
    }
  }
})

test('built service workers reload their own manifest and icon URLs during installation', async () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  type WorkerEvent = {
    request: Request
    respondWith: (response: Promise<Response>) => void
    waitUntil: (promise: Promise<unknown>) => void
  }

  for (const [identity, otherIdentity] of [
    [build.graph.public, build.graph.admin],
    [build.graph.admin, build.graph.public],
  ]) {
    const source = fs.readFileSync(path.join(build.distDir, identity.serviceWorker), 'utf8')
    const listeners = new Map<string, (event: WorkerEvent) => void>()
    const requests: Request[] = []
    let installPromise: Promise<unknown> | null = null

    const workerSelf = {
      location: { origin: 'https://hikari.test' },
      addEventListener(type: string, listener: (event: WorkerEvent) => void) {
        listeners.set(type, listener)
      },
    }
    const workerCaches = {
      open: async () => ({
        addAll: async (entries: Request[]) => {
          requests.push(...entries)
        },
      }),
    }
    const requestOptions: RequestInit[] = []
    const requestConstructor = globalThis.Request
    const workerRequest = function(input: RequestInfo | URL, init?: RequestInit): Request {
      requestOptions.push(init ?? {})
      return new requestConstructor(input, init)
    }

    new Function('self', 'caches', 'Request', 'Response', 'fetch', source)(
      workerSelf,
      workerCaches,
      workerRequest,
      Response,
      globalThis.fetch,
    )
    const installListener = listeners.get('install')
    expect(installListener).toBeDefined()
    installListener?.({
      request: new requestConstructor('https://hikari.test/'),
      respondWith: () => undefined,
      waitUntil: (promise) => {
        installPromise = promise
      },
    })
    expect(installPromise).not.toBeNull()
    await installPromise

    expect(requests.length).toBeGreaterThan(0)
    expect(requestOptions).toHaveLength(requests.length)
    expect(requestOptions.every((options) => options.cache === 'reload')).toBe(true)
    const requestPaths = requests.map((request) => new URL(request.url).pathname)
    expect(requestPaths).toContain(`/${identity.manifest}`)
    expect(requestPaths).not.toContain(`/${otherIdentity.manifest}`)
  }
})

test('built service workers carry the release version in their identity', () => {
  const build = readAssetGraph()
  const versionPath = build ? path.join(build.distDir, 'version.json') : ''
  if (!build || !fs.existsSync(versionPath)) {
    expect(true).toBe(true)
    return
  }

  const version = JSON.parse(fs.readFileSync(versionPath, 'utf8')) as { version: string }
  expect(version.version).toBeString()
  for (const identity of [build.graph.public, build.graph.admin]) {
    const serviceWorkerPath = path.join(build.distDir, identity.serviceWorker)
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    expect(source).toContain(`const BUILD_VERSION = ${JSON.stringify(version.version)};`)
  }
})

test('built service workers leave network-only requests to the browser and contain runtime fetch failures', async () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const identity of [build.graph.public, build.graph.admin]) {
    const serviceWorkerPath = path.join(build.distDir, identity.serviceWorker)
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

      let networkOnlyResponse: Promise<Response> | null = null
      fetchListener?.({
        request: new Request('https://hikari.test/mcp/console/state?refresh=true'),
        respondWith: (response) => {
          networkOnlyResponse = response
        },
      })
      expect(networkOnlyResponse).toBeNull()

      let runtimeResponse: Promise<Response> | null = null
      fetchListener?.({
        request: new Request('https://hikari.test/assets/lazy-chunk.js'),
        respondWith: (response) => {
          runtimeResponse = response
        },
      })
      expect(runtimeResponse).not.toBeNull()
      const response = await runtimeResponse
      expect(response.status).toBe(503)
    } finally {
      Object.assign(globalThis, { self: originalSelf, fetch: originalFetch })
    }
  }
})
