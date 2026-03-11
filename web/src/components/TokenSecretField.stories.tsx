import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import TokenSecretField, { type TokenSecretCopyState } from './TokenSecretField'

function TokenSecretFieldStory(props: { visible?: boolean; copyState?: TokenSecretCopyState; copyDisabled?: boolean }): JSX.Element {
  const [value, setValue] = useState('tvly-demo-token-123456')
  const [visible, setVisible] = useState(props.visible ?? false)

  return (
    <div style={{ maxWidth: 720, margin: '0 auto' }}>
      <TokenSecretField
        inputId="storybook-token-secret"
        label="Access Token"
        value={value}
        visible={visible}
        copyState={props.copyState ?? 'idle'}
        copyDisabled={props.copyDisabled ?? false}
        onValueChange={setValue}
        onToggleVisibility={() => setVisible((current) => !current)}
        onCopy={() => undefined}
        visibilityShowLabel="Show access token"
        visibilityHideLabel="Hide access token"
        visibilityIconAlt="token visibility"
        copyAriaLabel="Copy token"
        copyLabel="Copy token"
        copiedLabel="Copied"
        copyErrorLabel="Copy failed"
      />
    </div>
  )
}

const meta = {
  title: 'Public/Wrappers/TokenSecretField',
  parameters: {
    layout: 'padded',
  },
  render: (args) => <TokenSecretFieldStory {...args} />,
} satisfies Meta<typeof TokenSecretFieldStory>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Visible: Story = {
  args: {
    visible: true,
  },
}

export const CopiedState: Story = {
  args: {
    visible: true,
    copyState: 'copied',
  },
}
