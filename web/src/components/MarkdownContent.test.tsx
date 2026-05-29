import '../../test/happydom'

import { describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot } from 'react-dom/client'

import MarkdownContent from './MarkdownContent'

async function renderMarkdown(content: string): Promise<HTMLElement> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)

  await act(async () => {
    root.render(<MarkdownContent content={content} />)
  })

  return container
}

describe('MarkdownContent', () => {
  it('renders supported markdown without executing raw html', async () => {
    const container = await renderMarkdown(
      '**Important** [safe](https://example.com) <script>alert(1)</script>',
    )

    expect(container.querySelector('strong')?.textContent).toBe('Important')
    expect(container.querySelector('a')?.getAttribute('href')).toBe('https://example.com')
    expect(container.querySelector('script')).toBeNull()
    expect(container.innerHTML).not.toContain('<script')
  })

  it('drops unsafe links and images', async () => {
    const container = await renderMarkdown(
      '[bad](javascript:alert(1)) ![pixel](https://example.com/pixel.png)',
    )

    expect(container.querySelector('a')).toBeNull()
    expect(container.querySelector('img')).toBeNull()
    expect(container.textContent).toContain('bad')
  })
})
