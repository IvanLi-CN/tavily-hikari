import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import PublicHomeHeroCard from './components/PublicHomeHeroCard'
import LanguageSwitcher from './components/LanguageSwitcher'
import ThemeToggle from './components/ThemeToggle'
import TokenSecretField from './components/TokenSecretField'
import { Button } from './components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { useTranslate } from './i18n'

type CopyState = 'idle' | 'copied' | 'error'

interface PublicHomeStoryArgs {
  showAdminAction: boolean
}

function PublicHomeTokenModalStory(args: PublicHomeStoryArgs): JSX.Element {
  const strings = useTranslate().public
  const [open, setOpen] = useState(true)
  const [tokenDraft, setTokenDraft] = useState('')
  const [tokenVisible, setTokenVisible] = useState(false)
  const [copyState, setCopyState] = useState<CopyState>('idle')

  const copyToken = async () => {
    const next = tokenDraft.trim()
    if (!next) return
    try {
      await navigator.clipboard.writeText(next)
      setCopyState('copied')
      window.setTimeout(() => setCopyState('idle'), 1500)
    } catch {
      setCopyState('error')
      window.setTimeout(() => setCopyState('idle'), 1500)
    }
  }

  return (
    <main className="app-shell public-home">
      <PublicHomeHeroCard
        publicStrings={strings}
        loading={false}
        metrics={{ monthlySuccess: 1240, dailySuccess: 87 }}
        availableKeys={7}
        totalKeys={12}
        error={null}
        showLinuxDoLogin
        showTokenAccessButton
        showAdminAction={args.showAdminAction}
        adminActionLabel={strings.adminLoginButton}
        topControls={(
          <>
            <ThemeToggle />
            <LanguageSwitcher />
          </>
        )}
      />
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="token-access-modal max-w-xl">
          <DialogHeader>
            <DialogTitle>{strings.tokenAccess.dialog.title}</DialogTitle>
            <DialogDescription>{strings.tokenAccess.dialog.description}</DialogDescription>
          </DialogHeader>
          <TokenSecretField
            inputId="story-token-input"
            name="story-token-input"
            label={strings.accessToken.label}
            value={tokenDraft}
            visible={tokenVisible}
            copyState={copyState}
            onValueChange={setTokenDraft}
            onToggleVisibility={() => setTokenVisible((prev) => !prev)}
            onCopy={() => void copyToken()}
            placeholder={strings.accessToken.placeholder}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            aria-autocomplete="none"
            inputMode="text"
            data-1p-ignore="true"
            data-lpignore="true"
            data-form-type="other"
            visibilityShowLabel={strings.accessToken.toggle.show}
            visibilityHideLabel={strings.accessToken.toggle.hide}
            visibilityIconAlt={strings.accessToken.toggle.iconAlt}
            copyAriaLabel={strings.copyToken.iconAlt}
            copyLabel={strings.copyToken.copy}
            copiedLabel={strings.copyToken.copied}
            copyErrorLabel={strings.copyToken.error}
            copyDisabled={tokenDraft.trim().length === 0}
          />
          <p className="opacity-80" style={{ marginTop: 14, marginBottom: 0 }}>
            {strings.tokenAccess.dialog.loginHint}{' '}
            <a href="/auth/linuxdo" className="link">
              {strings.linuxDoLogin.button}
            </a>
          </p>
          <div className="modal-action">
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              {strings.tokenAccess.dialog.actions.cancel}
            </Button>
            <Button type="button" disabled={tokenDraft.trim().length === 0}>
              {strings.tokenAccess.dialog.actions.confirm}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </main>
  )
}

const meta = {
  title: 'Public/PublicHome',
  parameters: {
    layout: 'fullscreen',
  },
  render: (args) => <PublicHomeTokenModalStory {...args} />,
} satisfies Meta<PublicHomeStoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const TokenModalOpen: Story = {
  args: {
    showAdminAction: false,
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const TokenModalOpenWithAdminAction: Story = {
  args: {
    showAdminAction: true,
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}
