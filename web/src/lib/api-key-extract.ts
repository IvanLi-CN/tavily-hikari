const TVLY_DEV_API_KEY_PATTERN = /tvly-dev-[A-Za-z0-9_-]+/
const TOKEN_SPLIT_PATTERN = /[\s|;,]+/

export interface ApiKeyImportEntry {
  api_key: string
  registration_ip: string | null
}

function cleanToken(token: string): string {
  return token.trim().replace(/^[\[\]{}()<>"'`]+|[\[\]{}()<>"'`]+$/g, '')
}

function parseIpv4(input: string): number[] | null {
  if (!/^(?:\d{1,3}\.){3}\d{1,3}$/.test(input)) return null
  const octets = input.split('.').map((part) => Number.parseInt(part, 10))
  if (octets.some((value) => !Number.isInteger(value) || value < 0 || value > 255)) return null
  return octets
}

function isPublicIpv4(octets: number[]): boolean {
  const [a, b, c] = octets
  if (a === 0 || a >= 240) return false
  if (a === 10 || a === 127) return false
  if (a === 100 && b >= 64 && b <= 127) return false
  if (a === 169 && b === 254) return false
  if (a === 172 && b >= 16 && b <= 31) return false
  if (a === 192 && b === 168) return false
  if (a === 192 && b === 0 && c === 0) return false
  if (a === 198 && (b === 18 || b === 19)) return false
  if (a === 192 && b === 0 && c === 2) return false
  if (a === 198 && b === 51 && c === 100) return false
  if (a === 203 && b === 0 && c === 113) return false
  if (a >= 224 && a <= 239) return false
  return true
}

function parseIpv6(input: string): number[] | null {
  const raw = input.trim().toLowerCase()
  if (!raw.includes(':')) return null
  if (!/^[0-9a-f:.]+$/.test(raw)) return null

  const doubleColonIndex = raw.indexOf('::')
  if (doubleColonIndex !== -1 && raw.indexOf('::', doubleColonIndex + 1) !== -1) return null

  const [leftRaw, rightRaw = ''] = raw.split('::')
  const left = leftRaw.length > 0 ? leftRaw.split(':') : []
  const right = doubleColonIndex === -1
    ? []
    : rightRaw.length > 0
      ? rightRaw.split(':')
      : []

  const parseSegmentList = (segments: string[]): number[] | null => {
    const parsed: number[] = []
    for (const segment of segments) {
      if (!segment) return null
      if (segment.includes('.')) {
        const v4 = parseIpv4(segment)
        if (!v4) return null
        parsed.push((v4[0] << 8) | v4[1], (v4[2] << 8) | v4[3])
        continue
      }
      if (!/^[0-9a-f]{1,4}$/.test(segment)) return null
      parsed.push(Number.parseInt(segment, 16))
    }
    return parsed
  }

  const leftSegments = parseSegmentList(left)
  const rightSegments = parseSegmentList(right)
  if (!leftSegments || !rightSegments) return null

  const totalSegments = leftSegments.length + rightSegments.length
  if (doubleColonIndex === -1) {
    if (totalSegments !== 8) return null
    return [...leftSegments, ...rightSegments]
  }
  if (totalSegments >= 8) return null

  const missingSegments = 8 - totalSegments
  return [...leftSegments, ...new Array<number>(missingSegments).fill(0), ...rightSegments]
}

function isPublicIpv6(segments: number[]): boolean {
  if (segments.length !== 8) return false
  const isAllZero = segments.every((segment) => segment === 0)
  if (isAllZero) return false
  const isLoopback = segments.slice(0, 7).every((segment) => segment === 0) && segments[7] === 1
  if (isLoopback) return false

  const firstByte = segments[0] >> 8
  const secondByte = segments[0] & 0xff
  if (firstByte === 0xff) return false
  if ((firstByte & 0xfe) === 0xfc) return false
  if (firstByte === 0xfe && (secondByte & 0xc0) === 0x80) return false
  if (segments[0] === 0x2001 && segments[1] === 0x0db8) return false
  return true
}

export function extractRegistrationIpFromLine(line: string): string | null {
  const tokens = line.split(TOKEN_SPLIT_PATTERN)
  for (const rawToken of tokens) {
    const token = cleanToken(rawToken)
    if (!token) continue
    const ipv4 = parseIpv4(token)
    if (ipv4 && isPublicIpv4(ipv4)) return token
    const ipv6 = parseIpv6(token)
    if (ipv6 && isPublicIpv6(ipv6)) return token
  }
  return null
}

export function extractApiKeyImportEntryFromLine(line: string): ApiKeyImportEntry | null {
  const match = line.match(TVLY_DEV_API_KEY_PATTERN)
  if (!match) return null
  return {
    api_key: match[0],
    registration_ip: extractRegistrationIpFromLine(line),
  }
}

export function extractApiKeyImportEntriesFromText(text: string): ApiKeyImportEntry[] {
  return text
    .split(/\r?\n/)
    .map((line) => extractApiKeyImportEntryFromLine(line))
    .filter((entry): entry is ApiKeyImportEntry => entry != null)
}

export function extractTvlyDevApiKeyFromLine(line: string): string | null {
  return extractApiKeyImportEntryFromLine(line)?.api_key ?? null
}

export function extractTvlyDevApiKeysFromText(text: string): string[] {
  return extractApiKeyImportEntriesFromText(text).map((entry) => entry.api_key)
}
