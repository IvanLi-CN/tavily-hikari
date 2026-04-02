import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import AdminOverlayHost, { ADMIN_OVERLAY_HOST_TEST_ID } from './AdminOverlayHost'

describe('AdminOverlayHost', () => {
  it('keeps a stable shared overlay mount next to route content', () => {
    const html = renderToStaticMarkup(
      <AdminOverlayHost overlays={<div data-overlay-probe="monthly-broken-drawer">overlay</div>}>
        <section data-route-shell="user-usage">page</section>
      </AdminOverlayHost>,
    )

    expect(html).toContain('data-route-shell="user-usage"')
    expect(html).toContain(`data-admin-overlay-host="${ADMIN_OVERLAY_HOST_TEST_ID}"`)
    expect(html).toContain('data-overlay-probe="monthly-broken-drawer"')
    expect(html.indexOf('data-route-shell="user-usage"')).toBeLessThan(
      html.indexOf(`data-admin-overlay-host="${ADMIN_OVERLAY_HOST_TEST_ID}"`),
    )
  })
})
