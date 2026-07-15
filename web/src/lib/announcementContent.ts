const ATX_HEADING_PATTERN = /^( {0,3})(#{1,6})[ \t]+(.+?)\s*$/u
const SETEXT_HEADING_PATTERN = /^ {0,3}(=+|-+)[ \t]*$/u
const TRAILING_HASHES_PATTERN = /(?:[ \t]+#+[ \t]*)$/u

export interface ParsedAnnouncementContent {
  fullContent: string
  titleMarkdown: string | null
  titleText: string | null
  bodyMarkdown: string
  hasTitle: boolean
  hasBody: boolean
  summary: string
}

function normalizeAnnouncementContent(content: string): string {
  return content.replace(/\r\n?/gu, '\n').trim()
}

function parseAtxHeading(line: string): string | null {
  const match = line.match(ATX_HEADING_PATTERN)
  if (!match) return null
  const title = match[3].replace(TRAILING_HASHES_PATTERN, '').trim()
  return title.length > 0 ? title : null
}

function isSetextUnderline(line: string): boolean {
  return SETEXT_HEADING_PATTERN.test(line)
}

export function markdownToPlainText(markdown: string): string {
  return markdown
    .replace(/\r\n?/gu, '\n')
    .replace(/!\[([^\]]*)\]\(([^)]+)\)/gu, '$1')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/gu, '$1')
    .replace(/`([^`]+)`/gu, '$1')
    .replace(/[*_~]+/gu, '')
    .replace(/^ {0,3}(#{1,6})[ \t]+/gmu, '')
    .replace(/^ {0,3}>\s?/gmu, '')
    .replace(/^ {0,3}[-+*]\s+/gmu, '')
    .replace(/^ {0,3}\d+\.\s+/gmu, '')
    .replace(/\n+/gu, ' ')
    .replace(/\s+/gu, ' ')
    .trim()
}

function summarizePlainText(markdown: string, maxChars = 120): string {
  const plain = markdownToPlainText(markdown)
  if (plain.length <= maxChars) return plain
  return `${plain.slice(0, maxChars - 1).trimEnd()}…`
}

export function parseAnnouncementContent(content: string): ParsedAnnouncementContent {
  const fullContent = normalizeAnnouncementContent(content)
  if (!fullContent) {
    return {
      fullContent: '',
      titleMarkdown: null,
      titleText: null,
      bodyMarkdown: '',
      hasTitle: false,
      hasBody: false,
      summary: '',
    }
  }

  const lines = fullContent.split('\n')
  let firstNonEmptyIndex = 0
  while (firstNonEmptyIndex < lines.length && lines[firstNonEmptyIndex].trim().length === 0) {
    firstNonEmptyIndex += 1
  }

  const firstLine = lines[firstNonEmptyIndex] ?? ''
  const atxTitle = parseAtxHeading(firstLine)
  if (atxTitle) {
    const bodyMarkdown = lines.slice(firstNonEmptyIndex + 1).join('\n').trim()
    return {
      fullContent,
      titleMarkdown: atxTitle,
      titleText: markdownToPlainText(atxTitle) || atxTitle,
      bodyMarkdown,
      hasTitle: true,
      hasBody: bodyMarkdown.length > 0,
      summary: summarizePlainText(atxTitle),
    }
  }

  const secondLine = lines[firstNonEmptyIndex + 1] ?? ''
  if (firstLine.trim().length > 0 && isSetextUnderline(secondLine)) {
    const titleMarkdown = firstLine.trim()
    const bodyMarkdown = lines.slice(firstNonEmptyIndex + 2).join('\n').trim()
    return {
      fullContent,
      titleMarkdown,
      titleText: markdownToPlainText(titleMarkdown) || titleMarkdown,
      bodyMarkdown,
      hasTitle: true,
      hasBody: bodyMarkdown.length > 0,
      summary: summarizePlainText(titleMarkdown),
    }
  }

  return {
    fullContent,
    titleMarkdown: null,
    titleText: null,
    bodyMarkdown: fullContent,
    hasTitle: false,
    hasBody: fullContent.length > 0,
    summary: summarizePlainText(fullContent),
  }
}
