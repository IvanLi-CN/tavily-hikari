import type { ReactNode } from 'react'

import type { QueryLoadState } from '../admin/queryLoadState'
import { isBlockingLoadState, isRefreshingLoadState } from '../admin/queryLoadState'
import { cn } from '../lib/utils'

interface AdminLoadingRegionProps {
  children?: ReactNode
  className?: string
  loadState?: QueryLoadState
  loadingLabel?: ReactNode
  errorLabel?: ReactNode
  minHeight?: number | string
  skeletonRows?: number
}

export default function AdminLoadingRegion({
  children,
  className,
  loadState = 'ready',
  loadingLabel = 'Loading…',
  errorLabel,
  minHeight = 220,
  skeletonRows = 4,
}: AdminLoadingRegionProps): JSX.Element {
  const blocking = isBlockingLoadState(loadState)
  const refreshing = isRefreshingLoadState(loadState)
  const errored = loadState === 'error'
  const ariaBusy = blocking || refreshing

  return (
    <div
      className={cn(
        'admin-loading-region',
        blocking && 'admin-loading-region-blocking',
        refreshing && 'admin-loading-region-refreshing',
        className,
      )}
      aria-busy={ariaBusy ? true : undefined}
    >
      {blocking ? (
        <div className="admin-loading-region-placeholder" style={{ minHeight }}>
          <div className="admin-loading-region-skeleton" aria-hidden="true">
            {Array.from({ length: skeletonRows }, (_, index) => (
              <span
                key={`admin-loading-skeleton-${index}`}
                className="admin-loading-region-skeleton-row"
                style={{ width: `${Math.max(42, 100 - index * 11)}%` }}
              />
            ))}
          </div>
          <div className="admin-loading-region-label">{loadingLabel}</div>
        </div>
      ) : errored && errorLabel ? (
        <div className="admin-loading-region-error empty-state alert" role="alert">
          {errorLabel}
        </div>
      ) : (
        <>
          {refreshing && (
            <div className="admin-loading-region-indicator" aria-live="polite">
              {loadingLabel}
            </div>
          )}
          {children}
        </>
      )}
    </div>
  )
}
