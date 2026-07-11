---
title: PWA update activation lifecycle
module: tavily-hikari-web
problem_type: service_worker_update_stall
component: pwa-runtime
tags:
  - pwa
  - service-worker
  - frontend
  - state-machine
status: active
related_specs:
  - docs/specs/2br7z-web-pwa-split-identities-offline-shells/SPEC.md
---

# PWA update activation lifecycle

## Context

A page can send an activation message to a waiting service worker without ever receiving the
`controllerchange` event it expects. Treating message delivery as completion leaves the UI in an
unbounded activating state when the worker becomes redundant, the message channel fails, or the
browser does not hand control to the current page.

Multiple registrations add another trap. A page may be controlled by a root-scope public worker
while it installs a more-specific admin worker for the first time. The presence of any controller
does not prove that the admin registration is performing an update.

## Resolution

- Treat `postMessage` as a request, not an acknowledgement of activation.
- Give every user-triggered activation a bounded watchdog and a retryable failure state.
- Accept both `controllerchange` and the target worker reaching `activated` as success signals.
- Guard reload so concurrent success signals can trigger at most one navigation.
- Treat `redundant` and synchronous message delivery errors as explicit failures.
- Determine first install versus update from the target registration's own `active` worker, not
  from `navigator.serviceWorker.controller`.
- Activate a first-install waiting worker silently. If another registration currently controls the
  page, expect the new registration to take over on the next in-scope navigation rather than
  requiring an immediate controller swap.

## Guardrails

- Do not auto-reload after an activation timeout; the user may have unsaved work and reload may not
  repair a worker that never activated.
- Do not hide an activation failure by returning silently to the ready state. Show a non-blocking,
  non-modal status with retry and dismiss actions.
- Test the state machine with deterministic worker mocks, then retain one real-browser two-release
  scenario that changes the service worker script on the same origin and proves controller/cache
  takeover.
