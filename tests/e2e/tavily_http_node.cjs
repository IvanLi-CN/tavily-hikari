#!/usr/bin/env node

/**
 * Simple end-to-end smoke test for the Tavily HTTP proxy using the official
 * JavaScript SDK `@tavily/core`.
 *
 * Requirements:
 * - Tavily Hikari backend running locally (e.g. via scripts/start-backend-dev.sh)
 * - TAVILY_USAGE_BASE configured to point at a mock Tavily HTTP upstream
 * - HIKARI_TAVILY_TOKEN (or TAVILY_HIKARI_TOKEN) set to a valid Hikari access token
 *
 * The test sends a search / extract / crawl / map request through the SDK with
 * apiBaseURL overridden to point at Hikari's `/api/tavily` façade and performs
 * basic structural assertions on the responses.
 */

/* eslint-disable @typescript-eslint/no-var-requires */
const { tavily } = require("@tavily/core");

async function main() {
  const baseUrl =
    process.env.HIKARI_TAVILY_BASE_URL ||
    "http://127.0.0.1:58087/api/tavily";
  const apiKey =
    process.env.HIKARI_TAVILY_TOKEN || process.env.TAVILY_HIKARI_TOKEN;

  if (!apiKey) {
    console.error(
      "Missing Hikari access token: set HIKARI_TAVILY_TOKEN or TAVILY_HIKARI_TOKEN.",
    );
    process.exit(1);
  }

  console.log(`Using Tavily HTTP base URL: ${baseUrl}`);

  const client = tavily({
    apiKey,
    apiBaseURL: baseUrl,
  });

  try {
    const response = await client.search(
      "tavily hikari http proxy e2e smoke test",
      {
        searchDepth: "basic",
        maxResults: 3,
        includeAnswer: false,
        includeImages: false,
        includeRawContent: false,
      },
    );

    if (!response || !Array.isArray(response.results)) {
      throw new Error("Unexpected Tavily response shape (missing results array)");
    }

    console.log(
      `Search OK: query="${response.query}", results=${response.results.length}, responseTime=${response.responseTime}`,
    );

    const first = response.results[0];
    if (first) {
      if (typeof first.url !== "string" || typeof first.title !== "string") {
        throw new Error("First result is missing url/title fields");
      }
    }

    console.log(
      "Search endpoint OK via @tavily/core → Hikari /api/tavily/search.",
    );

    // --- extract ---

    // The official SDK exposes `extract`, so we call it directly. Hikari
    // proxies this to `/api/tavily/extract` which, in turn, targets the
    // configured Tavily HTTP upstream (usually mock_tavily in tests).
    const extractResult = await client.extract(
      ["https://example.com/"],
      {
        includeImages: false,
        format: "text",
        timeout: 10,
      },
    );

    if (
      !extractResult ||
      !Array.isArray(extractResult.results) ||
      !Array.isArray(extractResult.failedResults)
    ) {
      throw new Error(
        "Unexpected extract response shape (results / failedResults).",
      );
    }

    console.log(
      `Extract endpoint OK: results=${extractResult.results.length}, failed=${extractResult.failedResults.length}, responseTime=${extractResult.responseTime}`,
    );

    // --- crawl ---

    const crawlResult = await client.crawl("https://example.com/", {
      maxDepth: 1,
      maxBreadth: 2,
      limit: 5,
      includeImages: false,
      allowExternal: false,
      format: "text",
      timeout: 15,
    });

    if (!crawlResult || !Array.isArray(crawlResult.results)) {
      throw new Error("Unexpected crawl response shape (missing results array).");
    }

    console.log(
      `Crawl endpoint OK: baseUrl=${crawlResult.baseUrl}, results=${crawlResult.results.length}, responseTime=${crawlResult.responseTime}`,
    );

    // --- map ---

    const mapResult = await client.map("https://example.com/", {
      maxDepth: 1,
      maxBreadth: 2,
      limit: 5,
      timeout: 15,
    });

    if (!mapResult || !Array.isArray(mapResult.results)) {
      throw new Error("Unexpected map response shape (missing results array).");
    }

    console.log(
      `Map endpoint OK: baseUrl=${mapResult.baseUrl}, results=${mapResult.results.length}, responseTime=${mapResult.responseTime}`,
    );
  } catch (err) {
    console.error("E2E Tavily HTTP proxy test failed:", err);
    process.exit(1);
  }

  console.log("E2E Tavily HTTP proxy test passed.");
}

if (require.main === module) {
  main();
}
