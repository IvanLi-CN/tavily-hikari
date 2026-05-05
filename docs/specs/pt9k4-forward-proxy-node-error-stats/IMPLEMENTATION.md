# Forward Proxy Node Error Statistics Implementation

## Backend

- Add a persistent forward proxy node override table keyed by `proxy_key`.
- Load disabled overrides at startup and apply them to the in-memory forward proxy manager.
- Filter disabled nodes out of real request routing candidates.
- Add admin endpoints for node error statistics and bulk enablement changes.
- Aggregate error statistics from `forward_proxy_attempts` with `is_probe = 0`.

## Frontend

- Extend the admin forward proxy module with a node pool/error statistics switcher.
- Add error statistics table rendering with compact activity and pie charts.
- Add shared row selection state and a floating bulk action bar for both views.
- Refresh settings, live stats, and error stats after successful bulk enablement updates.

## Evidence

- Storybook stories provide stable mock states for visual review.
- Local validation covers backend tests, frontend build, and Storybook visual screenshots.
