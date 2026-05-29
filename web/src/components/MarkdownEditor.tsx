import { useEffect, useRef, useState } from 'react'
import { Crepe, CrepeFeature } from '@milkdown/crepe'
import { replaceAll } from '@milkdown/kit/utils'

import { Textarea } from './ui/textarea'
import { cn } from '../lib/utils'

interface MarkdownEditorProps {
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

function keepToolbarInsideShell(shell: HTMLElement): () => void {
  let frame: number | null = null

  const update = () => {
    frame = null

    const toolbar = shell.querySelector<HTMLElement>('.milkdown-toolbar[data-show="true"]')
    const milkdown = shell.querySelector<HTMLElement>('.milkdown')
    if (!toolbar || !milkdown) return

    const toolbarWidth = toolbar.getBoundingClientRect().width
    const boundaryWidth =
      toolbar.offsetParent instanceof HTMLElement
        ? toolbar.offsetParent.getBoundingClientRect().width
        : milkdown.getBoundingClientRect().width
    const currentLeft = Number.parseFloat(toolbar.style.left)
    if (!Number.isFinite(currentLeft) || toolbarWidth <= 0 || boundaryWidth <= 0) return

    const padding = 12
    const maxLeft = Math.max(padding, boundaryWidth - toolbarWidth - padding)
    const nextLeft = Math.min(Math.max(currentLeft, padding), maxLeft)
    if (Math.abs(nextLeft - currentLeft) > 0.5) toolbar.style.left = `${nextLeft}px`
  }

  const schedule = () => {
    if (frame != null) return
    frame = window.requestAnimationFrame(update)
  }

  const observer = new MutationObserver(schedule)
  observer.observe(shell, {
    attributes: true,
    attributeFilter: ['data-show', 'style'],
    childList: true,
    subtree: true,
  })
  document.addEventListener('selectionchange', schedule)
  window.addEventListener('resize', schedule)
  window.addEventListener('scroll', schedule, true)
  schedule()

  return () => {
    if (frame != null) window.cancelAnimationFrame(frame)
    document.removeEventListener('selectionchange', schedule)
    window.removeEventListener('resize', schedule)
    window.removeEventListener('scroll', schedule, true)
    observer.disconnect()
  }
}

export default function MarkdownEditor({
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
}: MarkdownEditorProps): JSX.Element {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const editorRef = useRef<Crepe | null>(null)
  const onChangeRef = useRef(onChange)
  const valueRef = useRef(value)
  const [fallback, setFallback] = useState(false)
  const readonly = disabled || readOnly

  useEffect(() => {
    onChangeRef.current = onChange
  }, [onChange])

  useEffect(() => {
    valueRef.current = value
  }, [value])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return undefined

    let cancelled = false
    let cleanupToolbarBounds: (() => void) | null = null
    root.innerHTML = ''
    setFallback(false)
    const interactiveTools = !readonly

    const editor = new Crepe({
      root,
      defaultValue: value,
      features: {
        [CrepeFeature.Cursor]: true,
        [CrepeFeature.BlockEdit]: interactiveTools,
        [CrepeFeature.Toolbar]: interactiveTools,
        [CrepeFeature.Placeholder]: true,
        [CrepeFeature.ListItem]: true,
        [CrepeFeature.LinkTooltip]: interactiveTools,
        [CrepeFeature.Table]: true,
        [CrepeFeature.CodeMirror]: true,
        [CrepeFeature.ImageBlock]: false,
        [CrepeFeature.Latex]: false,
      },
      featureConfigs: {
        [CrepeFeature.Placeholder]: {
          text: placeholder,
          mode: 'block',
        },
      },
    })

    editor.on((api) => {
      api.markdownUpdated((_ctx, markdown, prevMarkdown) => {
        valueRef.current = markdown
        if (markdown !== prevMarkdown) onChangeRef.current(markdown)
      })
    })

    editor.create()
      .then(() => {
        if (cancelled) {
          void editor.destroy()
          return
        }
        editorRef.current = editor
        editor.setReadonly(readonly)
        if (root.parentElement) cleanupToolbarBounds = keepToolbarInsideShell(root.parentElement)
      })
      .catch(() => {
        if (!cancelled) setFallback(true)
      })

    return () => {
      cancelled = true
      cleanupToolbarBounds?.()
      editorRef.current = null
      void editor.destroy()
    }
  }, [])

  useEffect(() => {
    editorRef.current?.setReadonly(readonly)
  }, [readonly])

  useEffect(() => {
    const editor = editorRef.current
    if (!editor) return
    if (editor.getMarkdown() === value) return
    valueRef.current = value
    editor.editor.action(replaceAll(value))
  }, [value])

  if (fallback) {
    return (
      <Textarea
        id={id}
        name={name}
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
    )
  }

  return (
    <div
      id={id}
      className={cn('markdown-editor-shell', readOnly && 'markdown-editor-shell--readonly', className)}
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
      aria-describedby={ariaDescribedBy}
    >
      {name ? <input type="hidden" name={name} value={value} /> : null}
      <div ref={rootRef} />
    </div>
  )
}
