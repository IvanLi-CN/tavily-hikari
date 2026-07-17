import { useId, useMemo } from 'react'

import type { Language } from '../i18n'
import type { AdminMcpSessionBindingsStatusView } from './routes'

import SegmentedTabs from '../components/ui/SegmentedTabs'

function copyFor(language: Language) {
  if (language === 'zh') {
    return {
      active: '活跃项',
      revoked: '已释放',
      all: '全部',
      ariaLabel: 'session 状态',
    }
  }

  return {
    active: 'Active',
    revoked: 'Revoked',
    all: 'All',
    ariaLabel: 'Session status',
  }
}

interface McpSessionBindingsStatusTabsProps {
  language: Language
  value: AdminMcpSessionBindingsStatusView
  onChange: (value: AdminMcpSessionBindingsStatusView) => void
}

export default function McpSessionBindingsStatusTabs({
  language,
  value,
  onChange,
}: McpSessionBindingsStatusTabsProps): JSX.Element {
  const labelId = useId()
  const copy = useMemo(() => copyFor(language), [language])

  return (
    <div
      role="group"
      aria-labelledby={labelId}
      style={{ display: 'grid', gap: 10, marginLeft: 'auto' }}
    >
      <span id={labelId} className="sr-only">
        status
      </span>
      <SegmentedTabs
        value={value}
        onChange={onChange}
        ariaLabel={copy.ariaLabel}
        options={[
          { value: 'active', label: copy.active },
          { value: 'revoked', label: copy.revoked },
          { value: 'all', label: copy.all },
        ]}
      />
    </div>
  )
}
