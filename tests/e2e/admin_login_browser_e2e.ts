#!/usr/bin/env bun

import { existsSync } from "node:fs";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import path from "node:path";
import { createServer } from "node:net";

import { chromium, type Browser, type Locator, type Page } from "playwright-core";

function log(message: string) {
  console.log(`[admin-login-browser-e2e] ${message}`);
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
      server.close((error) => {
        if (error) reject(error);
        else resolve(port);
      });
    });
    server.on("error", reject);
  });
}

function tryWhich(command: string): string | null {
  const result = spawnSync("which", [command], {
    stdio: ["ignore", "pipe", "ignore"],
    encoding: "utf8",
  });
  return result.status === 0 ? result.stdout.trim() : null;
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

  throw new Error("No Chrome/Chromium executable found. Set CHROME_EXECUTABLE.");
}

function startDemoServer(repoRoot: string, port: number): ChildProcessWithoutNullStreams {
  const webRoot = path.join(repoRoot, "web");
  const child = spawn(
    "bun",
    ["--bun", "./node_modules/.bin/vite", "--host", "127.0.0.1", "--port", String(port)],
    {
      cwd: webRoot,
      env: { ...process.env, VITE_DEMO_MODE: "true" },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.stdout.on("data", (chunk) => process.stdout.write(chunk));
  child.stderr.on("data", (chunk) => process.stderr.write(chunk));
  return child;
}

async function waitForLoginRoute(baseUrl: string, child: ChildProcessWithoutNullStreams) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (child.exitCode != null) {
      throw new Error(`demo server exited early with code ${child.exitCode}`);
    }
    try {
      const response = await fetch(`${baseUrl}/login`);
      if (response.ok) return;
    } catch {
      // retry until Vite is ready
    }
    await Bun.sleep(200);
  }
  throw new Error("demo server did not serve /login in time");
}

async function assertVisible(locator: Locator, label: string) {
  await locator.waitFor({ state: "visible", timeout: 10_000 }).catch((error) => {
    throw new Error(`Expected ${label} to be visible: ${error instanceof Error ? error.message : String(error)}`);
  });
}

async function assertDisabled(locator: Locator, label: string) {
  if (!(await locator.isDisabled())) {
    throw new Error(`Expected ${label} to be disabled`);
  }
}

async function assertEnabled(locator: Locator, label: string) {
  if (await locator.isDisabled()) {
    throw new Error(`Expected ${label} to be enabled`);
  }
}

async function assertLoginShell(page: Page, baseUrl: string) {
  log("opening login route");
  page.setDefaultTimeout(30_000);
  page.setDefaultNavigationTimeout(30_000);
  await page.goto(`${baseUrl}/login`, { waitUntil: "domcontentloaded" });
  log("asserting login shell");
  await assertVisible(page.getByRole("heading", { name: /admin login|管理员登录/i }), "admin login heading");
  await assertVisible(page.getByText(/login credentials|登录凭据/i), "login credentials panel");
  if (await page.locator(".auth-method-list").count()) {
    throw new Error("Expected login method status summary to be absent");
  }
  await assertVisible(page.locator("#admin-totp-code-input"), "TOTP input");
  await assertVisible(page.locator("#admin-password-input"), "password input");
  await assertDisabled(page.getByRole("button", { name: /passkey/i }), "passkey button before TOTP");
  await assertDisabled(page.getByRole("button", { name: /sign in|登录/i }).last(), "password submit before credentials");
}

async function assertPasswordLogin(browser: Browser, baseUrl: string) {
  const context = await browser.newContext();
  const page = await context.newPage();
  log("checking password login flow");
  await assertLoginShell(page, baseUrl);
  await page.locator("#admin-totp-code-input").fill("123456");
  await page.locator("#admin-password-input").fill("demo-admin-password");
  await assertEnabled(page.getByRole("button", { name: /sign in|登录/i }).last(), "password submit after credentials");
  await Promise.all([
    page.waitForURL((url) => url.pathname === "/admin" || url.pathname === "/admin/", { timeout: 10_000 }),
    page.getByRole("button", { name: /sign in|登录/i }).last().click(),
  ]);
  log("password login reached admin");
  await context.close();
}

async function assertPasskeyLogin(browser: Browser, baseUrl: string) {
  const context = await browser.newContext();
  const page = await context.newPage();
  log("checking passkey login flow");
  await assertLoginShell(page, baseUrl);
  await page.locator("#admin-totp-code-input").fill("654321");
  await assertEnabled(page.getByRole("button", { name: /passkey/i }), "passkey button after TOTP");
  await Promise.all([
    page.waitForURL((url) => url.pathname === "/admin" || url.pathname === "/admin/", { timeout: 10_000 }),
    page.getByRole("button", { name: /passkey/i }).click(),
  ]);
  log("passkey login reached admin");
  await context.close();
}

async function main() {
  const repoRoot = path.resolve(import.meta.dir, "..", "..");
  const port = await reservePort();
  const baseUrl = `http://127.0.0.1:${port}`;
  const server = startDemoServer(repoRoot, port);
  let browser: Browser | null = null;

  try {
    await waitForLoginRoute(baseUrl, server);
    log(`demo server ready at ${baseUrl}`);
    browser = await chromium.launch({
      executablePath: resolveChromeExecutable(),
      headless: true,
    });
    await assertPasswordLogin(browser, baseUrl);
    await assertPasskeyLogin(browser, baseUrl);
    log("login shell, password login, and demo passkey login passed");
  } finally {
    if (browser) await browser.close();
    server.kill("SIGTERM");
  }
}

await main();
