import { Icon as IconifyIcon, addIcon } from '@iconify/react/offline'

import { BUNDLED_ICONS } from './icon-data'

type GuideClientIconId =
  | 'codex'
  | 'claude'
  | 'vscode'
  | 'claudeDesktop'
  | 'cursor'
  | 'windsurf'
  | 'cherryStudio'
  | 'other'

const GUIDE_CLIENT_ICON_NAMES: Record<GuideClientIconId, string> = {
  codex: 'simple-icons:openai',
  claude: 'simple-icons:anthropic',
  vscode: 'simple-icons:visualstudiocode',
  claudeDesktop: 'simple-icons:anthropic',
  cursor: 'simple-icons:cursor',
  windsurf: 'simple-icons:codeium',
  cherryStudio: 'mdi:fruit-cherries',
  other: 'mdi:dots-horizontal',
}

let iconsRegistered = false

function registerAppIcons(): void {
  if (iconsRegistered) return
  iconsRegistered = true

  for (const [iconName, iconData] of Object.entries(BUNDLED_ICONS)) {
    addIcon(iconName, iconData)
  }
}

registerAppIcons()

export const Icon = IconifyIcon

export function getGuideClientIconName(id: string): string {
  return GUIDE_CLIENT_ICON_NAMES[id as GuideClientIconId] ?? GUIDE_CLIENT_ICON_NAMES.other
}
