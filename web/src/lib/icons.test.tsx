import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { Icon, getGuideClientIconName } from './icons'

const REQUIRED_RUNTIME_ICON_NAMES = [
  'circle-flags:cn',
  'circle-flags:gb',
  'mdi:account-group-outline',
  'mdi:account-off-outline',
  'mdi:alert-circle',
  'mdi:alert-circle-outline',
  'mdi:arrow-left',
  'mdi:bell-ring-outline',
  'mdi:calendar-clock-outline',
  'mdi:calendar-today-outline',
  'mdi:chart-timeline-variant',
  'mdi:check',
  'mdi:check-bold',
  'mdi:check-circle',
  'mdi:check-circle-outline',
  'mdi:chevron-down',
  'mdi:chevron-up',
  'mdi:circle-outline',
  'mdi:clock-time-four-outline',
  'mdi:close',
  'mdi:close-circle-outline',
  'mdi:content-copy',
  'mdi:cog-outline',
  'mdi:crown-outline',
  'mdi:dots-horizontal',
  'mdi:eye-off-outline',
  'mdi:eye-outline',
  'mdi:file-document-outline',
  'mdi:filter-outline',
  'mdi:filter-variant',
  'mdi:fruit-cherries',
  'mdi:github',
  'mdi:help-circle-outline',
  'mdi:information-outline',
  'mdi:key-chain-variant',
  'mdi:key-change',
  'mdi:key-outline',
  'mdi:lock-outline',
  'mdi:loading',
  'mdi:map-marker-radius-outline',
  'mdi:menu',
  'mdi:monitor-dashboard',
  'mdi:minus-circle-outline',
  'mdi:open-in-new',
  'mdi:pause-circle-outline',
  'mdi:pencil-outline',
  'mdi:play-circle-outline',
  'mdi:progress-helper',
  'mdi:refresh',
  'mdi:share-variant',
  'mdi:shield-check-outline',
  'mdi:trash-can-outline',
  'mdi:trash-outline',
  'mdi:tray-arrow-down',
  'mdi:tune',
  'mdi:tune-variant',
  'mdi:view-dashboard-outline',
  'simple-icons:anthropic',
  'simple-icons:codeium',
  'simple-icons:cursor',
  'simple-icons:openai',
  'simple-icons:visualstudiocode',
] as const

describe('local icon registry', () => {
  it('maps guide clients to bundled icons with a stable local fallback', () => {
    expect(getGuideClientIconName('codex')).toBe('simple-icons:openai')
    expect(getGuideClientIconName('claude')).toBe('simple-icons:anthropic')
    expect(getGuideClientIconName('vscode')).toBe('simple-icons:visualstudiocode')
    expect(getGuideClientIconName('claudeDesktop')).toBe('simple-icons:anthropic')
    expect(getGuideClientIconName('cursor')).toBe('simple-icons:cursor')
    expect(getGuideClientIconName('windsurf')).toBe('simple-icons:codeium')
    expect(getGuideClientIconName('cherryStudio')).toBe('mdi:fruit-cherries')
    expect(getGuideClientIconName('unknown-client')).toBe('mdi:dots-horizontal')
  })

  it('renders every required runtime icon locally without remote Iconify URLs', () => {
    const html = renderToStaticMarkup(
      <div>
        {REQUIRED_RUNTIME_ICON_NAMES.map((iconName) => (
          <Icon key={iconName} icon={iconName} width={18} height={18} />
        ))}
      </div>,
    )

    expect(html.match(/<svg/g)?.length).toBe(REQUIRED_RUNTIME_ICON_NAMES.length)
    expect(html).not.toContain('api.iconify.design')
  })
})
