import '../../test/happydom'

import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import RollingNumber, { buildRollingCells } from './RollingNumber'

interface MockMediaQueryList {
  matches: boolean
  media: string
  onchange: ((event: MediaQueryListEvent) => void) | null
  addListener: (listener: (event: MediaQueryListEvent) => void) => void
  removeListener: (listener: (event: MediaQueryListEvent) => void) => void
  addEventListener: (type: string, listener: (event: MediaQueryListEvent) => void) => void
  removeEventListener: (type: string, listener: (event: MediaQueryListEvent) => void) => void
  dispatchEvent: (event: Event) => boolean
}

let root: Root | null = null
let container: HTMLDivElement | null = null
let reducedMotion = false
let animationFrameQueue: Array<FrameRequestCallback> = []

function installMatchMediaMock(): void {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string): MockMediaQueryList => ({
      matches: reducedMotion && query === '(prefers-reduced-motion: reduce)',
      media: query,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => true,
    }),
  })
}

function installAnimationFrameMock(): void {
  animationFrameQueue = []

  Object.defineProperty(window, 'requestAnimationFrame', {
    configurable: true,
    writable: true,
    value: (callback: FrameRequestCallback) => {
      animationFrameQueue.push(callback)
      return animationFrameQueue.length
    },
  })

  Object.defineProperty(window, 'cancelAnimationFrame', {
    configurable: true,
    writable: true,
    value: (frameId: number) => {
      const index = frameId - 1
      if (index >= 0 && index < animationFrameQueue.length) {
        animationFrameQueue[index] = () => undefined
      }
    },
  })
}

async function flushAnimationFrame(): Promise<void> {
  const queued = [...animationFrameQueue]
  animationFrameQueue = []

  await act(async () => {
    queued.forEach((callback) => callback(16))
  })
}

async function renderRollingNumber(value: number | null | undefined, loading = false): Promise<void> {
  if (!container) {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
  }

  await act(async () => {
    root?.render(<RollingNumber value={value} loading={loading} />)
  })
}

function digitColumns(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('.rn-col'))
}

beforeEach(() => {
  reducedMotion = false
  installMatchMediaMock()
  installAnimationFrameMock()
})

afterEach(async () => {
  if (root) {
    await act(async () => {
      root?.unmount()
    })
  }

  root = null
  container = null
  document.body.innerHTML = ''
})

describe('buildRollingCells', () => {
  it('animates only the rightmost three digits for the carry-chain case', () => {
    const cells = buildRollingCells(65_777, 66_876)
    const digits = cells.filter((cell) => cell.kind === 'digit')

    expect(digits.map((cell) => cell.char)).toEqual(['6', '6', '8', '7', '6'])
    expect(digits.map((cell) => cell.animate)).toEqual([false, false, true, true, true])
    expect(digits.slice(2).map((cell) => cell.direction)).toEqual(['down', 'down', 'down'])
    expect(digits.slice(2).map((cell) => cell.steps)).toEqual([1, 10, 9])
  })

  it('animates only the rightmost three digits for the borrow-chain case', () => {
    const cells = buildRollingCells(66_876, 65_777)
    const digits = cells.filter((cell) => cell.kind === 'digit')

    expect(digits.map((cell) => cell.char)).toEqual(['6', '5', '7', '7', '7'])
    expect(digits.map((cell) => cell.animate)).toEqual([false, false, true, true, true])
    expect(digits.slice(2).map((cell) => cell.direction)).toEqual(['up', 'up', 'up'])
    expect(digits.slice(2).map((cell) => cell.steps)).toEqual([1, 10, 9])
  })

  it('keeps animation scoped to the final group when crossing a comma boundary', () => {
    const cells = buildRollingCells(999, 1_000)
    const digits = cells.filter((cell) => cell.kind === 'digit')
    const separators = cells.filter((cell) => cell.kind === 'separator')

    expect(separators.map((cell) => cell.char)).toEqual([','])
    expect(digits.map((cell) => cell.char)).toEqual(['1', '0', '0', '0'])
    expect(digits.map((cell) => cell.animate)).toEqual([false, true, true, true])
    expect(digits.slice(1).map((cell) => cell.direction)).toEqual(['down', 'down', 'down'])
  })

  it('keeps rendered digits aligned to the animated suffix when digit count shrinks', () => {
    const cells = buildRollingCells(1_000, 999)
    const digits = cells.filter((cell) => cell.kind === 'digit')

    expect(digits.map((cell) => cell.char)).toEqual(['9', '9', '9'])
    expect(digits.map((cell) => cell.animate)).toEqual([true, true, true])
    expect(digits.map((cell) => cell.direction)).toEqual(['up', 'up', 'up'])
    expect(digits.map((cell) => cell.steps)).toEqual([1, 1, 1])
  })

  it('disables rolling when reduced motion is requested', () => {
    const cells = buildRollingCells(65_777, 66_876, { reducedMotion: true })
    const digits = cells.filter((cell) => cell.kind === 'digit')

    expect(digits.every((cell) => !cell.animate && cell.direction === 'none' && cell.steps === 0)).toBe(true)
  })
})

describe('RollingNumber DOM rendering', () => {
  it('renders only the suffix digits as animated columns after a carry-chain update', async () => {
    await renderRollingNumber(65_777)
    await renderRollingNumber(66_876)
    await flushAnimationFrame()

    const columns = digitColumns()
    expect(columns).toHaveLength(5)
    expect(columns.map((column) => column.dataset.rnAnimate)).toEqual(['false', 'false', 'true', 'true', 'true'])
    expect(columns.map((column) => column.dataset.rnDirection)).toEqual(['none', 'none', 'down', 'down', 'down'])
    expect(columns.map((column) => column.dataset.rnSteps)).toEqual(['0', '0', '1', '10', '9'])
    expect(columns.slice(2).map((column) => Number(column.dataset.rnStartIndex))).toEqual([27, 27, 27])
    expect(columns.slice(2).map((column) => Number(column.dataset.rnEndIndex))).toEqual([28, 37, 36])
  })

  it('renders em dash while loading without digit columns', async () => {
    await renderRollingNumber(null, true)

    expect(document.body.textContent).toContain('—')
    expect(digitColumns()).toHaveLength(0)
  })

  it('honors prefers-reduced-motion by rendering static columns only', async () => {
    reducedMotion = true
    installMatchMediaMock()
    installAnimationFrameMock()

    await renderRollingNumber(66_876)
    await renderRollingNumber(65_777)
    await flushAnimationFrame()

    const columns = digitColumns()
    expect(columns).toHaveLength(5)
    expect(columns.every((column) => column.dataset.rnAnimate === 'false')).toBe(true)
    expect(columns.every((column) => column.dataset.rnDirection === 'none')).toBe(true)
  })

  it('aligns boundary decreases with the rightmost padded suffix in the DOM', async () => {
    await renderRollingNumber(1_000)
    await renderRollingNumber(999)
    await flushAnimationFrame()

    const columns = digitColumns()
    expect(columns).toHaveLength(3)
    expect(columns.map((column) => column.dataset.rnAnimate)).toEqual(['true', 'true', 'true'])
    expect(columns.map((column) => column.dataset.rnDirection)).toEqual(['up', 'up', 'up'])
    expect(columns.map((column) => column.dataset.rnSteps)).toEqual(['1', '1', '1'])
    expect(columns.map((column) => Number(column.dataset.rnStartIndex))).toEqual([20, 20, 20])
    expect(columns.map((column) => Number(column.dataset.rnEndIndex))).toEqual([19, 19, 19])
  })
})
