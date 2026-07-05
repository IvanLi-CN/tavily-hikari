import { useEffect, useMemo, useRef, useState } from 'react'
import type { ClipboardEvent, KeyboardEvent } from 'react'

import {
  confirmAdminTotp,
  createAdminTotpSetup,
  deleteAdminPasskey,
  deleteAdminPassword,
  disableAdminTotp,
  fetchAdminPasskeys,
  fetchAdminPasswordStatus,
  fetchAdminTotpStatus,
  registerAdminPasskey,
  setAdminLoginTotpRequired,
  setAdminPassword,
  updateAdminPasskeyLabel,
  type AdminPasskeys,
  type AdminPasswordStatus,
  type AdminTotpSetup,
  type AdminTotpStatus,
  type Profile,
} from '../api'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Button } from '../components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
import { Input } from '../components/ui/input'
import { Switch } from '../components/ui/switch'
import type { AdminTranslations } from '../i18n'
import { Icon } from '../lib/icons'

interface AdminSecuritySettingsModuleProps {
  strings: AdminTranslations['systemSettings']
  profile: Profile | null
  initialTotpStatus?: AdminTotpStatus | null
  initialPasskeys?: AdminPasskeys | null
  initialPasswordStatus?: AdminPasswordStatus | null
  disableAutoLoad?: boolean
}

type PendingSecurityAction =
  | { type: 'password-delete' }
  | { type: 'passkey-delete'; credentialId: string; label: string }
  | { type: 'totp-disable' }

function formatTimestamp(value: number | null | undefined, language: 'en' | 'zh'): string {
  if (value == null) return language === 'zh' ? '从未使用' : 'Never used'
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value * 1000))
}

function statusToneClass(active: boolean): string {
  return active
    ? 'border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
    : 'border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-300'
}

interface TotpCodeInputProps {
  id: string
  name: string
  value: string
  onChange: (value: string) => void
  ariaLabel: string
  autoFocus?: boolean
  disabled?: boolean
}

function TotpCodeInput({
  id,
  name,
  value,
  onChange,
  ariaLabel,
  autoFocus = false,
  disabled = false,
}: TotpCodeInputProps): JSX.Element {
  const inputsRef = useRef<Array<HTMLInputElement | null>>([])
  const digits = Array.from({ length: 6 }, (_, index) => value[index] ?? '')

  const focusCell = (index: number) => {
    inputsRef.current[Math.max(0, Math.min(5, index))]?.focus()
  }

  const writeDigits = (nextDigits: string[], focusIndex: number) => {
    onChange(nextDigits.join('').slice(0, 6))
    window.requestAnimationFrame(() => focusCell(focusIndex))
  }

  const handleCellChange = (index: number, rawValue: string) => {
    const nextValue = rawValue.replace(/\D/g, '')
    if (!nextValue) {
      const nextDigits = [...digits]
      nextDigits[index] = ''
      writeDigits(nextDigits, index)
      return
    }
    const nextDigits = [...digits]
    nextValue.slice(0, 6 - index).split('').forEach((digit, offset) => {
      nextDigits[index + offset] = digit
    })
    writeDigits(nextDigits, Math.min(5, index + nextValue.length))
  }

  const handleCellKeyDown = (index: number, event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Backspace' && !digits[index] && index > 0) {
      event.preventDefault()
      const nextDigits = [...digits]
      nextDigits[index - 1] = ''
      writeDigits(nextDigits, index - 1)
    }
    if (event.key === 'ArrowLeft' && index > 0) {
      event.preventDefault()
      focusCell(index - 1)
    }
    if (event.key === 'ArrowRight' && index < 5) {
      event.preventDefault()
      focusCell(index + 1)
    }
  }

  const handlePaste = (index: number, event: ClipboardEvent<HTMLInputElement>) => {
    const pastedDigits = event.clipboardData.getData('text').replace(/\D/g, '').slice(0, 6 - index)
    if (!pastedDigits) return
    event.preventDefault()
    const nextDigits = [...digits]
    pastedDigits.split('').forEach((digit, offset) => {
      nextDigits[index + offset] = digit
    })
    writeDigits(nextDigits, Math.min(5, index + pastedDigits.length - 1))
  }

  return (
    <div className="totp-code-input" role="group" aria-label={ariaLabel}>
      <input id={id} name={name} type="hidden" value={value} readOnly />
      {digits.map((digit, index) => (
        <input
          key={`totp-code-cell-${index}`}
          ref={(node) => {
            inputsRef.current[index] = node
          }}
          className="totp-code-input-cell"
          value={digit}
          disabled={disabled}
          inputMode="numeric"
          pattern="[0-9]*"
          autoComplete={index === 0 ? 'one-time-code' : 'off'}
          autoFocus={autoFocus && index === 0}
          aria-label={`${ariaLabel} ${index + 1}`}
          onChange={(event) => handleCellChange(index, event.target.value)}
          onKeyDown={(event) => handleCellKeyDown(index, event)}
          onPaste={(event) => handlePaste(index, event)}
        />
      ))}
    </div>
  )
}

export default function AdminSecuritySettingsModule({
  strings,
  profile,
  initialTotpStatus,
  initialPasskeys,
  initialPasswordStatus,
  disableAutoLoad = false,
}: AdminSecuritySettingsModuleProps): JSX.Element {
  const language = strings.subnav.admin === '管理员' ? 'zh' : 'en'
  const copy = strings.admin
  const [totpStatus, setTotpStatus] = useState<AdminTotpStatus | null>(initialTotpStatus ?? null)
  const [totpSetup, setTotpSetup] = useState<AdminTotpSetup | null>(null)
  const [totpCode, setTotpCode] = useState('')
  const [totpCurrentCode, setTotpCurrentCode] = useState('')
  const [totpBusy, setTotpBusy] = useState(false)
  const [totpError, setTotpError] = useState<string | null>(null)
  const [passwordStatus, setPasswordStatus] = useState<AdminPasswordStatus | null>(
    initialPasswordStatus ?? null,
  )
  const [passwordLoading, setPasswordLoading] = useState(!disableAutoLoad && initialPasswordStatus === undefined)
  const [passwordError, setPasswordError] = useState<string | null>(null)
  const [passwordMessage, setPasswordMessage] = useState<string | null>(null)
  const [passwordDraft, setPasswordDraft] = useState('')
  const [passwordConfirm, setPasswordConfirm] = useState('')
  const [passwordBusy, setPasswordBusy] = useState(false)
  const [passkeys, setPasskeys] = useState<AdminPasskeys | null>(initialPasskeys ?? null)
  const [passkeysLoading, setPasskeysLoading] = useState(!disableAutoLoad && initialPasskeys === undefined)
  const [passkeysError, setPasskeysError] = useState<string | null>(null)
  const [passkeysMessage, setPasskeysMessage] = useState<string | null>(null)
  const [passkeyBusy, setPasskeyBusy] = useState(false)
  const [newPasskeyLabel, setNewPasskeyLabel] = useState('')
  const [labelDrafts, setLabelDrafts] = useState<Record<string, string>>({})
  const [pendingSecurityAction, setPendingSecurityAction] = useState<PendingSecurityAction | null>(null)

  const builtinPasswordEnabled = passwordStatus?.enabled ?? profile?.builtinAuthEnabled === true
  const passkeyConfigured = passkeys?.configured === true
  const passkeyEnabled = passkeys?.enabled === true
  const passkeyCredentialCount = passkeys?.credentialCount ?? 0
  const totpEnabled = totpStatus?.enabled === true
  const loginTotpRequired = passwordStatus?.loginTotpRequired === true
  const passwordStatusLabel = builtinPasswordEnabled ? copy.passwordEnabled : copy.passwordDisabled
  const passkeyStatusLabel = passkeyConfigured
    ? passkeyEnabled
      ? copy.passkeyEnabled
      : copy.passkeyNoCredentials
    : copy.passkeyUnavailable
  const totpStatusLabel = totpEnabled ? copy.postureEnabled : copy.postureDisabled
  const activeLoginMethods = (builtinPasswordEnabled ? 1 : 0) + (passkeyCredentialCount > 0 ? 1 : 0)
  const postureWarning = activeLoginMethods <= 1 ? copy.postureWeak : copy.postureReady

  const passwordValidationError = useMemo(() => {
    if (!passwordDraft && !passwordConfirm) return null
    if (passwordDraft.length < 8) return copy.passwordTooShort
    if (passwordDraft !== passwordConfirm) return copy.passwordMismatch
    return null
  }, [copy.passwordMismatch, copy.passwordTooShort, passwordConfirm, passwordDraft])

  const loadPasswordStatus = async (signal?: AbortSignal) => {
    setPasswordLoading(true)
    setPasswordError(null)
    try {
      const next = await fetchAdminPasswordStatus(signal)
      setPasswordStatus(next)
    } catch (err) {
      if (!signal?.aborted) setPasswordError(err instanceof Error ? err.message : String(err))
    } finally {
      if (!signal?.aborted) setPasswordLoading(false)
    }
  }

  const loadPasskeys = async (signal?: AbortSignal) => {
    setPasskeysLoading(true)
    setPasskeysError(null)
    try {
      const next = await fetchAdminPasskeys(signal)
      setPasskeys(next)
    } catch (err) {
      if (!signal?.aborted) setPasskeysError(err instanceof Error ? err.message : String(err))
    } finally {
      if (!signal?.aborted) setPasskeysLoading(false)
    }
  }

  useEffect(() => {
    if (disableAutoLoad) return
    const controller = new AbortController()
    fetchAdminTotpStatus(controller.signal)
      .then(setTotpStatus)
      .catch((err: unknown) => {
        if (!controller.signal.aborted) setTotpError(err instanceof Error ? err.message : String(err))
      })
    void loadPasswordStatus(controller.signal)
    void loadPasskeys(controller.signal)
    return () => controller.abort()
  }, [disableAutoLoad])

  const passkeySummary = useMemo(
    () => copy.passkeyCredentialCount.replace('{count}', String(passkeyCredentialCount)),
    [copy.passkeyCredentialCount, passkeyCredentialCount],
  )

  const openSecurityAction = (action: PendingSecurityAction) => {
    setPendingSecurityAction(action)
  }

  const closeSecurityAction = () => {
    setPendingSecurityAction(null)
  }

  const submitPassword = async () => {
    if (passwordValidationError || !passwordDraft) return
    setPasswordBusy(true)
    setPasswordError(null)
    setPasswordMessage(null)
    try {
      const next = await setAdminPassword(passwordDraft)
      setPasswordStatus(next)
      setPasswordDraft('')
      setPasswordConfirm('')
      setPasswordMessage(copy.passwordUpdated)
    } catch (err) {
      setPasswordError(err instanceof Error ? err.message : String(err))
    } finally {
      setPasswordBusy(false)
    }
  }

  const removePassword = async () => {
    if (!builtinPasswordEnabled) return
    setPasswordBusy(true)
    setPasswordError(null)
    setPasswordMessage(null)
    try {
      const next = await deleteAdminPassword()
      setPasswordStatus(next)
      setPasswordDraft('')
      setPasswordConfirm('')
      setPasswordMessage(copy.passwordDeleted)
    } catch (err) {
      setPasswordError(err instanceof Error ? err.message : String(err))
    } finally {
      setPasswordBusy(false)
    }
  }

  const toggleLoginTotpRequired = async (required: boolean) => {
    setPasswordBusy(true)
    setPasswordError(null)
    setPasswordMessage(null)
    try {
      const next = await setAdminLoginTotpRequired(required)
      setPasswordStatus(next)
      setPasswordMessage(required ? copy.loginTotpRequiredEnabled : copy.loginTotpRequiredDisabled)
    } catch (err) {
      setPasswordError(err instanceof Error ? err.message : String(err))
    } finally {
      setPasswordBusy(false)
    }
  }

  const addPasskey = async () => {
    if (!passkeyConfigured) return
    setPasskeyBusy(true)
    setPasskeysError(null)
    setPasskeysMessage(null)
    try {
      await registerAdminPasskey(newPasskeyLabel.trim() || undefined)
      await loadPasskeys()
      setNewPasskeyLabel('')
    } catch (err) {
      setPasskeysError(err instanceof Error ? err.message : String(err))
    } finally {
      setPasskeyBusy(false)
    }
  }

  const savePasskeyLabel = async (credentialId: string) => {
    setPasskeyBusy(true)
    setPasskeysError(null)
    setPasskeysMessage(null)
    try {
      const next = await updateAdminPasskeyLabel(credentialId, labelDrafts[credentialId] ?? '')
      setPasskeys(next)
      setPasskeysMessage(copy.passkeyUpdated)
    } catch (err) {
      setPasskeysError(err instanceof Error ? err.message : String(err))
    } finally {
      setPasskeyBusy(false)
    }
  }

  const removePasskey = async (credentialId: string) => {
    setPasskeyBusy(true)
    setPasskeysError(null)
    setPasskeysMessage(null)
    try {
      const next = await deleteAdminPasskey(credentialId)
      setPasskeys(next)
      setPasskeysMessage(copy.passkeyDeleted)
    } catch (err) {
      setPasskeysError(err instanceof Error ? err.message : String(err))
    } finally {
      setPasskeyBusy(false)
    }
  }

  const beginTotpSetup = async () => {
    setTotpBusy(true)
    setTotpError(null)
    try {
      const setup = await createAdminTotpSetup()
      setTotpSetup(setup)
      setTotpCode('')
      setTotpCurrentCode('')
    } catch (err) {
      setTotpError(err instanceof Error ? err.message : String(err))
    } finally {
      setTotpBusy(false)
    }
  }

  const confirmTotpSetup = async () => {
    if (!totpSetup) return
    setTotpBusy(true)
    setTotpError(null)
    try {
      const status = await confirmAdminTotp(totpSetup.secret, totpCode)
      setTotpStatus(status)
      setTotpSetup(null)
      setTotpCode('')
      setTotpCurrentCode('')
    } catch (err) {
      setTotpError(err instanceof Error ? err.message : String(err))
    } finally {
      setTotpBusy(false)
    }
  }

  const disableTotpBinding = async () => {
    setTotpBusy(true)
    setTotpError(null)
    try {
      const status = await disableAdminTotp(totpCurrentCode || totpCode)
      setTotpStatus(status)
      setTotpSetup(null)
      setTotpCode('')
      setTotpCurrentCode('')
    } catch (err) {
      setTotpError(err instanceof Error ? err.message : String(err))
    } finally {
      setTotpBusy(false)
    }
  }

  const confirmSecurityAction = async () => {
    if (!pendingSecurityAction) return
    if (pendingSecurityAction.type === 'password-delete') await removePassword()
    if (pendingSecurityAction.type === 'passkey-delete') await removePasskey(pendingSecurityAction.credentialId)
    if (pendingSecurityAction.type === 'totp-disable') await disableTotpBinding()
    closeSecurityAction()
  }

  const securityActionCopy = useMemo(() => {
    if (!pendingSecurityAction) return null
    if (pendingSecurityAction.type === 'password-delete') {
      return {
        title: copy.passwordDeleteDialogTitle,
        description: copy.passwordDeleteDialogDescription,
        confirm: copy.passwordDeleteAction,
      }
    }
    if (pendingSecurityAction.type === 'passkey-delete') {
      return {
        title: copy.passkeyDeleteDialogTitle,
        description: copy.passkeyDeleteDialogDescription,
        confirm: copy.passkeyDelete,
      }
    }
    return {
      title: copy.totpDisableDialogTitle,
      description: copy.totpDisableDialogDescription,
      confirm: strings.form.totpDisableAction,
    }
  }, [
    copy,
    pendingSecurityAction,
    strings.form.totpDisableAction,
  ])

  return (
    <section className="surface panel system-settings-shell">
      <div className="system-settings-form-layout" aria-label={copy.title}>
        <section className="system-settings-config-section">
          <h4>{copy.postureSectionTitle}</h4>
          <div className="grid gap-3">
            <div className="grid gap-2 md:grid-cols-3">
              <div className="rounded-md border border-border/60 bg-muted/20 px-3 py-2">
                <p className="m-0 text-xs font-bold text-muted-foreground">{copy.posturePassword}</p>
                <p className="m-0 text-sm font-semibold text-foreground">{passwordStatusLabel}</p>
              </div>
              <div className="rounded-md border border-border/60 bg-muted/20 px-3 py-2">
                <p className="m-0 text-xs font-bold text-muted-foreground">{copy.posturePasskeys}</p>
                <p className="m-0 text-sm font-semibold text-foreground">
                  {copy.posturePasskeyCount.replace('{count}', String(passkeyCredentialCount))}
                </p>
              </div>
              <div className="rounded-md border border-border/60 bg-muted/20 px-3 py-2">
                <p className="m-0 text-xs font-bold text-muted-foreground">{copy.postureTotp}</p>
                <p className="m-0 text-sm font-semibold text-foreground">{totpStatusLabel}</p>
              </div>
            </div>
            <p className={`m-0 text-xs font-medium ${activeLoginMethods <= 1 ? 'text-amber-700 dark:text-amber-300' : 'text-muted-foreground'}`}>
              {postureWarning}
            </p>
          </div>
        </section>

        <section className="system-settings-config-section">
          <h4>{copy.passwordSectionTitle}</h4>
          <AdminLoadingRegion
            loadState={passwordLoading ? 'initial_loading' : passwordError ? 'error' : 'ready'}
            loadingLabel={copy.passwordLoading}
            errorLabel={passwordError ?? undefined}
            minHeight={160}
          >
            <div className="system-settings-field-grid">
              <div className="system-settings-action-row" aria-labelledby="admin-password-status-title">
                <div className="system-settings-toggle-copy">
                  <span className="system-settings-setting-title" id="admin-password-status-title">
                    {copy.passwordStatusTitle}
                  </span>
                  <p>{copy.passwordDescription}</p>
                  {passwordMessage ? <p className="text-xs text-emerald-600">{passwordMessage}</p> : null}
                </div>
                <span className={`inline-flex rounded-full border px-3 py-1 text-sm font-semibold ${statusToneClass(builtinPasswordEnabled)}`}>
                  {passwordStatusLabel}
                </span>
              </div>
              <div className="system-settings-password-controls">
                <label className="grid gap-2 text-sm font-medium" htmlFor="admin-password-new">
                  <span>{copy.passwordNewLabel}</span>
                  <Input
                    id="admin-password-new"
                    type="password"
                    value={passwordDraft}
                    onChange={(event) => setPasswordDraft(event.target.value)}
                    placeholder={copy.passwordNewPlaceholder}
                    autoComplete="new-password"
                  />
                </label>
                <label className="grid gap-2 text-sm font-medium" htmlFor="admin-password-confirm">
                  <span>{copy.passwordConfirmLabel}</span>
                  <Input
                    id="admin-password-confirm"
                    type="password"
                    value={passwordConfirm}
                    onChange={(event) => setPasswordConfirm(event.target.value)}
                    placeholder={copy.passwordConfirmPlaceholder}
                    autoComplete="new-password"
                  />
                </label>
                <div className="system-settings-password-actions">
                  <Button
                    type="button"
                    disabled={passwordBusy || Boolean(passwordValidationError) || !passwordDraft}
                    onClick={() => void submitPassword()}
                  >
                    <Icon icon="mdi:lock-reset" width={16} height={16} aria-hidden="true" />
                    {copy.passwordSetAction}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    disabled={passwordBusy || !builtinPasswordEnabled}
                    onClick={() => openSecurityAction({ type: 'password-delete' })}
                  >
                    <Icon icon="mdi:lock-off-outline" width={16} height={16} aria-hidden="true" />
                    {copy.passwordDeleteAction}
                  </Button>
                </div>
              </div>
              {passwordValidationError ? <p className="form-error">{passwordValidationError}</p> : null}
            </div>
          </AdminLoadingRegion>
        </section>

        <section className="system-settings-config-section">
          <h4>{copy.passkeySectionTitle}</h4>
          <AdminLoadingRegion
            loadState={passkeysLoading ? 'initial_loading' : passkeysError ? 'error' : 'ready'}
            loadingLabel={copy.passkeyLoading}
            errorLabel={passkeysError ?? undefined}
            minHeight={160}
          >
            <div className="system-settings-field-grid">
              <div className="system-settings-action-row" aria-labelledby="admin-passkey-status-title">
                <div className="system-settings-toggle-copy">
                  <span className="system-settings-setting-title" id="admin-passkey-status-title">
                    {copy.passkeyStatusTitle}
                  </span>
                  <p>{copy.passkeyDescription}</p>
                  <p className="text-xs text-muted-foreground">{passkeySummary}</p>
                  {passkeysMessage ? <p className="text-xs text-emerald-600">{passkeysMessage}</p> : null}
                </div>
                <span className={`inline-flex rounded-full border px-3 py-1 text-sm font-semibold ${statusToneClass(passkeyEnabled)}`}>
                  {passkeyStatusLabel}
                </span>
              </div>
              {passkeys?.credentials.length ? (
                <div className="admin-passkey-list">
                  {passkeys.credentials.map((credential) => {
                    const labelDraft = labelDrafts[credential.credentialId] ?? credential.label ?? ''
                    const unchanged = labelDraft === (credential.label ?? '')
                    return (
                      <div
                        key={credential.credentialId}
                        className="admin-passkey-card rounded-md border border-border/60 bg-muted/20 text-sm"
                      >
                        <div className="admin-passkey-edit-row">
                          <Input
                            className="admin-passkey-note-input"
                            value={labelDraft}
                            onChange={(event) => setLabelDrafts((drafts) => ({
                              ...drafts,
                              [credential.credentialId]: event.target.value,
                            }))}
                            placeholder={copy.passkeyNewPlaceholder}
                            aria-label={copy.passkeyNewLabel}
                          />
                          <Button
                            type="button"
                            variant="outline"
                            className="admin-passkey-note-button"
                            disabled={passkeyBusy || unchanged}
                            onClick={() => void savePasskeyLabel(credential.credentialId)}
                          >
                            {copy.passkeySaveLabel}
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            className="admin-passkey-note-button"
                            disabled={passkeyBusy}
                            aria-label={copy.passkeyDeleteNamed.replace('{label}', credential.label || copy.passkeyDefaultLabel)}
                            onClick={() => openSecurityAction({
                              type: 'passkey-delete',
                              credentialId: credential.credentialId,
                              label: credential.label || copy.passkeyDefaultLabel,
                            })}
                          >
                            {copy.passkeyDelete}
                          </Button>
                        </div>
                        <p className="admin-passkey-meta-row text-sm font-medium text-muted-foreground">
                          <span>
                            {copy.passkeyCreatedAt.replace('{time}', formatTimestamp(credential.createdAt, language))}
                          </span>
                          <span>
                            {copy.passkeyLastUsedAt.replace('{time}', formatTimestamp(credential.lastUsedAt, language))}
                          </span>
                          <span className="font-mono">
                            {credential.credentialId.slice(0, 16)}
                          </span>
                        </p>
                      </div>
                    )
                  })}
                </div>
              ) : (
                <p className="rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-sm text-muted-foreground">
                  {copy.passkeyEmpty}
                </p>
              )}
              <div className="admin-passkey-add-row">
                <label className="sr-only" htmlFor="admin-passkey-new-label">
                  {copy.passkeyNewLabel}
                </label>
                <Input
                  id="admin-passkey-new-label"
                  value={newPasskeyLabel}
                  onChange={(event) => setNewPasskeyLabel(event.target.value)}
                  placeholder={copy.passkeyNewPlaceholder}
                />
                <Button
                  type="button"
                  disabled={passkeyBusy || !passkeyConfigured}
                  onClick={() => void addPasskey()}
                >
                  <Icon icon="mdi:key-plus" width={16} height={16} aria-hidden="true" />
                  {passkeyBusy ? copy.passkeyAdding : copy.passkeyAddAction}
                </Button>
              </div>
            </div>
          </AdminLoadingRegion>
        </section>

        <section className="system-settings-config-section">
          <h4>{strings.form.totpTitle}</h4>
          <div className="system-settings-field-grid">
            <div className="system-settings-action-row system-settings-totp-row" aria-labelledby="system-settings-totp-title">
              <div className="system-settings-toggle-copy">
                <span className="system-settings-setting-title" id="system-settings-totp-title">
                  {strings.form.totpTitle}
                </span>
                <p>
                  {totpStatus?.enabled
                    ? strings.form.totpBoundHint
                    : strings.form.totpUnboundHint}
                </p>
                {totpStatus?.missingCryptoKey && <p className="form-error">{strings.form.totpMissingCryptoKey}</p>}
                {totpError && <p className="form-error">{totpError}</p>}
                {totpSetup && (
                  <div className="system-settings-totp-setup">
                    <img
                      src={`data:image/png;base64,${totpSetup.qrPngBase64}`}
                      alt={strings.form.totpQrAlt}
                      className="system-settings-totp-qr"
                    />
                    <Input value={totpSetup.secret} readOnly aria-label={strings.form.totpSetupSecretLabel} />
                    <TotpCodeInput
                      id="admin-totp-bind-code"
                      name="admin_totp_bind_code"
                      value={totpCode}
                      onChange={setTotpCode}
                      ariaLabel={strings.form.totpConfirmCodePlaceholder}
                    />
                  </div>
                )}
              </div>
              <div className="system-settings-totp-actions">
                {!totpSetup && !totpStatus?.enabled && (
                  <Button
                    type="button"
                    variant="outline"
                    disabled={totpBusy || !totpStatus?.available}
                    onClick={() => void beginTotpSetup()}
                  >
                    {strings.form.totpBindAction}
                  </Button>
                )}
                {totpSetup && (
                  <Button
                    type="button"
                    disabled={totpBusy || totpCode.length !== 6 || (totpStatus?.enabled && totpCurrentCode.length !== 6)}
                    onClick={() => void confirmTotpSetup()}
                  >
                    {strings.form.totpConfirmAction}
                  </Button>
                )}
                {totpStatus?.enabled && !totpSetup && (
                  <>
                    <TotpCodeInput
                      id="admin-totp-disable-code"
                      name="admin_totp_disable_code"
                      value={totpCurrentCode}
                      onChange={setTotpCurrentCode}
                      ariaLabel={strings.form.totpCurrentCodePlaceholder}
                    />
                    <Button
                      type="button"
                      variant="outline"
                      disabled={totpBusy || totpCurrentCode.length !== 6}
                      onClick={() => openSecurityAction({ type: 'totp-disable' })}
                    >
                      {strings.form.totpDisableAction}
                    </Button>
                  </>
                )}
              </div>
            </div>
            <div className="system-settings-toggle-row">
              <div className="system-settings-toggle-copy">
                <label htmlFor="admin-login-totp-required">{copy.loginTotpRequiredTitle}</label>
                <p>{totpEnabled ? copy.loginTotpRequiredDescription : copy.loginTotpRequiredUnavailable}</p>
              </div>
              <Switch
                id="admin-login-totp-required"
                checked={loginTotpRequired}
                disabled={passwordBusy || !totpEnabled}
                aria-label={copy.loginTotpRequiredTitle}
                onCheckedChange={(checked) => void toggleLoginTotpRequired(checked)}
              />
            </div>
          </div>
        </section>
      </div>
      <Dialog open={pendingSecurityAction != null} onOpenChange={(open) => {
        if (!open) closeSecurityAction()
      }}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>{securityActionCopy?.title}</DialogTitle>
            <DialogDescription>{securityActionCopy?.description}</DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2">
            <Button type="button" variant="outline" onClick={closeSecurityAction}>
              {copy.securityCancel}
            </Button>
            <Button
              type="button"
              variant="destructive"
              disabled={passwordBusy || passkeyBusy || totpBusy}
              onClick={() => void confirmSecurityAction()}
            >
              {securityActionCopy?.confirm}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  )
}
