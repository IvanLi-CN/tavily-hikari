#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from PIL import Image


WEB_ROOT = Path(__file__).resolve().parent.parent
DIST_DIR = WEB_ROOT / "dist"
VITE_MANIFEST_PATH = DIST_DIR / ".vite" / "manifest.json"
ICON_SIZES = (64, 96, 128, 144, 152, 167, 180, 192, 256, 384, 512, 1024)
MASKABLE_SIZES = (192, 512)

PUBLIC_HTML_FILES = {
    "index.html",
    "console.html",
    "login.html",
    "registration-paused.html",
}
ADMIN_HTML_FILES = {"admin.html"}


def normalize(values: list[str]) -> list[str]:
    return sorted({value for value in values if value})


def collect_assets(manifest: dict[str, Any], html_files: set[str]) -> list[str]:
    files: set[str] = set(html_files)
    visited: set[str] = set()

    def visit(key: str) -> None:
        entry = manifest.get(key)
        if not isinstance(entry, dict):
            return
        file_name = entry.get("file")
        if isinstance(file_name, str) and file_name:
            if file_name in visited:
                return
            visited.add(file_name)
            files.add(file_name)
        for css_file in entry.get("css", []):
            if isinstance(css_file, str):
                files.add(css_file)
        for asset_file in entry.get("assets", []):
            if isinstance(asset_file, str):
                files.add(asset_file)
        for import_key in entry.get("imports", []):
            if isinstance(import_key, str):
                visit(import_key)
        for import_key in entry.get("dynamicImports", []):
            if isinstance(import_key, str):
                visit(import_key)

    for html_file in html_files:
        visit(html_file)

    return normalize(list(files))


def ensure_not_empty(name: str, values: list[str]) -> None:
    if not values:
        raise RuntimeError(f"PWA asset graph '{name}' is empty")


def draw_icon(prefix: str, source_file: Path) -> dict[str, dict[str, str] | str]:
    base = Image.open(source_file).convert("RGBA")
    output_any: dict[str, str] = {}
    output_maskable: dict[str, str] = {}
    pwa_dir = DIST_DIR / "pwa"
    pwa_dir.mkdir(parents=True, exist_ok=True)
    for size in ICON_SIZES:
        rel = f"pwa/{prefix}-{size}.png"
        base.resize((size, size), Image.Resampling.LANCZOS).save(DIST_DIR / rel)
        output_any[str(size)] = rel
    for size in MASKABLE_SIZES:
        rel = f"pwa/{prefix}-maskable-{size}.png"
        base.resize((size, size), Image.Resampling.LANCZOS).save(DIST_DIR / rel)
        output_maskable[str(size)] = rel
    touch_rel = f"pwa/{prefix}-touch-icon.png"
    base.resize((180, 180), Image.Resampling.LANCZOS).save(DIST_DIR / touch_rel)
    return {
        "any": output_any,
        "maskable": output_maskable,
        "touch": touch_rel,
    }


def hash_cache_key(values: list[str]) -> str:
    return hashlib.sha256("|".join(values).encode("utf-8")).hexdigest()[:12]


def collect_icon_files(icons: dict[str, dict[str, str] | str]) -> list[str]:
    output: list[str] = []
    any_icons = icons.get("any")
    maskable_icons = icons.get("maskable")
    touch_icon = icons.get("touch")
    if isinstance(any_icons, dict):
        output.extend(any_icons.values())
    if isinstance(maskable_icons, dict):
        output.extend(maskable_icons.values())
    if isinstance(touch_icon, str):
        output.append(touch_icon)
    return normalize(output)


def make_manifest(
    name: str,
    short_name: str,
    start_url: str,
    scope: str,
    theme_color: str,
    background_color: str,
    icons: dict[str, dict[str, str] | str],
) -> dict[str, Any]:
    icon_entries: list[dict[str, str]] = []
    any_icons = icons["any"]
    maskable_icons = icons["maskable"]
    if not isinstance(any_icons, dict) or not isinstance(maskable_icons, dict):
        raise RuntimeError("PWA icon export graph is malformed")
    for size in ICON_SIZES:
        rel = any_icons[str(size)]
        icon_entries.append(
            {
                "src": f"/{rel}",
                "sizes": f"{size}x{size}",
                "type": "image/png",
            }
        )
    for size in MASKABLE_SIZES:
        rel = maskable_icons[str(size)]
        icon_entries.append(
            {
                "src": f"/{rel}",
                "sizes": f"{size}x{size}",
                "type": "image/png",
                "purpose": "maskable",
            }
        )
    return {
        "name": name,
        "short_name": short_name,
        "start_url": start_url,
        "scope": scope,
        "display": "standalone",
        "theme_color": theme_color,
        "background_color": background_color,
        "icons": icon_entries,
    }


def make_service_worker(cache_name: str, files: list[str], offline_fallbacks: dict[str, str], reject_admin: bool) -> str:
    precache_urls = [f"/{file_name}" for file_name in files]
    return f"""const CACHE_NAME = {json.dumps(cache_name)};
const PRECACHE_URLS = {json.dumps(precache_urls, indent=2)};
const OFFLINE_FALLBACKS = {json.dumps(offline_fallbacks, indent=2)};
const ACTIVATE_UPDATE_MESSAGE = 'TAVILY_HIKARI_ACTIVATE_UPDATE';

self.addEventListener('install', (event) => {{
  event.waitUntil((async () => {{
    const cache = await caches.open(CACHE_NAME);
    await cache.addAll(PRECACHE_URLS);
  }})());
}});

self.addEventListener('message', (event) => {{
  if (event.data && event.data.type === ACTIVATE_UPDATE_MESSAGE) {{
    event.waitUntil(self.skipWaiting());
  }}
}});

self.addEventListener('activate', (event) => {{
  event.waitUntil((async () => {{
    const keys = await caches.keys();
    await Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key)));
    await self.clients.claim();
  }})());
}});

function isNetworkOnly(request, requestUrl) {{
  if (request.method !== 'GET') return true;
  if (requestUrl.pathname.startsWith('/api/')) return true;
  if (requestUrl.pathname === '/mcp' || requestUrl.pathname.startsWith('/mcp/')) return true;
  if (requestUrl.pathname.startsWith('/health')) return true;
  if (requestUrl.pathname.startsWith('/auth/')) return true;
  return false;
}}

function isPrecached(requestUrl) {{
  return PRECACHE_URLS.includes(requestUrl.pathname);
}}

function networkFailureResponse() {{
  return new Response(null, {{ status: 503, statusText: 'Service Unavailable' }});
}}

async function fetchWithNetworkFallback(request) {{
  try {{
    return await fetch(request);
  }} catch {{
    return networkFailureResponse();
  }}
}}

function resolveOfflineFallback(pathname) {{
  if ({'pathname === "/admin" || pathname.startsWith("/admin/")' if reject_admin else 'false'}) {{
    return null;
  }}
  for (const [prefix, fallbackUrl] of Object.entries(OFFLINE_FALLBACKS)) {{
    if (pathname === prefix || pathname.startsWith(prefix)) {{
      return fallbackUrl;
    }}
  }}
  return null;
}}

self.addEventListener('fetch', (event) => {{
  const request = event.request;
  const requestUrl = new URL(request.url);
  if (requestUrl.origin !== self.location.origin) return;

  if (isNetworkOnly(request, requestUrl)) {{
    event.respondWith(fetchWithNetworkFallback(request));
    return;
  }}

  if (request.mode === 'navigate' || request.destination === 'document') {{
    event.respondWith((async () => {{
      try {{
        return await fetch(request);
      }} catch (error) {{
        const cache = await caches.open(CACHE_NAME);
        const fallbackUrl = resolveOfflineFallback(requestUrl.pathname);
        if (fallbackUrl) {{
          const fallbackResponse = await cache.match(fallbackUrl);
          if (fallbackResponse) return fallbackResponse;
        }}
        throw error;
      }}
    }})());
    return;
  }}

  if (!isPrecached(requestUrl)) {{
    event.respondWith(fetchWithNetworkFallback(request));
    return;
  }}

  event.respondWith((async () => {{
    const cache = await caches.open(CACHE_NAME);
    const cached = await cache.match(requestUrl.pathname);
    if (cached) return cached;
    const response = await fetch(request);
    if (response.ok) {{
      cache.put(requestUrl.pathname, response.clone()).catch(() => {{}});
    }}
    return response;
  }})());
}});"""


def write_json(relative_path: str, value: Any) -> None:
    absolute_path = DIST_DIR / relative_path
    absolute_path.parent.mkdir(parents=True, exist_ok=True)
    absolute_path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def write_text(relative_path: str, value: str) -> None:
    absolute_path = DIST_DIR / relative_path
    absolute_path.parent.mkdir(parents=True, exist_ok=True)
    absolute_path.write_text(value, encoding="utf-8")


def main() -> None:
    manifest = json.loads(VITE_MANIFEST_PATH.read_text(encoding="utf-8"))
    public_files = collect_assets(manifest, PUBLIC_HTML_FILES)
    admin_files = collect_assets(manifest, ADMIN_HTML_FILES)
    ensure_not_empty("public", public_files)
    ensure_not_empty("admin", admin_files)

    public_source_icon = WEB_ROOT / "public" / "assets" / "relay-mesh-icon-light.png"
    admin_source_icon = WEB_ROOT / "public" / "assets" / "relay-mesh-icon-dark.png"
    public_icons = draw_icon(prefix="public", source_file=public_source_icon)
    admin_icons = draw_icon(prefix="admin", source_file=admin_source_icon)
    public_icon_files = collect_icon_files(public_icons)
    admin_icon_files = collect_icon_files(admin_icons)
    public_precache_files = normalize(public_files + public_icon_files)
    admin_precache_files = normalize(admin_files + admin_icon_files)

    write_json(
        "manifest.webmanifest",
        make_manifest(
            name="Tavily Hikari",
            short_name="Hikari",
            start_url="/",
            scope="/",
            theme_color="#7c3aed",
            background_color="#f4f1fa",
            icons=public_icons,
        ),
    )
    write_json(
        "manifest-admin.webmanifest",
        make_manifest(
            name="Tavily Hikari Admin",
            short_name="Hikari Admin",
            start_url="/admin/",
            scope="/admin/",
            theme_color="#0ea5e9",
            background_color="#eef1fa",
            icons=admin_icons,
        ),
    )

    write_text(
        "sw-public.js",
        make_service_worker(
            cache_name=f"tavily-hikari-public-{hash_cache_key(public_precache_files)}",
            files=public_precache_files,
            offline_fallbacks={
                "/console": "/console.html",
                "/login": "/login.html",
                "/registration-paused": "/registration-paused.html",
                "/": "/index.html",
            },
            reject_admin=True,
        ),
    )
    write_text(
        "sw-admin.js",
        make_service_worker(
            cache_name=f"tavily-hikari-admin-{hash_cache_key(admin_precache_files)}",
            files=admin_precache_files,
            offline_fallbacks={"/admin/": "/admin.html", "/admin": "/admin.html"},
            reject_admin=False,
        ),
    )

    write_json(
        "pwa/asset-graphs.json",
        {
            "generatedAt": "build-time",
            "public": {
                "manifest": "manifest.webmanifest",
                "serviceWorker": "sw-public.js",
                "files": public_files,
                "precacheFiles": public_precache_files,
                "icons": public_icons,
            },
            "admin": {
                "manifest": "manifest-admin.webmanifest",
                "serviceWorker": "sw-admin.js",
                "files": admin_files,
                "precacheFiles": admin_precache_files,
                "icons": admin_icons,
            },
        },
    )
    print(f"[pwa] generated split PWA assets in {DIST_DIR}")


if __name__ == "__main__":
    main()
