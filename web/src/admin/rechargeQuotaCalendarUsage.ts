export function getRechargeMonthUsedQuota(
  rowMonthStart: number,
  currentMonthStart: number | null,
  currentMonthUsed: number,
): number {
  if (currentMonthStart == null) return 0
  return rowMonthStart === currentMonthStart ? currentMonthUsed : 0
}
