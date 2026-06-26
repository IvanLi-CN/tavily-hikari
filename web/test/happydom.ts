import { GlobalRegistrator } from '@happy-dom/global-registrator'

GlobalRegistrator.register({
  url: 'http://localhost/',
})

;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const DEFAULT_CLIENT_WIDTH = 1280
const DEFAULT_CLIENT_HEIGHT = 720

function parsePx(raw: string): number | null {
  const value = Number.parseFloat(raw)
  return Number.isFinite(value) ? value : null
}

function inferClientWidth(element: HTMLElement): number {
  const inlineWidth = parsePx(element.style.width)
  if (inlineWidth && inlineWidth > 0) return inlineWidth
  if (element instanceof HTMLCanvasElement && element.width > 0) return element.width
  return DEFAULT_CLIENT_WIDTH
}

function inferClientHeight(element: HTMLElement): number {
  const inlineHeight = parsePx(element.style.height)
  if (inlineHeight && inlineHeight > 0) return inlineHeight
  if (element instanceof HTMLCanvasElement && element.height > 0) return element.height
  return DEFAULT_CLIENT_HEIGHT
}

Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
  configurable: true,
  get() {
    return inferClientWidth(this as HTMLElement)
  },
})

Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
  configurable: true,
  get() {
    return inferClientHeight(this as HTMLElement)
  },
})

const mockGradient = {
  addColorStop: () => undefined,
}

function createCanvas2dContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const base = {
    canvas,
    globalAlpha: 1,
    globalCompositeOperation: 'source-over',
    fillStyle: '#000',
    strokeStyle: '#000',
    lineWidth: 1,
    font: '12px sans-serif',
    textAlign: 'start',
    textBaseline: 'alphabetic',
    shadowBlur: 0,
    shadowColor: 'transparent',
    shadowOffsetX: 0,
    shadowOffsetY: 0,
    beginPath: () => undefined,
    closePath: () => undefined,
    moveTo: () => undefined,
    lineTo: () => undefined,
    bezierCurveTo: () => undefined,
    quadraticCurveTo: () => undefined,
    arc: () => undefined,
    arcTo: () => undefined,
    ellipse: () => undefined,
    rect: () => undefined,
    roundRect: () => undefined,
    fill: () => undefined,
    stroke: () => undefined,
    clip: () => undefined,
    save: () => undefined,
    restore: () => undefined,
    translate: () => undefined,
    rotate: () => undefined,
    scale: () => undefined,
    transform: () => undefined,
    setTransform: () => undefined,
    resetTransform: () => undefined,
    clearRect: () => undefined,
    fillRect: () => undefined,
    strokeRect: () => undefined,
    drawImage: () => undefined,
    fillText: () => undefined,
    strokeText: () => undefined,
    setLineDash: () => undefined,
    getLineDash: () => [],
    createLinearGradient: () => mockGradient,
    createRadialGradient: () => mockGradient,
    createPattern: () => null,
    measureText: (text: string) =>
      ({
        width: String(text).length * 7,
        actualBoundingBoxLeft: 0,
        actualBoundingBoxRight: String(text).length * 7,
        actualBoundingBoxAscent: 8,
        actualBoundingBoxDescent: 2,
        fontBoundingBoxAscent: 8,
        fontBoundingBoxDescent: 2,
      }) satisfies Partial<TextMetrics>,
    getImageData: () => ({ data: new Uint8ClampedArray(4), width: 1, height: 1 }),
    putImageData: () => undefined,
    isPointInPath: () => false,
    isPointInStroke: () => false,
  }

  return new Proxy(base, {
    get(target, property) {
      if (property in target) return target[property as keyof typeof target]
      return () => undefined
    },
    set(target, property, value) {
      ;(target as Record<PropertyKey, unknown>)[property] = value
      return true
    },
  }) as unknown as CanvasRenderingContext2D
}

const canvasContextCache = new WeakMap<HTMLCanvasElement, CanvasRenderingContext2D>()

Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', {
  configurable: true,
  writable: true,
  value(this: HTMLCanvasElement, type: string) {
    if (type !== '2d') return null
    const existing = canvasContextCache.get(this)
    if (existing) return existing
    const next = createCanvas2dContext(this)
    canvasContextCache.set(this, next)
    return next
  },
})

Object.defineProperty(HTMLCanvasElement.prototype, 'toDataURL', {
  configurable: true,
  writable: true,
  value() {
    return 'data:image/png;base64,'
  },
})
