import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import JobKeyLink from './JobKeyLink'

describe('JobKeyLink', () => {
  it('renders a desktop tooltip wrapper with the key group when enabled', () => {
    const html = renderToStaticMarkup(
      <JobKeyLink
        keyId="7QZ5"
        keyGroup="ops"
        ungroupedLabel="Ungrouped"
        detailLabel="Key details"
      />,
    )

    expect(html).toContain('href="/admin/keys/7QZ5"')
    expect(html).toContain('data-tip="ops"')
    expect(html).toContain('<code>7QZ5</code>')
  })

  it('omits the tooltip wrapper when mobile rendering disables bubbles', () => {
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
