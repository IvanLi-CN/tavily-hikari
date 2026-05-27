interface MarkdownEditorStorybookProps {
  id?: string
  name?: string
  value: string
  placeholder: string
  ariaLabel?: string
  ariaLabelledBy?: string
  ariaDescribedBy?: string
  disabled?: boolean
  readOnly?: boolean
  className?: string
  onChange: (value: string) => void
}

export default function MarkdownEditorStorybook({
  id,
  name,
  value,
  placeholder,
  ariaLabel,
  ariaLabelledBy,
  ariaDescribedBy,
  disabled = false,
  readOnly = false,
  className,
  onChange,
}: MarkdownEditorStorybookProps): JSX.Element {
  return (
    <div
      className={[
        'markdown-editor-shell markdown-editor-shell--storybook',
        readOnly ? 'markdown-editor-shell--readonly' : '',
        className ?? '',
      ].filter(Boolean).join(' ')}
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
      aria-describedby={ariaDescribedBy}
    >
      <textarea
        id={id}
        name={name}
        className="textarea markdown-editor-storybook-input"
        value={value}
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
        aria-describedby={ariaDescribedBy}
        placeholder={placeholder}
        rows={7}
        maxLength={4000}
        disabled={disabled}
        readOnly={readOnly}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  )
}
