import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

interface MarkdownContentProps {
  content: string
  className?: string
  compact?: boolean
  inline?: boolean
  compactWrap?: boolean
}

function safeMarkdownHref(href: string | undefined): string | null {
  if (!href) return null
  const trimmed = href.trim()
  if (!trimmed) return null
  if (trimmed.startsWith('/') && !trimmed.startsWith('//')) return trimmed

  try {
    const url = new URL(trimmed)
    return ['http:', 'https:', 'mailto:'].includes(url.protocol) ? trimmed : null
  } catch {
    return null
  }
}

export default function MarkdownContent({
  content,
  className,
  compact = false,
  inline = false,
  compactWrap = false,
}: MarkdownContentProps): JSX.Element {
  const classes = [
    'markdown-content',
    compact ? 'markdown-content-compact' : null,
    inline ? 'markdown-content-inline' : null,
    compactWrap ? 'markdown-content-compact-wrap' : null,
    className,
  ].filter(Boolean).join(' ')
  const RootTag = inline ? 'span' : 'div'

  return (
    <RootTag className={classes}>
      <ReactMarkdown
        skipHtml
        remarkPlugins={[remarkGfm]}
        components={{
          a({ href, children, ...props }) {
            const safeHref = safeMarkdownHref(href)
            if (!safeHref) return <span>{children}</span>
            return (
              <a
                {...props}
                href={safeHref}
                target="_blank"
                rel="noopener noreferrer"
              >
                {children}
              </a>
            )
          },
          img() {
            return null
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </RootTag>
  )
}
