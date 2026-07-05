import type { Meta, StoryObj } from '@storybook/react-vite'
import { expect, within } from 'storybook/test'

import type { Profile } from '../api'
import { installDemoRuntime } from '../api/demo'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import AdminLogin from './AdminLogin'

interface AdminLoginStoryProps {
  path?: string
  profile?: Profile
  profileUnavailable?: boolean
}

const baseProfile: Profile = {
  displayName: 'Hikari Demo Admin',
  isAdmin: true,
  forwardAuthEnabled: false,
  builtinAuthEnabled: true,
  passkeyAuthEnabled: true,
  adminLoginTotpRequired: true,
  allowRegistration: false,
  userLoggedIn: true,
  userProvider: 'linuxdo',
  userDisplayName: 'Hikari Demo Admin',
  userAvatarUrl: null,
}

function jsonResponse(value: unknown, init: ResponseInit = {}): Response {
  const headers = new Headers(init.headers)
  headers.set('Content-Type', 'application/json')
  return new Response(JSON.stringify(value), {
    ...init,
    headers,
  })
}

function installAdminLoginStoryRuntime(profile: Profile, profileUnavailable: boolean): void {
  installDemoRuntime()
  const passthrough = window.fetch.bind(window)
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, window.location.origin)
    const method = init?.method?.toUpperCase() ?? 'GET'
    if (url.pathname === '/api/profile') {
      return profileUnavailable
        ? new Response('profile unavailable', { status: 503 })
        : jsonResponse(profile)
    }
    if (url.pathname === '/api/admin/login' && method === 'POST') return jsonResponse({ ok: true })
    return passthrough(input, init)
  }
}

function AdminLoginStory({
  path = '/login',
  profile = baseProfile,
  profileUnavailable = false,
}: AdminLoginStoryProps): JSX.Element {
  window.localStorage.setItem('tavily-hikari-demo-mode', 'true')
  window.history.replaceState({}, '', path)
  installAdminLoginStoryRuntime(profile, profileUnavailable)
  return (
    <LanguageProvider>
      <ThemeProvider>
        <AdminLogin />
      </ThemeProvider>
    </LanguageProvider>
  )
}

const meta = {
  title: 'Public/Pages/AdminLogin',
  component: AdminLoginStory,
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component: 'Admin login page states for passkey login and break-glass password login.',
      },
    },
  },
  tags: ['autodocs'],
  render: (args) => <AdminLoginStory {...args} />,
} satisfies Meta<typeof AdminLoginStory>

export default meta

type Story = StoryObj<typeof meta>

export const PasskeyLogin: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByRole('button', { name: /passkey/i })).resolves.toBeInTheDocument()
    await expect(canvas.findByLabelText(/totp|验证码/i)).resolves.toBeInTheDocument()
  },
}

export const ResetEnrollment: Story = {
  args: {
    path: '/login?adminPasskeyResetToken=story-reset-token',
    profile: {
      ...baseProfile,
      isAdmin: false,
      builtinAuthEnabled: false,
      passkeyAuthEnabled: false,
      adminLoginTotpRequired: false,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByText(/one-time reset link|一次性重置链接/i)).resolves.toBeInTheDocument()
    await expect(canvas.findByRole('button', { name: /register passkey|注册 Passkey/i })).resolves.toBeInTheDocument()
  },
}

export const ResetEnrollmentComplete: Story = {
  args: {
    path: '/login?adminPasskeyRegistered=1',
    profile: {
      ...baseProfile,
      isAdmin: false,
      builtinAuthEnabled: false,
      passkeyAuthEnabled: true,
      adminLoginTotpRequired: false,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByText(/Passkey registered|Passkey 已注册/i)).resolves.toBeInTheDocument()
    await expect(canvas.findByRole('button', { name: /passkey/i })).resolves.toBeInTheDocument()
  },
}

export const PasswordOnly: Story = {
  args: {
    profile: {
      ...baseProfile,
      passkeyAuthEnabled: false,
      adminLoginTotpRequired: false,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByLabelText(/password|口令/i)).resolves.toBeInTheDocument()
    await expect(canvas.findByRole('button', { name: /sign in|登录/i })).resolves.toBeInTheDocument()
  },
}

export const PasskeyOnlyTotpRequired: Story = {
  args: {
    profile: {
      ...baseProfile,
      builtinAuthEnabled: false,
      passkeyAuthEnabled: true,
      adminLoginTotpRequired: true,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByLabelText(/totp|验证码/i)).resolves.toBeInTheDocument()
    await expect(canvas.findByRole('button', { name: /passkey/i })).resolves.toBeInTheDocument()
  },
}

export const NoLoginMethods: Story = {
  args: {
    profile: {
      ...baseProfile,
      builtinAuthEnabled: false,
      passkeyAuthEnabled: false,
      adminLoginTotpRequired: false,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByText(/disabled|未启用/i)).resolves.toBeInTheDocument()
  },
}

export const ProfileUnavailable: Story = {
  args: {
    profileUnavailable: true,
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.findByText(/unable to confirm|无法确认/i)).resolves.toBeInTheDocument()
  },
}

export const DarkTheme: Story = {
  globals: {
    themeMode: 'dark',
  },
}
