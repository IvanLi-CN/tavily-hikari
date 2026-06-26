import '../../test/happydom'

import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { ThemeProvider } from '../theme'
import { TooltipProvider } from '../components/ui/tooltip'
import { ZH } from '../i18n/translations/zh'
import AdminUserRankingsPage, { type RankingTabKey } from './AdminUserRankingsPage'
import { rankingsStorySnapshot } from './rankingsStoryData'

interface MountedRankingsPage {
  container: HTMLDivElement
  root: Root
  selectedUsers: string[]
}

const OriginalImage = globalThis.Image

class StaticImageMock {
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  referrerPolicy = ''
  set src(_value: string) {}
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => window.setTimeout(resolve, 0))
  })
}

async function mountRankingsPage(activeTab: RankingTabKey = 'last24h'): Promise<MountedRankingsPage> {
  const selectedUsers: string[] = []
  const container = document.createElement('div')
  container.style.width = '1440px'
  container.style.minHeight = '960px'
  document.body.appendChild(container)
  const root = createRoot(container)

  await act(async () => {
    root.render(
      <ThemeProvider>
        <TooltipProvider delayDuration={0} skipDelayDuration={0}>
          <AdminUserRankingsPage
            strings={ZH.admin.rankings}
            language="zh"
            snapshot={rankingsStorySnapshot}
            loading={false}
            error={null}
            connectionState="live"
            showHeader={false}
            activeTab={activeTab}
            onTabChange={() => {}}
            onRetry={() => {}}
            onSelectUser={(userId) => selectedUsers.push(userId)}
          />
        </TooltipProvider>
      </ThemeProvider>,
    )
  })
  await flushEffects()

  return { container, root, selectedUsers }
}

function chartTitles(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll<HTMLElement>('.admin-ranking-card h3'))
    .map((title) => title.textContent?.trim() ?? '')
}

afterEach(() => {
  document.body.innerHTML = ''
})

beforeEach(() => {
  globalThis.Image = StaticImageMock as unknown as typeof Image
})

afterEach(() => {
  globalThis.Image = OriginalImage
})

describe('AdminUserRankingsPage rendering contracts', () => {
  it('shows three metric cards for a time-window tab', async () => {
    const { container, root } = await mountRankingsPage('last24h')

    expect(chartTitles(container)).toEqual(['主要调用', '积分', 'IP'])
    expect(container.textContent).toContain('按时间窗统计成功完成的主要调用次数')
    expect(container.textContent).toContain('按时间窗统计累计消耗的业务积分')
    expect(container.textContent).toContain('按时间窗统计唯一 IP 数')

    await act(async () => {
      root.unmount()
    })
  })

  it('shows three window cards for a metric tab', async () => {
    const { container, root } = await mountRankingsPage('uniqueIp')

    expect(chartTitles(container)).toEqual(['最近 24 小时', '最近 7 天', '最近 30 天'])
    expect(container.textContent).toContain('按时间窗统计唯一 IP 数')

    await act(async () => {
      root.unmount()
    })
  })

  it('propagates hover and focus state through the interactive hit layer and forwards user clicks', async () => {
    const { container, root, selectedUsers } = await mountRankingsPage('last24h')

    const hitTargets = Array.from(container.querySelectorAll<HTMLButtonElement>('.admin-ranking-chart-hit-target'))
    expect(hitTargets.length).toBeGreaterThan(0)
    const firstTarget = hitTargets[0]
    expect(firstTarget).not.toBeNull()

    await act(async () => {
      firstTarget.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))
    })
    await flushEffects()
    expect(container.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBeGreaterThanOrEqual(1)

    await act(async () => {
      firstTarget.focus()
    })
    await flushEffects()
    expect(container.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBeGreaterThanOrEqual(1)

    await act(async () => {
      firstTarget.click()
    })
    expect(selectedUsers).toHaveLength(1)
    expect(selectedUsers[0]).toBe('usr_alice_chen')

    await act(async () => {
      firstTarget.dispatchEvent(new MouseEvent('mouseout', { bubbles: true }))
      firstTarget.blur()
    })
    await flushEffects()
    expect(container.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBe(0)

    await act(async () => {
      root.unmount()
    })
  })
})
