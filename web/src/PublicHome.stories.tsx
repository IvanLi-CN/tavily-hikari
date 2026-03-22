import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import PublicHomeHeroCard from './components/PublicHomeHeroCard'
import LanguageSwitcher from './components/LanguageSwitcher'
import ThemeToggle from './components/ThemeToggle'
import TokenSecretField from './components/TokenSecretField'
import { Button } from './components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from './components/ui/dropdown-menu'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { useTranslate } from './i18n'
import { Icon, getGuideClientIconName } from './lib/icons'

type CopyState = 'idle' | 'copied' | 'error'

interface PublicHomeStoryArgs {
  showAdminAction: boolean
}

const guideProofLabels = [
  { id: 'codex', label: 'Codex CLI' },
  { id: 'claude', label: 'Claude Code' },
  { id: 'vscode', label: 'VS Code' },
] as const

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

function PublicHomeMobileGuideMenuProof(): JSX.Element {
  const active = guideProofLabels[0]

  return (
    <div
      style={{
        display: 'grid',
        gap: 20,
        maxWidth: 420,
        margin: '0 auto',
      }}
    >
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Mobile guide menu proof</h2>
            <p className="panel-description">
              The menu stays visible even when the guide card lives inside a clipped mobile shell.
            </p>
          </div>
        </div>
        <div
          style={{
            overflow: 'hidden',
            borderRadius: 28,
            border: '1px dashed hsl(var(--accent) / 0.42)',
            background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--muted) / 0.3))',
            padding: 18,
          }}
        >
          <div style={{ minHeight: 120 }}>
            <DropdownMenu open>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" size="sm" className="w-full justify-between md:h-10">
                  <span className="inline-flex items-center gap-2">
                    <Icon
                      icon={getGuideClientIconName(active.id)}
                      width={18}
                      height={18}
                      aria-hidden="true"
                      style={{ color: '#475569' }}
                    />
                    {active.label}
                  </span>
                  <Icon
                    icon="mdi:chevron-down"
                    width={16}
                    height={16}
                    aria-hidden="true"
                    style={{ color: '#647589' }}
                  />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="guide-select-menu p-1">
                {guideProofLabels.map((tab) => (
                  <DropdownMenuItem
                    key={tab.id}
                    className={`flex items-center gap-2 ${tab.id === active.id ? 'bg-accent/45 text-accent-foreground' : ''}`}
                  >
                    <Icon
                      icon={getGuideClientIconName(tab.id)}
                      width={16}
                      height={16}
                      aria-hidden="true"
                      style={{ color: '#475569' }}
                    />
                    <span className="truncate">{tab.label}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </section>
    </div>
  )
}

const meta = {
  title: 'Public/PublicHome',
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component: [
          'Public landing surface that brings together Linux DO login, token access, and the optional admin entry.',
          '',
          'Public docs: [Quick Start](../quick-start.html) · [Configuration & Access](../configuration-access.html) · [Storybook Guide](../storybook-guide.html)',
        ].join('\n'),
      },
    },
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

export const MobileGuideMenuProof: Story = {
  args: {
    showAdminAction: false,
  },
  render: () => <PublicHomeMobileGuideMenuProof />,
  parameters: {
    layout: 'padded',
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
}
