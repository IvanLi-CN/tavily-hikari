import { Icon } from '@iconify/react'
import type { InputHTMLAttributes } from 'react'

import { cn } from '../lib/utils'
import { Button } from './ui/button'
import { Input } from './ui/input'

export type TokenSecretCopyState = 'idle' | 'copied' | 'error'

interface TokenSecretFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'value' | 'onChange'> {
  inputId: string
  label: string
  value: string
  visible: boolean
  copyState: TokenSecretCopyState
  onValueChange: (value: string) => void
  onToggleVisibility: () => void
  onCopy: () => void | Promise<void>
  visibilityShowLabel: string
  visibilityHideLabel: string
  visibilityIconAlt: string
  copyAriaLabel: string
  copyLabel: string
  copiedLabel: string
  copyErrorLabel: string
  wrapperClassName?: string
  rowClassName?: string
  shellClassName?: string
  inputClassName?: string
  copyButtonClassName?: string
  copyDisabled?: boolean
}

export default function TokenSecretField({
  inputId,
  label,
  value,
  visible,
  copyState,
  onValueChange,
  onToggleVisibility,
  onCopy,
  visibilityShowLabel,
  visibilityHideLabel,
  visibilityIconAlt,
  copyAriaLabel,
  copyLabel,
  copiedLabel,
  copyErrorLabel,
  wrapperClassName,
  rowClassName,
  shellClassName,
  inputClassName,
  copyButtonClassName,
  copyDisabled = false,
  className,
  onBlur,
  ...inputProps
}: TokenSecretFieldProps): JSX.Element {
  const copyVariant = copyState === 'copied' ? 'success' : copyState === 'error' ? 'warning' : 'outline'
  const copyStateClassName =
    copyState === 'copied'
      ? 'token-copy-button-success'
      : copyState === 'error'
        ? 'token-copy-button-warning'
        : 'token-copy-button-outline'
  const copyIcon =
    copyState === 'copied'
      ? 'mdi:check'
      : copyState === 'error'
        ? 'mdi:alert-circle-outline'
        : 'mdi:content-copy'
  const copyText = copyState === 'copied' ? copiedLabel : copyState === 'error' ? copyErrorLabel : copyLabel

  return (
    <div className={cn('token-input-wrapper', wrapperClassName)}>
      <label htmlFor={inputId} className="token-label">
        {label}
      </label>
      <div className={cn('token-input-row', rowClassName)}>
        <div className={cn('token-input-shell', shellClassName)}>
          <Input
            {...inputProps}
            id={inputId}
            className={cn('token-input', !visible && 'masked', inputClassName, className)}
            type="text"
            value={value}
            onChange={(event) => onValueChange(event.target.value)}
            onBlur={onBlur}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="token-visibility-button h-8 w-8 rounded-md p-1 shadow-none"
            onClick={onToggleVisibility}
            aria-label={visible ? visibilityHideLabel : visibilityShowLabel}
          >
            <img
              src={`https://api.iconify.design/mdi/${visible ? 'eye-off-outline' : 'eye-outline'}.svg?color=%236b7280`}
              alt={visibilityIconAlt}
            />
          </Button>
        </div>
        <Button
          type="button"
          variant={copyVariant}
          className={cn('token-copy-button', copyStateClassName, copyButtonClassName)}
          onClick={() => void onCopy()}
          aria-label={copyAriaLabel}
          disabled={copyDisabled}
        >
          <Icon icon={copyIcon} aria-hidden="true" className="token-copy-icon" />
          <span>{copyText}</span>
        </Button>
      </div>
    </div>
  )
}
