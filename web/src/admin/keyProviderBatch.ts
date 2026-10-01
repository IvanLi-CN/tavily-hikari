/**
 * Provider-aware batch import bridge (plan todo 22).
 *
 * The committed batch endpoint (`POST /api/keys/batch`) has no provider field,
 * so the legacy Tavily path stays byte-identical through `addApiKeysBatch`.
 * For any other provider the batch contract is "one provider per batch" and we
 * stamp each key through the single-key endpoint, which does accept provider.
 */
import {
  addApiKey,
  addApiKeysBatch,
  type AddApiKeysBatchItem,
  type AddApiKeysBatchResponse,
  type AddApiKeysBatchResult,
  type ProviderId,
} from '../api'

export async function addProviderAwareApiKeysBatch(
  provider: ProviderId | undefined,
  items: AddApiKeysBatchItem[],
  group?: string,
  exhaustedApiKeys?: string[],
): Promise<AddApiKeysBatchResponse> {
  if (!provider || provider === 'tavily') {
    return addApiKeysBatch(items, group, exhaustedApiKeys)
  }
  const results: AddApiKeysBatchResult[] = []
  let created = 0
  let failed = 0
  for (const item of items) {
    try {
      const response = await addApiKey(item.api_key, group, provider)
      results.push({ api_key: item.api_key, status: 'created', id: response.id })
      created += 1
    } catch (err) {
      results.push({
        api_key: item.api_key,
        status: 'failed',
        error: err instanceof Error && err.message ? err.message : 'Failed to add key',
      })
      failed += 1
    }
  }
  return {
    summary: {
      input_lines: items.length,
      valid_lines: items.length,
      unique_in_input: items.length,
      created,
      undeleted: 0,
      existed: 0,
      duplicate_in_input: 0,
      failed,
    },
    results,
  }
}