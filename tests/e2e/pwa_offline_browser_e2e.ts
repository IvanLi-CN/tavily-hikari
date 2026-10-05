#!/usr/bin/env bun

import { Database } from "bun:sqlite";
import { cpSync, copyFileSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { createServer } from "node:net";

import { chromium } from "playwright-core";

const USER_SESSION_COOKIE_NAME = "hikari_user_session";

type InstallMetadata = {
  id: string;
  scope: string;
  startUrl: string;
  iconPath: string;
  iconByteLength: number;
  manifestCacheControl: string | null;
  iconCacheControl: string | null;
};

type ManifestIcon = {
  src: string;
  sizes: string;
  purpose?: string;
};

type ManifestPayload = {
  id: string;
  scope: string;
  start_url: string;
  icons: ManifestIcon[];
};

function log(message: string) {
  console.log(`[pwa-offline-e2e] ${message}`);
}

async function delay(ms: number) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function reservePort(): Promise<number> {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        reject(new Error("failed to reserve port"));
        return;
      }
      const port = address.port;
      server.close((err) => {
        if (err) reject(err);
        else resolve(port);
      });
    });
    server.on("error", reject);
  });
}

function runOrThrow(command: string[], cwd: string, label: string) {
  const result = spawnSync(command[0], command.slice(1), {
    cwd,
    env: process.env,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${label} failed with exit code ${result.status ?? "unknown"}`);
  }
}

function collectPlaywrightCacheExecutables(): string[] {
  const platformRelativeCandidates = process.platform === "darwin"
    ? [
        { priority: 0, relativePath: "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing" },
        { priority: 0, relativePath: "chrome-mac/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing" },
        { priority: 1, relativePath: "chrome-mac-arm64/Chromium.app/Contents/MacOS/Chromium" },
        { priority: 1, relativePath: "chrome-mac/Chromium.app/Contents/MacOS/Chromium" },
        { priority: 2, relativePath: "chrome-mac/headless_shell" },
        { priority: 2, relativePath: "chrome-mac-arm64/headless_shell" },
      ]
    : [];

  const rankedMatches: Array<{ executablePath: string; priority: number; version: number }> = [];
  const cacheRoots = [
    path.join(homedir(), "Library", "Caches", "ms-playwright"),
    path.join(homedir(), ".cache", "ms-playwright"),
  ];

  for (const cacheRoot of cacheRoots) {
    if (!existsSync(cacheRoot)) continue;
    for (const entry of readdirSync(cacheRoot, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const version = Number(entry.name.split("-").at(-1) ?? 0) || 0;
      const entryRoot = path.join(cacheRoot, entry.name);
      for (const candidate of platformRelativeCandidates) {
        const executablePath = path.join(entryRoot, candidate.relativePath);
        if (existsSync(executablePath)) {
          rankedMatches.push({
            executablePath,
            priority: candidate.priority,
            version,
          });
        }
      }
    }
  }

  return rankedMatches
    .sort((left, right) => left.priority - right.priority || right.version - left.version)
    .map((entry) => entry.executablePath);
}

function resolveChromeExecutables(): string[] {
  const orderedCandidates: string[] = [];
  const seen = new Set<string>();

  const addCandidate = (candidate: string | undefined | null) => {
    if (!candidate || seen.has(candidate) || !existsSync(candidate)) return;
    orderedCandidates.push(candidate);
    seen.add(candidate);
  };

  addCandidate(process.env.CHROME_EXECUTABLE);
  addCandidate(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH);

  for (const candidate of collectPlaywrightCacheExecutables()) {
    addCandidate(candidate);
  }

  addCandidate("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
  addCandidate("/Applications/Chromium.app/Contents/MacOS/Chromium");

  const whichTargets = ["chromium", "google-chrome", "google-chrome-stable"];
  for (const whichTarget of whichTargets) {
    const whichResult = spawnSync("which", [whichTarget], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    if (whichResult.status === 0) {
      addCandidate(whichResult.stdout.trim());
    }
  }

  if (orderedCandidates.length === 0) {
    throw new Error("No Chrome/Chromium executable found. Set CHROME_EXECUTABLE.");
  }

  return orderedCandidates;
}

async function launchBrowser(): Promise<import("playwright-core").Browser> {
  const failures: string[] = [];

  for (const executablePath of resolveChromeExecutables()) {
    let browser: import("playwright-core").Browser | null = null;
    try {
      log(`launching browser candidate: ${executablePath}`);
      browser = await chromium.launch({
        executablePath,
        headless: true,
        timeout: 20_000,
      });

      const smokeContext = await browser.newContext();
      const smokePage = await smokeContext.newPage();
      await smokePage.goto("data:text/html,<html><body>browser-smoke-ok</body></html>", {
        waitUntil: "load",
        timeout: 5_000,
      });
      await smokeContext.close();

      log(`using browser executable: ${executablePath}`);
      return browser;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      failures.push(`${executablePath}: ${message}`);
      if (browser) {
        await Promise.race([
          browser.close().catch(() => {}),
          delay(3_000),
        ]);
      }
    }
  }

  throw new Error(
    `Unable to launch Chromium for PWA offline E2E.\n${failures.join("\n\n")}`,
  );
}

function ensureBuild(repoRoot: string) {
  runOrThrow(["bun", "run", "build"], path.join(repoRoot, "web"), "web build");
}

function ensureBackend(repoRoot: string): string {
  const binary = path.join(repoRoot, "target", "debug", "tavily-hikari");
  if (!existsSync(binary)) {
    runOrThrow(["cargo", "build", "--bin", "tavily-hikari"], repoRoot, "cargo build");
  }
  return binary;
}

function stageStaticRelease(repoRoot: string, tempRoot: string, releaseId: string, sourceVersion: string): string {
  const staticDir = path.join(tempRoot, "dist");
  cpSync(path.join(repoRoot, "web", "dist"), staticDir, { recursive: true });
  switchStaticRelease(staticDir, releaseId, sourceVersion);
  return staticDir;
}

function rewriteManifestIconForRelease(
  staticDir: string,
  manifestName: string,
  sourceIconPath: string,
  identityPrefix: "public" | "admin",
) {
  const manifestPath = path.join(staticDir, manifestName);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as ManifestPayload;
  const targetIcon = manifest.icons.find((icon) => icon.sizes === "192x192" && !icon.purpose);
  if (!targetIcon) {
    throw new Error(`unable to prepare ${manifestName} release fixture icon`);
  }

  const sourcePath = path.join(staticDir, sourceIconPath);
  const sourceBytes = readFileSync(sourcePath);
  const digest = createHash("sha256").update(sourceBytes).digest("hex").slice(0, 12);
  const nextRelativePath = `pwa/${identityPrefix}-192-${digest}.png`;
  if (targetIcon.src === `/${nextRelativePath}`) {
    throw new Error(`release fixture did not change ${manifestName} icon URL`);
  }

  copyFileSync(sourcePath, path.join(staticDir, nextRelativePath));
  targetIcon.src = `/${nextRelativePath}`;
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}

function listFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory() ? listFiles(entryPath) : [entryPath];
  });
}

function switchStaticRelease(staticDir: string, releaseId: string, sourceVersion: string) {
  writeFileSync(
    path.join(staticDir, "version.json"),
    `${JSON.stringify({ version: releaseId }, null, 2)}\n`,
  );

  let rewroteApplicationVersion = false;
  for (const javascriptPath of listFiles(path.join(staticDir, "assets")).filter((file) => file.endsWith(".js"))) {
    const source = readFileSync(javascriptPath, "utf8");
    const next = source.replaceAll(JSON.stringify(sourceVersion), JSON.stringify(releaseId));
    if (next !== source) {
      rewroteApplicationVersion = true;
      writeFileSync(javascriptPath, next);
    }
  }
  if (!rewroteApplicationVersion) throw new Error(`no application bundle contained ${sourceVersion}`);

  if (releaseId === "release-b") {
    // Reuse the other approved identity's PNG as a deterministic V2 fixture without changing production artwork.
    const publicManifest = JSON.parse(
      readFileSync(path.join(staticDir, "manifest.webmanifest"), "utf8"),
    ) as ManifestPayload;
    const adminManifest = JSON.parse(
      readFileSync(path.join(staticDir, "manifest-admin.webmanifest"), "utf8"),
    ) as ManifestPayload;
    const publicSourceIcon = publicManifest.icons.find((icon) => icon.sizes === "192x192" && !icon.purpose);
    const adminSourceIcon = adminManifest.icons.find((icon) => icon.sizes === "192x192" && !icon.purpose);
    if (!publicSourceIcon || !adminSourceIcon) {
      throw new Error("release fixture manifests are missing regular 192x192 icons");
    }
    rewriteManifestIconForRelease(
      staticDir,
      "manifest.webmanifest",
      adminSourceIcon.src.replace(/^\//, ""),
      "public",
    );
    rewriteManifestIconForRelease(
      staticDir,
      "manifest-admin.webmanifest",
      publicSourceIcon.src.replace(/^\//, ""),
      "admin",
    );
  }
  for (const workerName of ["sw-public.js", "sw-admin.js"]) {
    const workerPath = path.join(staticDir, workerName);
    const source = readFileSync(workerPath, "utf8");
    const withVersion = source.replace(
      /^const BUILD_VERSION = .*;$/m,
      `const BUILD_VERSION = ${JSON.stringify(releaseId)};`,
    );
    const next = withVersion.replace(
      /^const CACHE_NAME = .*;$/m,
      `const CACHE_NAME = ${JSON.stringify(`tavily-hikari-${workerName}-e2e-${releaseId}`)};`,
    );
    if (next === source) throw new Error(`failed to rewrite cache name for ${workerName}`);
    writeFileSync(workerPath, next);
  }
}

function startBackend(
  backendBinary: string,
  repoRoot: string,
  staticDir: string,
  backendPort: number,
  dbPath: string,
): {
  child: ChildProcessWithoutNullStreams;
  stop: () => void;
} {
  const child = spawn(
    backendBinary,
    [
      "--bind",
      "127.0.0.1",
      "--port",
      String(backendPort),
      "--db-path",
      dbPath,
      "--static-dir",
      staticDir,
      "--keys",
      "tvly-pwa-e2e",
      "--admin-auth-builtin-enabled",
      "--admin-auth-builtin-password",
      "pw-e2e-admin",
      "--linuxdo-oauth-enabled",
      "--linuxdo-oauth-client-id",
      "linuxdo-pwa-e2e",
      "--linuxdo-oauth-client-secret",
      "linuxdo-pwa-e2e-secret",
      "--linuxdo-oauth-redirect-url",
      `http://127.0.0.1:${backendPort}/auth/linuxdo/callback`,
      "--linuxdo-oauth-authorize-url",
      `http://127.0.0.1:${backendPort}/__linuxdo/authorize`,
      "--linuxdo-oauth-token-url",
      `http://127.0.0.1:${backendPort}/__linuxdo/token`,
      "--linuxdo-oauth-userinfo-url",
      `http://127.0.0.1:${backendPort}/__linuxdo/user`,
    ],
    {
      cwd: repoRoot,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.stdout.on("data", (chunk) => process.stdout.write(chunk));
  child.stderr.on("data", (chunk) => process.stderr.write(chunk));
  return {
    child,
    stop: () => child.kill("SIGTERM"),
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
    await delay(200);
  }
  throw new Error("backend did not become healthy in time");
}

async function waitForServiceWorker(page: import("playwright-core").Page) {
  await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true), undefined, { timeout: 20_000 });
}

async function waitForController(page: import("playwright-core").Page, scriptName: string) {
  try {
    await page.waitForFunction(
      (expectedScript) => navigator.serviceWorker.controller?.scriptURL.endsWith(expectedScript) === true,
      scriptName,
      { timeout: 20_000 },
    );
  } catch (error) {
    const state = await page.evaluate(async () => ({
      url: location.href,
      controller: navigator.serviceWorker.controller?.scriptURL ?? null,
      registrations: (await navigator.serviceWorker.getRegistrations()).map((registration) => ({
        scope: registration.scope,
        active: registration.active?.scriptURL ?? null,
        activeState: registration.active?.state ?? null,
        waiting: registration.waiting?.scriptURL ?? null,
        waitingState: registration.waiting?.state ?? null,
      })),
    }));
    throw new Error(`${error instanceof Error ? error.message : String(error)}; worker state: ${JSON.stringify(state)}`);
  }
}

async function waitForActiveRegistration(page: import("playwright-core").Page, scope: string) {
  await page.waitForFunction(
    async (expectedScope) => {
      const registration = (await navigator.serviceWorker.getRegistrations()).find(
        (candidate) => new URL(candidate.scope).pathname === expectedScope,
      );
      return registration?.active?.state === "activated" && registration.waiting === null;
    },
    scope,
    { timeout: 20_000 },
  );
}

async function waitForSelector(page: import("playwright-core").Page, selector: string) {
  await page.waitForSelector(selector, {
    state: "attached",
    timeout: 20_000,
  });
}

async function setOffline(page: import("playwright-core").Page, offline: boolean) {
  const client = await page.context().newCDPSession(page);
  await client.send("Network.enable");
  await client.send("Network.emulateNetworkConditions", {
    offline,
    latency: 0,
    downloadThroughput: offline ? 0 : -1,
    uploadThroughput: offline ? 0 : -1,
  });
}

async function assertText(page: import("playwright-core").Page, text: string) {
  await page.waitForFunction(
    (expected) => document.body?.innerText.includes(expected),
    text,
    { timeout: 10_000 },
  );
}

async function assertApplicationBundleVersion(page: import("playwright-core").Page, version: string) {
  const found = await page.evaluate(async (expectedVersion) => {
    const javascriptUrls = [...new Set(
      performance.getEntriesByType("resource")
        .map((entry) => entry.name)
        .filter((url) => {
          const parsed = new URL(url, location.href);
          return parsed.origin === location.origin && parsed.pathname.startsWith("/assets/") && parsed.pathname.endsWith(".js");
        }),
    )];

    for (const url of javascriptUrls) {
      const response = await fetch(url);
      if (response.ok && (await response.text()).includes(JSON.stringify(expectedVersion))) return true;
    }
    return false;
  }, version);

  if (!found) throw new Error(`no loaded JavaScript application bundle contained ${version}`);
}

async function readInstallMetadata(
  page: import("playwright-core").Page,
  manifestPath: string,
): Promise<InstallMetadata> {
  return await page.evaluate(async (url) => {
    const manifestResponse = await fetch(url);
    if (!manifestResponse.ok) {
      throw new Error(`manifest request failed with ${manifestResponse.status}`);
    }
    const manifest = await manifestResponse.json() as ManifestPayload;
    const icon = manifest.icons.find((entry) => entry.sizes === "192x192" && !entry.purpose);
    if (!icon) throw new Error("manifest does not expose a regular 192x192 icon");
    const iconResponse = await fetch(icon.src);
    if (!iconResponse.ok) {
      throw new Error(`icon request failed with ${iconResponse.status}`);
    }
    const iconByteLength = (await iconResponse.arrayBuffer()).byteLength;
    return {
      id: manifest.id,
      scope: manifest.scope,
      startUrl: manifest.start_url,
      iconPath: icon.src,
      iconByteLength,
      manifestCacheControl: manifestResponse.headers.get("cache-control"),
      iconCacheControl: iconResponse.headers.get("cache-control"),
    };
  }, manifestPath);
}

function assertUpdatedInstallMetadata(
  identity: string,
  previous: InstallMetadata,
  current: InstallMetadata,
) {
  for (const [field, expected, actual] of [
    ["id", previous.id, current.id],
    ["scope", previous.scope, current.scope],
    ["start_url", previous.startUrl, current.startUrl],
  ] as const) {
    if (expected !== actual) {
      throw new Error(`${identity} manifest ${field} changed from ${expected} to ${actual}`);
    }
  }
  if (previous.iconPath === current.iconPath) {
    throw new Error(`${identity} V2 manifest kept the V1 icon URL ${current.iconPath}`);
  }
  if (current.iconByteLength <= 0) {
    throw new Error(`${identity} V2 icon response was empty`);
  }
  if (current.manifestCacheControl !== "no-cache, must-revalidate") {
    throw new Error(`${identity} manifest cache policy was ${current.manifestCacheControl}`);
  }
  if (current.iconCacheControl !== "public, max-age=31536000, immutable") {
    throw new Error(`${identity} icon cache policy was ${current.iconCacheControl}`);
  }
}

async function waitForAppUpdate(
  page: import("playwright-core").Page,
  registrationScope: string,
) {
  await page.evaluate(async (scope) => {
    const registration = (await navigator.serviceWorker.getRegistrations()).find(
      (candidate) => new URL(candidate.scope).pathname === scope,
    );
    if (!registration) throw new Error(`${scope} service worker registration missing`);
    await registration.update();
  }, registrationScope);
  await page.waitForSelector(".update-banner", { state: "visible", timeout: 20_000 });
  await page.waitForFunction(
    () => document.querySelector<HTMLButtonElement>(".update-banner-actions button")?.ariaBusy !== "true",
    undefined,
    { timeout: 20_000 },
  );
}

async function activateWaitingAppUpdate(
  page: import("playwright-core").Page,
  scriptName: string,
  identity: string,
) {
  const updateOutcome = await Promise.race([
    page.waitForNavigation({ waitUntil: "domcontentloaded", timeout: 15_000 }).then(() => "reloaded" as const),
    page.waitForSelector(".update-banner-failed", { state: "visible", timeout: 15_000 }).then(() => "failed" as const),
    page.locator(".update-banner-actions button").first().click().then(() => new Promise<never>(() => {})),
  ]);
  if (updateOutcome === "failed") {
    throw new Error(`${identity} app-shell update entered activation-failed instead of reloading`);
  }
  await waitForController(page, scriptName);
}

function seedLinuxDoUserSession(dbPath: string): string {
  const db = new Database(dbPath);
  const now = Math.floor(Date.now() / 1000);
  const userId = "pwa-offline-user";
  const providerUserId = "linuxdo-pwa-offline-user";
  const token = randomBytes(36).toString("base64url");

  db.exec("PRAGMA busy_timeout = 5000");
  db.exec("PRAGMA foreign_keys = ON");

  db.query(
    `INSERT INTO users (id, display_name, username, active, created_at, updated_at, last_login_at)
     VALUES (?, ?, ?, 1, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET
       display_name = excluded.display_name,
       username = excluded.username,
       active = 1,
       updated_at = excluded.updated_at,
       last_login_at = excluded.last_login_at`,
  ).run(userId, "PWA Offline User", "pwa_offline_user", now, now, now);

  db.query(
    `INSERT INTO oauth_accounts (
        provider,
        provider_user_id,
        user_id,
        username,
        name,
        avatar_template,
        active,
        trust_level,
        raw_payload,
        created_at,
        updated_at
      )
      VALUES (?, ?, ?, ?, ?, NULL, 1, ?, NULL, ?, ?)
      ON CONFLICT(provider, provider_user_id) DO UPDATE SET
        user_id = excluded.user_id,
        username = excluded.username,
        name = excluded.name,
        active = 1,
        trust_level = excluded.trust_level,
        updated_at = excluded.updated_at`,
  ).run("linuxdo", providerUserId, userId, "pwa_offline_user", "PWA Offline User", 2, now, now);

  db.query("DELETE FROM user_sessions WHERE user_id = ?").run(userId);
  db.query(
    `INSERT INTO user_sessions (token, user_id, provider, created_at, expires_at, revoked_at)
     VALUES (?, ?, ?, ?, ?, NULL)`,
  ).run(token, userId, "linuxdo", now, now + 3600);

  db.close();
  return token;
}

async function main() {
  const repoRoot = path.resolve(import.meta.dir, "..", "..");
  const tempRoot = mkdtempSync(path.join(tmpdir(), "tavily-hikari-pwa-e2e-"));
  const dbPath = path.join(tempRoot, "pwa-e2e.db");
  const backendPort = await reservePort();
  const baseUrl = `http://127.0.0.1:${backendPort}`;

  const webPackage = JSON.parse(readFileSync(path.join(repoRoot, "web", "package.json"), "utf8")) as { version: string };
  const sourceVersion = process.env.VITE_APP_VERSION?.trim() || webPackage.version;
  ensureBuild(repoRoot);
  const staticDir = stageStaticRelease(repoRoot, tempRoot, "release-a", sourceVersion);
  const backendBinary = ensureBackend(repoRoot);
  const backend = startBackend(backendBinary, repoRoot, staticDir, backendPort, dbPath);
  let browser: import("playwright-core").Browser | null = null;

  try {
    await waitForHealth(baseUrl, backend.child);
    const userSessionToken = seedLinuxDoUserSession(dbPath);

    browser = await launchBrowser();

    const publicContext = await browser.newContext({
      baseURL: baseUrl,
      serviceWorkers: "allow",
    });
    const publicPage = await publicContext.newPage();
    log("opening public home shell");
    await publicPage.goto(`${baseUrl}/`, { waitUntil: "domcontentloaded" });
    await waitForSelector(publicPage, ".public-home");
    await waitForServiceWorker(publicPage);
    await publicPage.reload({ waitUntil: "domcontentloaded" });
    await waitForController(publicPage, "/sw-public.js");
    await assertApplicationBundleVersion(publicPage, "release-a");
    const publicV1Metadata = await readInstallMetadata(publicPage, "/manifest.webmanifest");

    log("verifying the release-a shell remains usable offline before an update");
    await setOffline(publicPage, true);
    await publicPage.reload({ waitUntil: "domcontentloaded" });
    await assertText(publicPage, "Offline shell loaded");
    await assertApplicationBundleVersion(publicPage, "release-a");
    await setOffline(publicPage, false);

    const adminAuthContext = await browser.newContext({
      baseURL: baseUrl,
      serviceWorkers: "allow",
    });
    const adminLoginPage = await adminAuthContext.newPage();
    log("opening admin login");
    await adminLoginPage.goto(`${baseUrl}/login`, { waitUntil: "domcontentloaded" });
    await waitForSelector(adminLoginPage, "#admin-password-input");
    await adminLoginPage.reload({ waitUntil: "domcontentloaded" });
    await adminLoginPage.fill("#admin-password-input", "pw-e2e-admin");
    log("signing into admin shell");
    await adminLoginPage.click("button[type=submit]");
    await adminLoginPage.waitForURL(/\/admin/, { timeout: 15_000 });
    await waitForSelector(adminLoginPage, ".admin-shell-content");
    const adminCookies = await adminAuthContext.cookies();
    await adminAuthContext.close();

    const adminContext = await browser.newContext({
      baseURL: baseUrl,
      serviceWorkers: "allow",
    });
    await adminContext.addCookies(adminCookies);
    const adminPage = await adminContext.newPage();
    await adminPage.goto(`${baseUrl}/admin/`, { waitUntil: "domcontentloaded" });
    await waitForSelector(adminPage, ".admin-shell-content");
    await waitForServiceWorker(adminPage);
    await waitForActiveRegistration(adminPage, "/admin/");
    await adminPage.reload({ waitUntil: "domcontentloaded" });
    await waitForSelector(adminPage, ".admin-shell-content");
    await waitForController(adminPage, "/sw-admin.js");
    const adminRegistrationState = await adminPage.evaluate(async () => {
      const registration = (await navigator.serviceWorker.getRegistrations()).find(
        (candidate) => new URL(candidate.scope).pathname === "/admin/",
      );
      return {
        active: registration?.active?.state ?? null,
        waiting: registration?.waiting?.state ?? null,
        bannerVisible: document.querySelector(".update-banner") !== null,
      };
    });
    if (adminRegistrationState.active !== "activated" || adminRegistrationState.waiting !== null || adminRegistrationState.bannerVisible) {
      throw new Error(`admin first-install lifecycle mismatch: ${JSON.stringify(adminRegistrationState)}`);
    }
    const adminV1Metadata = await readInstallMetadata(adminPage, "/manifest-admin.webmanifest");

    log("verifying the release-a admin shell remains usable offline before an update");
    await setOffline(adminPage, true);
    await adminPage.goto(`${baseUrl}/admin/`, { waitUntil: "domcontentloaded" });
    await assertText(adminPage, "Admin shell loaded offline");
    await setOffline(adminPage, false);
    await waitForController(adminPage, "/sw-admin.js");

    log("switching the same origin to release-b install metadata");
    switchStaticRelease(staticDir, "release-b", "release-a");

    log("verifying the public V1 registration can read V2 manifest and icon before activation");
    await waitForAppUpdate(publicPage, "/");
    const publicV2BeforeActivation = await readInstallMetadata(publicPage, "/manifest.webmanifest");
    assertUpdatedInstallMetadata("public", publicV1Metadata, publicV2BeforeActivation);
    await activateWaitingAppUpdate(publicPage, "/sw-public.js", "public");
    await assertApplicationBundleVersion(publicPage, "release-b");
    const publicV2Metadata = await readInstallMetadata(publicPage, "/manifest.webmanifest");
    assertUpdatedInstallMetadata("public", publicV1Metadata, publicV2Metadata);
    if (publicV2Metadata.iconPath !== publicV2BeforeActivation.iconPath) {
      throw new Error("public V2 icon URL changed while activating the update");
    }
    const publicCacheKeys = await publicPage.evaluate(() => caches.keys());
    if (!publicCacheKeys.some((key) => key.endsWith("e2e-release-b"))) {
      throw new Error(`updated public cache missing: ${publicCacheKeys.join(", ")}`);
    }

    log("verifying offline public shell after the V2 update");
    await setOffline(publicPage, true);
    await publicPage.goto(`${baseUrl}/`, { waitUntil: "domcontentloaded" });
    await assertText(publicPage, "Offline shell loaded");
    log("verifying public identity does not cache admin shell");
    const adminAttempt = await publicPage.goto(`${baseUrl}/admin`, { waitUntil: "domcontentloaded" }).catch(() => null);
    if (adminAttempt && adminAttempt.ok()) {
      const body = await publicPage.textContent("body");
      if (body?.includes("Admin shell loaded offline")) {
        throw new Error("public identity incorrectly served cached admin shell while offline");
      }
    }
    await publicContext.close();

    log("verifying the admin V1 registration can read V2 manifest and icon before activation");
    await waitForAppUpdate(adminPage, "/admin/");
    const adminV2BeforeActivation = await readInstallMetadata(adminPage, "/manifest-admin.webmanifest");
    assertUpdatedInstallMetadata("admin", adminV1Metadata, adminV2BeforeActivation);
    await activateWaitingAppUpdate(adminPage, "/sw-admin.js", "admin");
    await assertApplicationBundleVersion(adminPage, "release-b");
    const adminV2Metadata = await readInstallMetadata(adminPage, "/manifest-admin.webmanifest");
    assertUpdatedInstallMetadata("admin", adminV1Metadata, adminV2Metadata);
    if (adminV2Metadata.iconPath !== adminV2BeforeActivation.iconPath) {
      throw new Error("admin V2 icon URL changed while activating the update");
    }
    const adminCacheKeys = await adminPage.evaluate(() => caches.keys());
    if (!adminCacheKeys.some((key) => key.endsWith("e2e-release-b"))) {
      throw new Error(`updated admin cache missing: ${adminCacheKeys.join(", ")}`);
    }

    log("verifying offline admin shell after the V2 update");
    await setOffline(adminPage, true);
    await adminPage.goto(`${baseUrl}/admin/`, { waitUntil: "domcontentloaded" });
    await assertText(adminPage, "Admin shell loaded offline");
    await adminContext.close();

    const userContext = await browser.newContext({
      baseURL: baseUrl,
      serviceWorkers: "allow",
    });
    await userContext.addCookies([
      {
        name: USER_SESSION_COOKIE_NAME,
        value: userSessionToken,
        url: baseUrl,
      },
    ]);
    const userConsolePage = await userContext.newPage();
    log("opening user console shell");
    await userConsolePage.goto(`${baseUrl}/console`, { waitUntil: "domcontentloaded" });
    await waitForSelector(userConsolePage, ".user-console-shell");
    await waitForServiceWorker(userConsolePage);
    log("verifying offline console shell");
    await setOffline(userConsolePage, true);
    await userConsolePage.goto(`${baseUrl}/console`, { waitUntil: "domcontentloaded" });
    await assertText(userConsolePage, "Console structure is available");
    await userContext.close();

    log("PWA offline browser E2E passed");
  } finally {
    if (browser) await browser.close().catch(() => {});
    backend.stop();
    rmSync(tempRoot, { recursive: true, force: true });
  }
}

await main();
