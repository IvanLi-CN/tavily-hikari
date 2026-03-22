import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import JobKeyLink from './JobKeyLink'
import { TooltipProvider } from './ui/tooltip'

describe('JobKeyLink', () => {
  it('renders the desktop link without legacy tooltip attributes', () => {
    const html = renderToStaticMarkup(
      <TooltipProvider>
        <JobKeyLink
          keyId="7QZ5"
          keyGroup="ops"
          ungroupedLabel="Ungrouped"
          detailLabel="Key details"
        />
      </TooltipProvider>,
    )

    expect(html).toContain('href="/admin/keys/7QZ5"')
    expect(html).toContain('class="jobs-key-link"')
    expect(html).toContain('<code>7QZ5</code>')
    expect(html).not.toContain('data-tip=')
  })

  it('omits legacy tooltip attributes when mobile rendering disables bubbles', () => {
    const html = renderToStaticMarkup(
      <JobKeyLink
        keyId="7QZ5"
        keyGroup={null}
        ungroupedLabel="Ungrouped"
        detailLabel="Key details"
        showBubble={false}
      />,
    )

    expect(html).toContain('href="/admin/keys/7QZ5"')
    expect(html).not.toContain('data-tip=')
  })

  it('renders a dash when the job does not reference a key', () => {
    const html = renderToStaticMarkup(
      <JobKeyLink
        keyId={null}
        keyGroup={null}
        ungroupedLabel="Ungrouped"
        detailLabel="Key details"
      />,
    )

    expect(html).toContain('—')
    expect(html).not.toContain('href="/admin/keys/')
  })
})
