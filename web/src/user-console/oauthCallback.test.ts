import { describe, expect, it } from 'bun:test'

import {
  parseOAuthCallbackQuery,
  resolveOAuthCallbackPanelModel,
  resolveOAuthCallbackProviderLabel,
} from './oauthCallback'
import { EN } from './text'

describe('oauthCallback helpers', () => {
  it('parses provider callback query params and normalizes empty values', () => {
    expect(parseOAuthCallbackQuery('?code=abc&state=xyz')).toEqual({
      code: 'abc',
      state: 'xyz',
      error: null,
      errorDescription: null,
    })

    expect(parseOAuthCallbackQuery('?error=access_denied&error_description=User%20cancelled&state=')).toEqual({
      code: null,
      state: null,
      error: 'access_denied',
      errorDescription: 'User cancelled',
    })
  })

  it('builds friendly callback models for connecting, timeout, and success states', () => {
    const connecting = resolveOAuthCallbackPanelModel({
      state: 'connecting',
      providerLabel: 'LinuxDo',
      text: EN.oauthCallback,
    })
    expect(connecting).toMatchObject({
      tone: 'info',
      busy: true,
      showActions: false,
      title: 'Talking to LinuxDo',
    })

    const timeout = resolveOAuthCallbackPanelModel({
      state: 'timeout',
      providerLabel: 'LinuxDo',
      text: EN.oauthCallback,
    })
    expect(timeout).toMatchObject({
      tone: 'warning',
      busy: false,
      showActions: true,
      title: 'The connection took too long',
    })

    const success = resolveOAuthCallbackPanelModel({
      state: 'success',
      providerLabel: 'LinuxDo',
      text: EN.oauthCallback,
    })
    expect(success).toMatchObject({
      tone: 'success',
      busy: false,
      showActions: false,
      title: 'LinuxDo connected',
    })
  })

  it('keeps unknown providers readable while preserving the LinuxDo label mapping', () => {
    expect(resolveOAuthCallbackProviderLabel('linuxdo', { linuxdo: 'LinuxDo' })).toBe('LinuxDo')
    expect(resolveOAuthCallbackProviderLabel('future-provider', { linuxdo: 'LinuxDo' })).toBe('future-provider')
  })
})
