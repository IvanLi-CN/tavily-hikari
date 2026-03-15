import { describe, expect, it } from 'bun:test'

import {
  extractApiKeyImportEntriesFromText,
  extractApiKeyImportEntryFromLine,
  extractRegistrationIpFromLine,
} from './api-key-extract'

describe('api key import parser', () => {
  it('extracts the first key and first public ipv4 from a mixed line', () => {
    expect(extractApiKeyImportEntryFromLine('mail@example.com | tvly-dev-abc | 8.8.8.8')).toEqual({
      api_key: 'tvly-dev-abc',
      registration_ip: '8.8.8.8',
    })
  })

  it('extracts public ipv6 tokens and ignores delimiters', () => {
    expect(extractApiKeyImportEntryFromLine('note;2606:4700:4700::1111,tvly-dev-ghi')).toEqual({
      api_key: 'tvly-dev-ghi',
      registration_ip: '2606:4700:4700::1111',
    })
  })

  it('ignores private and reserved addresses', () => {
    expect(extractRegistrationIpFromLine('user | tvly-dev-private | 10.0.0.1 | 192.168.1.2')).toBeNull()
    expect(extractRegistrationIpFromLine('tvly-dev-doc | 2001:db8::1')).toBeNull()
  })

  it('returns one structured entry per matched line', () => {
    expect(
      extractApiKeyImportEntriesFromText([
        'a tvly-dev-first 1.1.1.1',
        'skip me',
        'b;tvly-dev-second;8.8.4.4',
      ].join('\n')),
    ).toEqual([
      { api_key: 'tvly-dev-first', registration_ip: '1.1.1.1' },
      { api_key: 'tvly-dev-second', registration_ip: '8.8.4.4' },
    ])
  })
})
