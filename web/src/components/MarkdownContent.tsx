import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

interface MarkdownContentProps {
  content: string
  className?: string
  compact?: boolean
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
}: MarkdownContentProps): JSX.Element {
  const classes = [
    'markdown-content',
    compact ? 'markdown-content-compact' : null,
    className,
  ].filter(Boolean).join(' ')

  return (
    <div className={classes}>
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
    </div>
  )
}
