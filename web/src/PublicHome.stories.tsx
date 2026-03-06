import { useState } from 'react'
import { Icon } from '@iconify/react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import PublicHomeHeroCard from './components/PublicHomeHeroCard'
import LanguageSwitcher from './components/LanguageSwitcher'
import ThemeToggle from './components/ThemeToggle'
import { useTranslate } from './i18n'

const ICONIFY_ENDPOINT = 'https://api.iconify.design'

type CopyState = 'idle' | 'copied' | 'error'

interface PublicHomeStoryArgs {
  showAdminAction: boolean
}

function PublicHomeTokenModalStory(args: PublicHomeStoryArgs): JSX.Element {
  const strings = useTranslate().public
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
      <dialog open className="modal token-access-modal">
        <div className="modal-box">
          <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>
            {strings.tokenAccess.dialog.title}
          </h3>
          <p className="opacity-80" style={{ marginTop: 8 }}>
            {strings.tokenAccess.dialog.description}
          </p>
          <div className="token-input-wrapper" style={{ marginTop: 14 }}>
            <label htmlFor="story-token-input" className="token-label">
              {strings.accessToken.label}
            </label>
            <div className="token-input-row">
              <div className="token-input-shell">
                <input
                  id="story-token-input"
                  name="story-token-input"
                  className={`token-input${tokenVisible ? '' : ' masked'}`}
                  type="text"
                  value={tokenDraft}
                  onChange={(event) => setTokenDraft(event.target.value)}
                  placeholder={strings.accessToken.placeholder}
                  autoComplete="off"
                />
                <button
                  type="button"
                  className="token-visibility-button"
                  onClick={() => setTokenVisible((prev) => !prev)}
                  aria-label={tokenVisible ? strings.accessToken.toggle.hide : strings.accessToken.toggle.show}
                >
                  <img
                    src={`${ICONIFY_ENDPOINT}/mdi/${tokenVisible ? 'eye-off-outline' : 'eye-outline'}.svg?color=%236b7280`}
                    alt={strings.accessToken.toggle.iconAlt}
                  />
                </button>
              </div>
              <button
                type="button"
                className={`btn token-copy-button${
                  copyState === 'copied'
                    ? ' btn-success'
                    : copyState === 'error'
                      ? ' btn-warning'
                      : ' btn-outline'
                }`}
                onClick={() => void copyToken()}
                disabled={tokenDraft.trim().length === 0}
              >
                <Icon
                  icon={
                    copyState === 'copied'
                      ? 'mdi:check'
                      : copyState === 'error'
                        ? 'mdi:alert-circle-outline'
                        : 'mdi:content-copy'
                  }
                  aria-hidden="true"
                  className="token-copy-icon"
                />
                <span>
                  {copyState === 'copied'
                    ? strings.copyToken.copied
                    : copyState === 'error'
                      ? strings.copyToken.error
                      : strings.copyToken.copy}
                </span>
              </button>
            </div>
          </div>
          <p className="opacity-80" style={{ marginTop: 14, marginBottom: 0 }}>
            {strings.tokenAccess.dialog.loginHint}{' '}
            <a href="/auth/linuxdo" className="link">
              {strings.linuxDoLogin.button}
            </a>
          </p>
          <div className="modal-action">
            <button type="button" className="btn">
              {strings.tokenAccess.dialog.actions.cancel}
            </button>
            <button type="button" className="btn btn-primary" disabled={tokenDraft.trim().length === 0}>
              {strings.tokenAccess.dialog.actions.confirm}
            </button>
          </div>
        </div>
      </dialog>
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
