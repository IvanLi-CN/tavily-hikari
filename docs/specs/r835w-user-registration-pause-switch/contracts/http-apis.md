# HTTP / Page Contracts

## GET `/api/admin/registration`

- Auth: admin only
- Response `200`
  - `allowRegistration: boolean`
- Error
  - `403` when request is not admin
  - `500` when persisted setting cannot be read

## PATCH `/api/admin/registration`

- Auth: admin only
- Body
  - `allowRegistration: boolean`
- Response `200`
  - `allowRegistration: boolean`
- Error
  - `400` when body is invalid
  - `403` when request is not admin
  - `500` when persisted setting cannot be written

## GET `/api/profile`

- Auth: none
- Change: modify (backward-compatible field addition)
- New field
  - `allowRegistration: boolean`

## GET `/auth/linuxdo/callback`

- Auth: none
- Change: modify
- New behavior
  - when `allowRegistration=false` and no existing local binding matches `(provider=linuxdo, provider_user_id)`:
    - return `307` with `Location: /registration-paused`
    - clear OAuth binding cookie
    - do not create/update local user session, oauth binding, or user-token binding
- Existing behavior retained
  - existing local LinuxDo users still complete login and receive normal redirect/session handling

## GET `/registration-paused`

- Auth: none
- Response
  - `200` public HTML page rendered from dedicated SPA entry
- UX requirements
  - explain that new registration is paused
  - clarify that existing registered users may still use Linux DO login from home page
  - provide CTA back to `/`
