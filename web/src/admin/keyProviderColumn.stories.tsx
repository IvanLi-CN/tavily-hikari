import type { Meta, StoryObj } from '@storybook/react-vite'

import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../components/ui/table'
import { useTranslate } from '../i18n'
import { KeyProviderBadge, KeyProviderFilter, PROVIDER_FILTER_ALL } from './keyProviderColumn'

/**
 * Plan todo 22 visual proof: a mixed-provider admin key table fragment with the
 * text provider badges (legacy NULL provider renders Tavily) and the provider
 * filter row control. Used as the happy-path QA capture target.
 */
function KeyProviderColumnProof(): JSX.Element {
  const shared = useTranslate().admin.providers.shared
  const rows: Array<{ id: string; provider?: unknown }> = [
    { id: 'legacy-key', provider: undefined },
    { id: 'serpapi-key', provider: 'serpapi' },
    { id: 'exa-key', provider: 'exa' },
    { id: 'brave-key', provider: 'brave' },
  ]
  return (
    <div className="surface panel" style={{ padding: 24, display: 'grid', gap: 16 }}>
      <div>
        <h2>Admin key provider column</h2>
        <p className="panel-description">
          Text badges only, one provider per row; legacy rows without a provider render Tavily.
        </p>
      </div>
      <KeyProviderFilter value={PROVIDER_FILTER_ALL} onChange={() => undefined} />
      <Table className="jobs-table api-keys-table">
        <TableHeader>
          <TableRow>
            <TableHead>Key</TableHead>
            <TableHead>{shared.tableHeader}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={row.id}>
              <TableCell>
                <code>{row.id}</code>
              </TableCell>
              <TableCell>
                <KeyProviderBadge provider={row.provider} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

const meta = {
  title: 'Admin/Fragments/KeyProviderColumn',
  component: KeyProviderColumnProof,
  tags: ['autodocs'],
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof KeyProviderColumnProof>

export default meta

type Story = StoryObj<typeof meta>

export const MixedProviderTable: Story = {
  globals: { language: 'en', themeMode: 'light' },
  parameters: { viewport: { defaultViewport: '1440-device-desktop' } },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 100))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of ['Provider', 'Tavily', 'SerpApi', 'Exa', 'Brave', 'All providers']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected provider column proof to contain: ${expected}`)
      }
    }
  },
}

export const MixedProviderTableZh: Story = {
  ...MixedProviderTable,
  globals: { language: 'zh', themeMode: 'light' },
}