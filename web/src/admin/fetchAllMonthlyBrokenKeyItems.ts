import type { MonthlyBrokenKeyDetail, Paginated } from "../api";

type FetchMonthlyBrokenKeyPage = (
  page: number,
  signal?: AbortSignal,
) => Promise<Paginated<MonthlyBrokenKeyDetail>>;

function clampTotal(total: number, itemCount: number): number {
  if (!Number.isFinite(total) || total < 0) {
    return itemCount;
  }
  return Math.max(itemCount, Math.trunc(total));
}

export async function fetchAllMonthlyBrokenKeyItems(
  fetchPage: FetchMonthlyBrokenKeyPage,
  signal?: AbortSignal,
): Promise<MonthlyBrokenKeyDetail[]> {
  const firstPage = await fetchPage(1, signal);
  const resolvedTotal = clampTotal(firstPage.total, firstPage.items.length);
  const resolvedPerPage = Math.max(1, Math.trunc(firstPage.perPage) || 1);
  const totalPages = Math.max(1, Math.ceil(resolvedTotal / resolvedPerPage));

  if (totalPages === 1) {
    return firstPage.items.slice(0, resolvedTotal);
  }

  const remainingPages = await Promise.all(
    Array.from({ length: totalPages - 1 }, (_, index) =>
      fetchPage(index + 2, signal),
    ),
  );
  return [firstPage, ...remainingPages]
    .flatMap((page) => page.items)
    .slice(0, resolvedTotal);
}
