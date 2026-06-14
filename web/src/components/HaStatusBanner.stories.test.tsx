import '../../test/happydom'

import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as stories from './HaStatusBanner.stories'
import { LanguageProvider, translations } from '../i18n'
import { ThemeProvider } from '../theme'

describe('HaStatusBanner Storybook proofs', () => {
  it('keeps the HA node list story and local promote entry available', () => {
    expect(meta).toMatchObject({
      title: 'Components/HaStatusBanner',
    })
    expect(stories.NodeListGallery).toBeDefined()
    expect(stories.StandbyAdmin).toBeDefined()
  })

  it('renders service node rows with the master switch action', () => {
    const renderStory = meta.render as ((args: typeof stories.StandbyAdmin.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.StandbyAdmin.args ?? {}),
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.panelTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.nodeInventoryTitle)
    expect(markup).toContain('node-b')
    expect(markup).toContain('configured-peer')
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.promoteToMaster)
  })

  it('renders compact admin attention without node inventory actions', () => {
    const renderStory = meta.render as ((args: typeof stories.StandbyAdmin.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.StandbyAdmin.args ?? {}),
            adminVariant: 'compact',
            compactHref: '/admin/system-settings/ha',
            compactTitle: translations.zh.admin.systemSettings.ha.compactTitle,
            compactDescription: translations.zh.admin.systemSettings.ha.compactDescription,
            compactActionLabel: translations.zh.admin.systemSettings.ha.viewSettings,
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.compactTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.viewSettings)
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.nodeInventoryTitle)
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.promoteToMaster)
  })

  it('keeps the origin group source settings dialog story available', () => {
    expect(stories.OriginGroupSourceDialog).toBeDefined()
    expect(stories.OriginGroupSourceDialog.render).toBeDefined()

    const renderStory = stories.OriginGroupSourceDialog.render as (() => JSX.Element) | undefined
    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, renderStory?.()),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.configureSource)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.sourceKindOriginGroup)
  })

  it('keeps the direct source settings dialog story available', () => {
    expect(stories.DirectSourceDialog).toBeDefined()
    expect(stories.DirectSourceDialog.render).toBeDefined()

    const renderStory = stories.DirectSourceDialog.render as (() => JSX.Element) | undefined
    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, renderStory?.()),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.sourceKindDirect)
    expect(markup).toContain('203.0.113.9:58087')
  })

  it('keeps the direct source selection summary in the story play proof', async () => {
    expect(stories.DirectSourceDialog.play).toBeDefined()

    await stories.DirectSourceDialog.play?.({
      canvasElement: {
        textContent: [
          translations.zh.admin.systemSettings.ha.configureSource,
          translations.zh.admin.systemSettings.ha.sourceKindDirect,
          translations.zh.admin.systemSettings.ha.sourceSelectedDirectLabel,
          'HTTPS · 203.0.113.9:58087',
        ].join(' '),
      } as HTMLElement,
    })
  })

  it('keeps the standby source dialog save-only contract in the story play proof', async () => {
    expect(stories.StandbySourceDialog.play).toBeDefined()

    await stories.StandbySourceDialog.play?.({
      canvasElement: {
        textContent: [
          translations.zh.admin.systemSettings.ha.configureSource,
          translations.zh.admin.systemSettings.ha.roleStandby,
          translations.zh.admin.systemSettings.ha.sourceSave,
        ].join(' '),
      } as HTMLElement,
    })

    await expect(
      stories.StandbySourceDialog.play?.({
        canvasElement: {
          textContent: [
            translations.zh.admin.systemSettings.ha.configureSource,
            translations.zh.admin.systemSettings.ha.roleStandby,
            translations.zh.admin.systemSettings.ha.sourceSave,
            translations.zh.admin.systemSettings.ha.sourceSaveAndApply,
          ].join(' '),
        } as HTMLElement,
      }),
    ).rejects.toThrow('Expected standby HA source dialog to omit the EdgeOne switch action.')
  })
})
