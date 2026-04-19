#!/usr/bin/env bun

import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { createServer } from "node:net";

import { chromium } from "playwright-core";

function log(message: string) {
  console.log(`[system-settings-browser-e2e] ${message}`);
}

async function reservePort(): Promise<number> {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        reject(new Error("failed to reserve local port"));
        return;
      }
      const { port } = address;
      server.close((err) => {
        if (err) {
          reject(err);
          return;
        }
        resolve(port);
      });
    });
    server.on("error", reject);
  });
}

function runOrThrow(
  cmd: string[],
  cwd: string,
  label: string,
  env?: NodeJS.ProcessEnv,
) {
  const result = spawnSync(cmd[0], cmd.slice(1), {
    cwd,
    env: { ...process.env, ...env },
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${label} failed with exit code ${result.status ?? "unknown"}`);
  }
}

function ensureWebBuild(repoRoot: string) {
  log("Building web/dist for browser E2E");
  runOrThrow(["bun", "run", "build"], path.join(repoRoot, "web"), "web build");
}

function resolveBackendBinary(repoRoot: string): string {
  const binary = path.join(repoRoot, "target", "debug", "tavily-hikari");
  if (!existsSync(binary)) {
    log("Backend binary missing, building target/debug/tavily-hikari");
    runOrThrow(["cargo", "build", "--bin", "tavily-hikari"], repoRoot, "cargo build");
  }
  return binary;
}

function tryWhich(command: string): string | null {
  const result = spawnSync("which", [command], {
    stdio: ["ignore", "pipe", "ignore"],
    encoding: "utf8",
  });
  if (result.status !== 0) return null;
  const resolved = result.stdout.trim();
  return resolved.length > 0 ? resolved : null;
}

function resolveChromeExecutable(): string {
  const candidates = [
    process.env.CHROME_EXECUTABLE,
    process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    tryWhich("google-chrome"),
    tryWhich("google-chrome-stable"),
    tryWhich("chromium"),
    tryWhich("chromium-browser"),
  ].filter((value): value is string => Boolean(value));

  for (const candidate of candidates) {
    if (existsSync(candidate) || candidate.startsWith("/usr/bin/") || candidate.startsWith("/opt/")) {
      return candidate;
    }
  }

  throw new Error(
    "No Chrome/Chromium executable found. Set CHROME_EXECUTABLE to a local Chrome path.",
  );
}

function startBackend(
  backendBinary: string,
  repoRoot: string,
  backendPort: number,
  upstreamPort: number,
  dbPath: string,
): {
  child: ChildProcessWithoutNullStreams;
  getStdout: () => string;
  getStderr: () => string;
} {
  const staticDir = path.join(repoRoot, "web", "dist");
  const args = [
    "--bind",
    "127.0.0.1",
    "--port",
    String(backendPort),
    "--db-path",
    dbPath,
    "--static-dir",
    staticDir,
    "--keys",
    "tvly-browser-e2e-key",
    "--upstream",
    `http://127.0.0.1:${upstreamPort}/mcp`,
    "--usage-base",
    `http://127.0.0.1:${upstreamPort}`,
    "--dev-open-admin",
  ];
  const child = spawn(backendBinary, args, {
    cwd: repoRoot,
    env: process.env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk.toString();
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });
  return {
    child,
    getStdout: () => stdout,
    getStderr: () => stderr,
  };
}

async function waitForHealth(baseUrl: string, child: ChildProcessWithoutNullStreams) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (child.exitCode != null) {
      throw new Error(`backend exited early with code ${child.exitCode}`);
    }
    try {
      const response = await fetch(`${baseUrl}/health`);
      if (response.ok) return;
    } catch {
      // retry
    }
    await Bun.sleep(200);
  }
  throw new Error("backend did not become healthy in time");
}

async function waitForInputValue(
  page: import("playwright-core").Page,
  selector: string,
  expected: string,
) {
  await page.waitForFunction(
    ({ selector, expected }) => {
      const element = document.querySelector(selector);
      return element instanceof HTMLInputElement && element.value === expected;
    },
    { selector, expected },
    { timeout: 10_000 },
  );
}

async function assertServerSetting(
  baseUrl: string,
  expected: { requestRateLimit: number; mcpSessionAffinityKeyCount: number },
) {
  const response = await fetch(`${baseUrl}/api/settings`);
  if (!response.ok) {
    throw new Error(`GET /api/settings failed with ${response.status}`);
  }
  const payload = (await response.json()) as {
    systemSettings?: {
      requestRateLimit?: number;
      mcpSessionAffinityKeyCount?: number;
    };
  };
  const actualRequestRateLimit = payload.systemSettings?.requestRateLimit;
  const actualAffinityCount = payload.systemSettings?.mcpSessionAffinityKeyCount;
  if (actualRequestRateLimit !== expected.requestRateLimit) {
    throw new Error(
      `expected requestRateLimit ${expected.requestRateLimit}, got ${actualRequestRateLimit ?? "undefined"}`,
    );
  }
  if (actualAffinityCount !== expected.mcpSessionAffinityKeyCount) {
    throw new Error(
      `expected mcpSessionAffinityKeyCount ${expected.mcpSessionAffinityKeyCount}, got ${actualAffinityCount ?? "undefined"}`,
    );
  }
}

async function main() {
  const repoRoot = path.resolve(import.meta.dir, "..", "..");
  const tempRoot = mkdtempSync(path.join(tmpdir(), "tavily-hikari-browser-e2e-"));
  const dbPath = path.join(tempRoot, "browser-e2e.db");
  const screenshotPath = path.join(tempRoot, "system-settings-browser-e2e-failure.png");
  let success = false;

  let upstreamServer: ReturnType<typeof Bun.serve> | null = null;
  let backend:
    | {
        child: ChildProcessWithoutNullStreams;
        getStdout: () => string;
        getStderr: () => string;
      }
    | null = null;
  let browser: import("playwright-core").Browser | null = null;
  let page: import("playwright-core").Page | null = null;

  try {
    ensureWebBuild(repoRoot);
    const backendBinary = resolveBackendBinary(repoRoot);
    const chromeExecutable = resolveChromeExecutable();
    const upstreamPort = await reservePort();
    const backendPort = await reservePort();
    const baseUrl = `http://127.0.0.1:${backendPort}`;

    upstreamServer = Bun.serve({
      hostname: "127.0.0.1",
      port: upstreamPort,
      fetch(request) {
        const { pathname } = new URL(request.url);
        if (pathname === "/mcp") {
          return Response.json({
            jsonrpc: "2.0",
            id: 1,
            result: {
              protocolVersion: "2025-03-26",
              serverInfo: { name: "mock-upstream", version: "1.0.0" },
              capabilities: {},
            },
          });
        }
        return Response.json({ ok: true, status: 200 });
      },
    });

    backend = startBackend(backendBinary, repoRoot, backendPort, upstreamPort, dbPath);
    await waitForHealth(baseUrl, backend.child);

    log(`Launching Chrome from ${chromeExecutable}`);
    browser = await chromium.launch({
      executablePath: chromeExecutable,
      headless: true,
      args: ["--no-first-run", "--no-default-browser-check"],
    });

    const context = await browser.newContext({
      baseURL: baseUrl,
      locale: "en-US",
      viewport: { width: 1440, height: 1100 },
    });
    page = await context.newPage();

    await page.route("**/api/settings/system", async (route) => {
      await Bun.sleep(450);
      await route.continue();
    });

    log("Opening /admin/system-settings");
    await page.goto(`${baseUrl}/admin/system-settings`, { waitUntil: "domcontentloaded" });
    await page.locator("#system-settings-request-rate-limit").waitFor({ state: "visible" });
    await page.locator("#system-settings-affinity-count").waitFor({ state: "visible" });
    await waitForInputValue(page, "#system-settings-request-rate-limit", "100");
    await waitForInputValue(page, "#system-settings-affinity-count", "5");

    const applyButton = page.getByTestId("system-settings-apply");
    if (await applyButton.isEnabled()) {
      throw new Error("apply button should be disabled before any changes");
    }

    await page.locator("#system-settings-request-rate-limit").fill("0");
    await page.getByText("Enter a safe integer greater than or equal to 1.").waitFor({
      state: "visible",
    });
    if (await applyButton.isEnabled()) {
      throw new Error("apply button should stay disabled for an invalid request-rate limit");
    }

    await page.locator("#system-settings-request-rate-limit").fill("75");
    await page.locator("#system-settings-affinity-count").fill("3");
    if (!(await applyButton.isEnabled())) {
      throw new Error("apply button should enable after editing valid system settings");
    }

    const saveResponse = page.waitForResponse(
      (response) =>
        response.url().endsWith("/api/settings/system") &&
        response.request().method() === "PUT",
    );

    log("Clicking Apply in the browser");
    await applyButton.click();
    await page.waitForFunction(
      () => {
        const button = document.querySelector(
          '[data-testid="system-settings-apply"]',
        ) as HTMLButtonElement | null;
        return (
          button != null &&
          button.disabled &&
          button.querySelector(".icon-spin") != null
        );
      },
      undefined,
      { timeout: 10_000 },
    );

    const response = await saveResponse;
    if (!response.ok()) {
      throw new Error(`PUT /api/settings/system returned ${response.status()}`);
    }

    await page.waitForFunction(
      () => {
        const button = document.querySelector(
          '[data-testid="system-settings-apply"]',
        ) as HTMLButtonElement | null;
        return button != null && button.disabled && button.querySelector(".icon-spin") == null;
      },
      undefined,
      { timeout: 10_000 },
    );

    await assertServerSetting(baseUrl, {
      requestRateLimit: 75,
      mcpSessionAffinityKeyCount: 3,
    });

    log("Reloading page to verify the saved value persists");
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator("#system-settings-request-rate-limit").waitFor({ state: "visible" });
    await page.locator("#system-settings-affinity-count").waitFor({ state: "visible" });
    await waitForInputValue(page, "#system-settings-request-rate-limit", "75");
    await waitForInputValue(page, "#system-settings-affinity-count", "3");

    log("Browser E2E passed");
    success = true;
  } catch (error) {
    if (page) {
      try {
        await page.screenshot({ path: screenshotPath, fullPage: true });
        console.error(`Failure screenshot saved to ${screenshotPath}`);
      } catch (screenshotError) {
        console.error("Failed to capture browser E2E screenshot:", screenshotError);
      }
    }

    if (backend) {
      console.error("Backend stdout:");
      console.error(backend.getStdout() || "<empty>");
      console.error("Backend stderr:");
      console.error(backend.getStderr() || "<empty>");
    }

    throw error;
  } finally {
    if (browser) {
      await browser.close();
    }
    if (backend) {
      backend.child.kill("SIGTERM");
      await new Promise((resolve) => backend?.child.once("exit", resolve));
    }
    upstreamServer?.stop(true);
    if (success) {
      rmSync(tempRoot, { force: true, recursive: true });
    } else {
      console.error(`Browser E2E artifacts preserved at ${tempRoot}`);
    }
  }
}

await main();
