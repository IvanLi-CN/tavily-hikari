const TVLY_DEV_API_KEY_PATTERN = /tvly-dev-[A-Za-z0-9_-]+/

export function extractTvlyDevApiKeyFromLine(line: string): string | null {
  const match = line.match(TVLY_DEV_API_KEY_PATTERN)
  return match?.[0] ?? null
}

export function extractTvlyDevApiKeysFromText(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => extractTvlyDevApiKeyFromLine(line))
    .filter((apiKey): apiKey is string => apiKey != null)
}
