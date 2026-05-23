import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'

export type UserTagBindingOption = {
  id: string
  displayName: string
}

type UserTagBindingControlsProps = {
  bindableTags: UserTagBindingOption[]
  buttonLabel: string
  buttonBusyLabel?: string
  disabled?: boolean
  emptyLabel: string
  onBind: () => void
  onSelectedTagIdChange: (value: string) => void
  placeholder: string
  selectedTagId: string
}

export function UserTagBindingControls({
  bindableTags,
  buttonBusyLabel,
  buttonLabel,
  disabled,
  emptyLabel,
  onBind,
  onSelectedTagIdChange,
  placeholder,
  selectedTagId,
}: UserTagBindingControlsProps): JSX.Element {
  const isBusy = disabled ?? false

  return (
    <div className="user-tag-bind-controls">
      <Select value={selectedTagId} onValueChange={onSelectedTagIdChange} disabled={isBusy}>
        <SelectTrigger className="user-tag-bind-select" aria-label={placeholder}>
          <SelectValue placeholder={placeholder} />
        </SelectTrigger>
        <SelectContent align="start">
          {bindableTags.length === 0 ? (
            <SelectItem value="__no_bindable_user_tags__" disabled>
              {emptyLabel}
            </SelectItem>
          ) : (
            bindableTags.map((tag) => (
              <SelectItem key={tag.id} value={tag.id}>
                {tag.displayName}
              </SelectItem>
            ))
          )}
        </SelectContent>
      </Select>
      <button
        type="button"
        className="btn btn-primary"
        onClick={() => void onBind()}
        disabled={isBusy || !selectedTagId}
      >
        {isBusy ? buttonBusyLabel ?? buttonLabel : buttonLabel}
      </button>
    </div>
  )
}
