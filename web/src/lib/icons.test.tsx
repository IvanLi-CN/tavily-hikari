import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { Icon, getGuideClientIconName } from './icons'

describe('local icon registry', () => {
  it('maps guide clients to bundled icons with a stable local fallback', () => {
    expect(getGuideClientIconName('codex')).toBe('simple-icons:openai')
    expect(getGuideClientIconName('cherryStudio')).toBe('mdi:fruit-cherries')
    expect(getGuideClientIconName('unknown-client')).toBe('mdi:dots-horizontal')
  })

  it('renders bundled UI, brand, flag, and alias icons without remote Iconify URLs', () => {
    const html = renderToStaticMarkup(
      <div>
        <Icon icon="mdi:check" width={16} height={16} />
        <Icon icon="simple-icons:openai" width={16} height={16} />
        <Icon icon="twemoji:flag-china" width={18} height={18} />
        <Icon icon="mdi:trash-outline" width={16} height={16} />
      </div>,
    )

    expect(html).toContain('<svg')
    expect(html).not.toContain('api.iconify.design')
  })
})
