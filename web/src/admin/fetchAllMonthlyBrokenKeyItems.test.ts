import { describe, expect, test } from "bun:test";

import type { MonthlyBrokenKeyDetail, Paginated } from "../api";
import { fetchAllMonthlyBrokenKeyItems } from "./fetchAllMonthlyBrokenKeyItems";

function buildItem(index: number): MonthlyBrokenKeyDetail {
  return {
    keyId: `key-${index}`,
    currentStatus: index % 2 === 0 ? "exhausted" : "quarantined",
    reasonCode: "manual_mark_exhausted",
    reasonSummary: `reason-${index}`,
    latestBreakAt: 1_700_000_000 + index,
    source: "manual",
    breakerTokenId: `tok-${index}`,
    breakerUserId: `user-${index}`,
    breakerUserDisplayName: `User ${index}`,
    manualActorDisplayName: null,
    relatedUsers: [],
  };
}

function buildPage(
  items: MonthlyBrokenKeyDetail[],
  total: number,
  page: number,
  perPage: number,
): Paginated<MonthlyBrokenKeyDetail> {
  return {
    items,
    total,
    page,
    perPage,
  };
}

describe("fetchAllMonthlyBrokenKeyItems", () => {
  test("fetches every page when the drawer total exceeds the first page", async () => {
    const items = Array.from({ length: 55 }, (_, index) =>
      buildItem(index + 1),
    );
    const calls: number[] = [];

    const result = await fetchAllMonthlyBrokenKeyItems(async (page) => {
      calls.push(page);
      const perPage = 20;
      const start = (page - 1) * perPage;
      return buildPage(
        items.slice(start, start + perPage),
        items.length,
        page,
        perPage,
      );
    });

    expect(calls).toEqual([1, 2, 3]);
    expect(result).toHaveLength(55);
    expect(result.at(0)?.keyId).toBe("key-1");
    expect(result.at(-1)?.keyId).toBe("key-55");
  });

  test("does not fetch extra pages when the first page already contains the full result", async () => {
    const calls: number[] = [];

    const result = await fetchAllMonthlyBrokenKeyItems(async (page) => {
      calls.push(page);
      return buildPage([buildItem(1), buildItem(2)], 2, page, 50);
    });

    expect(calls).toEqual([1]);
    expect(result.map((item) => item.keyId)).toEqual(["key-1", "key-2"]);
  });
});
