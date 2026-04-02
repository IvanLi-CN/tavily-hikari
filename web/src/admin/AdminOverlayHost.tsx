import type { ReactNode } from 'react'

export const ADMIN_OVERLAY_HOST_TEST_ID = 'admin-overlay-host'

export default function AdminOverlayHost({
  children,
  overlays,
}: {
  children: ReactNode
  overlays?: ReactNode
}): JSX.Element {
  return (
    <>
      {children}
      <div data-admin-overlay-host={ADMIN_OVERLAY_HOST_TEST_ID} style={{ display: 'contents' }}>
        {overlays}
      </div>
    </>
  )
}
