import '../../test/happydom'

import { describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as stories from './HaStatusBanner.stories'
import { LanguageProvider, translations } from '../i18n'
import { ThemeProvider } from '../theme'

async function renderIntoDom(element: JSX.Element): Promise<string> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  await act(async () => {
    root.render(createElement(LanguageProvider, { initialLanguage: 'zh' }, createElement(ThemeProvider, null, element)))
  })
  const text = container.textContent ?? ''
  await act(async () => {
    root.unmount()
  })
  container.remove()
  return text
}

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
  })

  it('renders the origin group source settings dialog story in the browser runtime', async () => {
    const renderStory = stories.OriginGroupSourceDialog.render as (() => JSX.Element) | undefined
    const text = await renderIntoDom(renderStory?.() ?? <></>)

    expect(text).toContain(translations.zh.admin.systemSettings.ha.configureSource)
    expect(text).toContain(translations.zh.admin.systemSettings.ha.sourceKindOriginGroup)
  })

  it('keeps the direct source settings dialog story available', () => {
    expect(stories.DirectSourceDialog).toBeDefined()
    expect(stories.DirectSourceDialog.render).toBeDefined()
  })

  it('renders the direct source settings dialog story in the browser runtime', async () => {
    const renderStory = stories.DirectSourceDialog.render as (() => JSX.Element) | undefined
    const text = await renderIntoDom(renderStory?.() ?? <></>)

    expect(text).toContain(translations.zh.admin.systemSettings.ha.sourceKindDirect)
    expect(text).toContain('203.0.113.9:58087')
  })

  it('keeps the submit-failure source dialog story available', () => {
    expect(stories.SourceDialogSubmitFailure).toBeDefined()
    expect(stories.SourceDialogSubmitFailure.render).toBeDefined()
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
