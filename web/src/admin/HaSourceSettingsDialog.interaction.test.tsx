import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot } from 'react-dom/client'

import type { HaSourceSettingsApiError, HaStatus } from '../api'
import { translations } from '../i18n'
import HaSourceSettingsDialog from './HaSourceSettingsDialog'

const strings = translations.zh.admin.systemSettings.ha

const baseStatus: HaStatus = {
  mode: 'active_standby',
  nodeId: 'node-b',
  nodePublicOrigin: '203.0.113.10:58087',
  role: 'full_master',
  degraded: false,
  allowsBasicBusiness: true,
  allowsFullWrites: true,
  edgeoneDomain: 'api.example.com',
  edgeoneOrigin: '203.0.113.9:58087',
  edgeoneExpectedOrigin: '203.0.113.9:58087',
  edgeoneCurrentTarget: '203.0.113.9:58087',
  edgeoneExpectedTarget: '203.0.113.9:58087',
  edgeoneCurrentSourceKind: 'direct',
  edgeoneExpectedSourceKind: 'direct',
  edgeoneCurrentOriginGroupId: null,
  edgeoneExpectedOriginGroupId: null,
  haSourceDefaults: {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: '203.0.113.9',
    directOriginPort: 58087,
    originGroupId: null,
    target: '203.0.113.9:58087',
  },
  haSourceOverride: null,
  haSourceEffective: {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: '203.0.113.9',
    directOriginPort: 58087,
    originGroupId: null,
    target: '203.0.113.9:58087',
  },
  edgeoneApiConfigured: true,
  lastEdgeoneCheckAt: 1_700_000_000,
  lastSyncAt: 1_700_000_002,
  syncLagSeconds: 8,
  recoveryStatus: null,
  message: 'node is serving as active master',
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  })
}

afterEach(() => {
  document.body.innerHTML = ''
})

function getPortalText(): string {
  return document.body.textContent ?? ''
}

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('HaSourceSettingsDialog interactions', () => {
  it('shows field-level validation without rendering a submit failure alert', async () => {
    let submitCalled = false
    const invalidDirectStatus: HaStatus = {
      ...baseStatus,
      haSourceDefaults: {
        ...baseStatus.haSourceDefaults!,
        directOriginHost: '',
        target: ':58087',
      },
      haSourceEffective: {
        ...baseStatus.haSourceEffective!,
        directOriginHost: '',
        target: ':58087',
      },
    }

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        <HaSourceSettingsDialog
          open
          status={invalidDirectStatus}
          strings={strings}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
          submitSourceSettings={async () => {
            submitCalled = true
            return baseStatus
          }}
        />,
      )
    })
    await flushEffects()

    const saveButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceSave,
    )
    expect(saveButton).not.toBeNull()

    await act(async () => {
      saveButton!.click()
    })
    await flushEffects()

    const hostInput = document.querySelector<HTMLInputElement>('input[placeholder="203.0.113.9"]')
    expect(hostInput).not.toBeNull()
    expect(document.activeElement).toBe(hostInput)
    expect(hostInput?.getAttribute('aria-describedby')).toBe('ha-source-direct-host-error')
    expect(getPortalText()).toContain(strings.sourceInvalidDirectHost)
    expect(getPortalText()).not.toContain(strings.sourceSaveFailedTitle)
    expect(document.body.querySelector('.alert.alert-error')).toBeNull()
    expect(submitCalled).toBe(false)

    await act(async () => root.unmount())
  })

  it('renders a destructive alert with collapsible technical details on submit failure', async () => {
    const submitSourceSettings = async (): Promise<HaStatus> => {
      const error = new Error('Failed to deserialize the JSON body into the target type') as HaSourceSettingsApiError
      error.status = 400
      error.rawDetail =
        'Failed to deserialize the JSON body into the target type: directOriginScheme: unknown variant `https`'
      throw error
    }

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        <HaSourceSettingsDialog
          open
          status={baseStatus}
          strings={strings}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
          submitSourceSettings={submitSourceSettings}
        />,
      )
    })
    await flushEffects()

    const applyButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceSaveAndApply,
    )
    expect(applyButton).not.toBeNull()

    await act(async () => {
      applyButton!.click()
    })
    await flushEffects()

    const alert = document.body.querySelector('.alert.alert-error')
    expect(alert).not.toBeNull()
    expect(document.activeElement).toBe(alert)
    expect(alert?.textContent).toContain(strings.sourceApplyFailedTitle)
    expect(alert?.textContent).toContain(strings.sourceSubmitFailedDirectDescription)
    expect(alert?.textContent).toContain(strings.sourceTechnicalDetailsLabel)
    expect(alert?.textContent).not.toContain('unknown variant `https`')

    const details = alert?.querySelector('details')
    expect(details).not.toBeNull()
    expect(details?.open).toBe(false)

    await act(async () => {
      details?.setAttribute('open', '')
      details?.dispatchEvent(new Event('toggle'))
    })
    await flushEffects()

    expect(details?.open).toBe(true)
    expect(alert?.textContent).toContain('unknown variant `https`')

    await act(async () => root.unmount())
  })

  it('clears the remote submit failure after the operator changes the form', async () => {
    let callCount = 0
    const submitSourceSettings = async (): Promise<HaStatus> => {
      callCount += 1
      const error = new Error('Failed to save source') as HaSourceSettingsApiError
      error.status = 400
      error.rawDetail = 'originGroupId is invalid'
      throw error
    }

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        <HaSourceSettingsDialog
          open
          status={{
            ...baseStatus,
            haSourceDefaults: {
              ...baseStatus.haSourceDefaults!,
              sourceKind: 'origin_group',
              directOriginScheme: null,
              directOriginHost: null,
              directOriginPort: null,
              originGroupId: 'eo-group-1',
              target: 'eo-group-1',
            },
            haSourceEffective: {
              ...baseStatus.haSourceEffective!,
              sourceKind: 'origin_group',
              directOriginScheme: null,
              directOriginHost: null,
              directOriginPort: null,
              originGroupId: 'eo-group-1',
              target: 'eo-group-1',
            },
          }}
          strings={strings}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
          submitSourceSettings={submitSourceSettings}
        />,
      )
    })
    await flushEffects()

    const saveButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceSave,
    )
    expect(saveButton).not.toBeNull()

    await act(async () => {
      saveButton!.click()
    })
    await flushEffects()

    expect(callCount).toBe(1)
    expect(getPortalText()).toContain(strings.sourceSaveFailedTitle)
    expect(getPortalText()).toContain(strings.sourceSubmitFailedOriginGroupDescription)

    const directSourceToggle = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceKindDirect,
    )
    expect(directSourceToggle).not.toBeNull()

    await act(async () => {
      directSourceToggle!.click()
    })
    await flushEffects()

    expect(getPortalText()).not.toContain(strings.sourceSaveFailedTitle)

    await act(async () => root.unmount())
  })

  it('freezes segmented controls while a submission is in flight', async () => {
    const deferred = createDeferred<HaStatus>()
    const submitSourceSettings = async (): Promise<HaStatus> => deferred.promise

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        <HaSourceSettingsDialog
          open
          status={baseStatus}
          strings={strings}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
          submitSourceSettings={submitSourceSettings}
        />,
      )
    })
    await flushEffects()

    const applyButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceSaveAndApply,
    )
    expect(applyButton).not.toBeNull()

    await act(async () => {
      applyButton!.click()
    })
    await flushEffects()

    const directSourceToggle = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceKindDirect,
    )
    expect(directSourceToggle?.disabled).toBe(true)

    const schemeTrigger = Array.from(document.querySelectorAll<HTMLElement>('[role="combobox"]')).find(
      (trigger) => trigger.textContent?.includes('HTTPS'),
    )
    expect(schemeTrigger?.getAttribute('data-disabled')).toBe('')
    expect(applyButton?.disabled).toBe(true)

    await act(async () => {
      deferred.resolve(baseStatus)
      await deferred.promise
    })
    await flushEffects()

    await act(async () => root.unmount())
  })

  it('keeps the failure copy tied to the submitted source kind until the operator edits again', async () => {
    const deferred = createDeferred<HaStatus>()
    let capturedPayload: { sourceKind: string } | null = null
    const submitSourceSettings = async (payload: { sourceKind: string }): Promise<HaStatus> => {
      capturedPayload = payload
      return deferred.promise
    }

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        <HaSourceSettingsDialog
          open
          status={{
            ...baseStatus,
            haSourceDefaults: {
              ...baseStatus.haSourceDefaults!,
              sourceKind: 'origin_group',
              directOriginScheme: null,
              directOriginHost: null,
              directOriginPort: null,
              originGroupId: 'eo-group-1',
              target: 'eo-group-1',
            },
            haSourceEffective: {
              ...baseStatus.haSourceEffective!,
              sourceKind: 'origin_group',
              directOriginScheme: null,
              directOriginHost: null,
              directOriginPort: null,
              originGroupId: 'eo-group-1',
              target: 'eo-group-1',
            },
          }}
          strings={strings}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
          submitSourceSettings={submitSourceSettings as never}
        />,
      )
    })
    await flushEffects()

    const saveButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === strings.sourceSave,
    )
    expect(saveButton).not.toBeNull()

    await act(async () => {
      saveButton!.click()
    })
    await flushEffects()

    expect(capturedPayload?.sourceKind).toBe('origin_group')

    await act(async () => {
      deferred.reject(Object.assign(new Error('Failed to save source'), { status: 400, rawDetail: 'originGroupId is invalid' }))
      try {
        await deferred.promise
      } catch {}
    })
    await flushEffects()

    expect(getPortalText()).toContain(strings.sourceSubmitFailedOriginGroupDescription)

    await act(async () => root.unmount())
  })
})
