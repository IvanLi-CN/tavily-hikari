import type { CSSProperties, ReactNode } from 'react'

import { Input } from './ui/input'

interface QuotaRangeFieldProps {
  label: string
  sliderName: string
  sliderMin: number
  sliderMax: number
  sliderValue: number
  sliderAriaLabel: string
  helperText: ReactNode
  sliderStyle?: CSSProperties
  onSliderChange: (value: number) => void
  inputName: string
  inputValue: string
  inputAriaLabel: string
  onInputChange: (value: string) => void
}

export default function QuotaRangeField({
  label,
  sliderName,
  sliderMin,
  sliderMax,
  sliderValue,
  sliderAriaLabel,
  helperText,
  sliderStyle,
  onSliderChange,
  inputName,
  inputValue,
  inputAriaLabel,
  onInputChange,
}: QuotaRangeFieldProps): JSX.Element {
  return (
    <label className="form-control quota-control">
      <span className="label-text">{label}</span>
      <div className="quota-control-row">
        <div className="quota-slider-wrap">
          <input
            type="range"
            name={sliderName}
            min={sliderMin}
            max={sliderMax}
            step="any"
            className="range quota-slider"
            value={sliderValue}
            onChange={(event) => onSliderChange(Number.parseFloat(event.target.value))}
            style={sliderStyle}
            aria-label={sliderAriaLabel}
          />
          <span className="panel-description">{helperText}</span>
        </div>
        <Input
          type="text"
          name={inputName}
          inputMode="numeric"
          autoComplete="off"
          size={10}
          className="quota-input shrink-0"
          value={inputValue}
          onChange={(event) => onInputChange(event.target.value)}
          aria-label={inputAriaLabel}
        />
      </div>
    </label>
  )
}
