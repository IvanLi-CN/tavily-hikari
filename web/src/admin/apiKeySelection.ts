export function retainVisibleApiKeySelection(
  selectedIds: Iterable<string>,
  visibleIds: Iterable<string>,
): string[] {
  const visible = new Set<string>()
  for (const id of visibleIds) {
    const normalized = id.trim()
    if (!normalized) continue
    visible.add(normalized)
  }

  const retained: string[] = []
  const seen = new Set<string>()
  for (const id of selectedIds) {
    const normalized = id.trim()
    if (!normalized || seen.has(normalized)) continue
    seen.add(normalized)
    if (visible.has(normalized)) {
      retained.push(normalized)
    }
  }

  return retained
}
