import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { LanguageProvider } from '../i18n'
import KeyStickyPanels from './KeyStickyPanels'
import { stickyNodesStoryData, stickyUsersStoryData, stickyUsersStoryPerPage, stickyUsersStoryTotal } from './keyStickyStoryData'

function renderStickyPanels(): string {
  return renderToStaticMarkup(
    <LanguageProvider initialLanguage="zh">
      <KeyStickyPanels
        stickyUsers={stickyUsersStoryData}
        stickyUsersLoadState="ready"
        stickyUsersError={null}
        stickyUsersPage={1}
        stickyUsersTotal={stickyUsersStoryTotal}
        stickyUsersPerPage={stickyUsersStoryPerPage}
        stickyNodes={stickyNodesStoryData}
        stickyNodesLoadState="ready"
        stickyNodesError={null}
        onStickyUsersPrevious={() => undefined}
        onStickyUsersNext={() => undefined}
        onOpenUser={() => undefined}
      />
    </LanguageProvider>,
  )
}

describe('KeyStickyPanels sticky nodes view', () => {
  it('renders primary and secondary assignment counts instead of the old trend columns', () => {
    const html = renderStickyPanels()

    expect(html).toContain('作为主节点的 key 数')
    expect(html).toContain('作为备节点的 key 数')
    expect(html).toContain('>3<')
    expect(html).toContain('>1<')
    expect(html).not.toContain('24h 活动')
    expect(html).not.toContain('24h 权重')
    expect(html).not.toContain('窗口')
  })
})
