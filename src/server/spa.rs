fn response_content_type(file_name: &str) -> String {
    let guess = mime_guess::from_path(file_name).first_or_octet_stream();
    if guess.essence_str() == "text/html" {
        "text/html; charset=utf-8".to_string()
    } else {
        guess.to_string()
    }
}

fn embedded_static_bytes(file_name: &str) -> Option<Bytes> {
    tavily_hikari::web_assets::embedded_bytes(file_name).map(Bytes::from_static)
}

async fn read_static_bytes(state: &AppState, file_name: &str) -> Option<Bytes> {
    if let Some(dir) = state.static_dir.as_ref() {
        let path = dir.join(file_name);
        if let Ok(bytes) = tokio::fs::read(path).await {
            return Some(Bytes::from(bytes));
        }
    }

    embedded_static_bytes(file_name)
}

async fn load_spa_response(state: &AppState, file_name: &str) -> Result<Response<Body>, StatusCode> {
    let Some(bytes) = read_static_bytes(state, file_name).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, response_content_type(file_name))
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn spa_file_exists(state: &AppState, file_name: &str) -> bool {
    if let Some(dir) = state.static_dir.as_ref()
        && tokio::fs::metadata(dir.join(file_name)).await.is_ok()
    {
        return true;
    }

    embedded_static_bytes(file_name).is_some()
}

fn is_safe_static_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains("..")
        && FsPath::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

async fn serve_asset(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let asset_path = path.trim_start_matches('/');
    if !is_safe_static_path(asset_path) {
        return Err(StatusCode::NOT_FOUND);
    }

    let file_name = format!("assets/{asset_path}");
    let Some(bytes) = read_static_bytes(state.as_ref(), &file_name).await else {
        return Err(StatusCode::NOT_FOUND);
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, response_content_type(&file_name))
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn serve_assets_root() -> Result<Response<Body>, StatusCode> {
    Err(StatusCode::NOT_FOUND)
}

async fn serve_favicon(State(state): State<Arc<AppState>>) -> Result<Response<Body>, StatusCode> {
    load_spa_response(state.as_ref(), "favicon.svg").await
}

async fn serve_linuxdo_logo(
    State(state): State<Arc<AppState>>,
) -> Result<Response<Body>, StatusCode> {
    load_spa_response(state.as_ref(), "linuxdo-logo.svg").await
}

async fn serve_version_json(State(state): State<Arc<AppState>>) -> Result<Response<Body>, StatusCode> {
    load_spa_response(state.as_ref(), "version.json").await
}

async fn serve_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // Only auto-redirect to admin when explicit dev convenience flag is enabled.
    // Admin users should still be able to access the public page without forced redirection.
    if state.dev_open_admin {
        return Ok(Redirect::temporary("/admin").into_response());
    }

    if state.linuxdo_oauth.is_enabled_and_configured()
        && resolve_user_session(state.as_ref(), &headers)
            .await
            .is_some()
        && spa_file_exists(state.as_ref(), "console.html").await
    {
        return Ok(Redirect::temporary("/console").into_response());
    }

    load_spa_response(state.as_ref(), "index.html").await
}

async fn serve_admin_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if is_admin_request(state.as_ref(), &headers) {
        return load_spa_response(state.as_ref(), "admin.html").await;
    }
    if state.builtin_admin.is_enabled() {
        return Ok(Redirect::temporary("/login").into_response());
    }
    Err(StatusCode::FORBIDDEN)
}

async fn serve_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.builtin_admin.is_enabled() {
        return Err(StatusCode::NOT_FOUND);
    }
    if is_admin_request(state.as_ref(), &headers) {
        return Ok(Redirect::temporary("/admin").into_response());
    }
    load_spa_response(state.as_ref(), "login.html").await
}

async fn serve_registration_paused_index(
    State(state): State<Arc<AppState>>,
) -> Result<Response<Body>, StatusCode> {
    if !spa_file_exists(state.as_ref(), "registration-paused.html").await {
        return load_spa_response(state.as_ref(), "index.html").await;
    }
    load_spa_response(state.as_ref(), "registration-paused.html").await
}

async fn serve_console_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return load_spa_response(state.as_ref(), "console.html").await;
    }
    if resolve_user_session(state.as_ref(), &headers)
        .await
        .is_none()
    {
        return Ok(Redirect::temporary("/").into_response());
    }
    load_spa_response(state.as_ref(), "console.html").await
}

const THEME_STORAGE_KEY: &str = "tavily-hikari-theme-mode";

const BASE_404_STYLES: &str = r#"
  :root {
    --not-found-background: var(--background, 210 40% 98%);
    --not-found-foreground: var(--foreground, 222 47% 11%);
    --not-found-card: var(--card, 0 0% 100%);
    --not-found-primary: var(--primary, 221 72% 43%);
    --not-found-primary-foreground: var(--primary-foreground, 210 100% 98%);
    --not-found-secondary: var(--secondary, 217 91% 60%);
    --not-found-muted: var(--muted, 210 32% 94%);
    --not-found-muted-foreground: var(--muted-foreground, 215 20% 36%);
    --not-found-border: var(--border, 214 32% 86%);
    color-scheme: light;
    font-family: 'IBM Plex Sans', 'Noto Sans SC', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    text-rendering: optimizeLegibility;
  }

  html.not-found-page.dark,
  html.not-found-page[data-theme-mode='dark'] {
    --not-found-background: var(--background, 222 47% 8%);
    --not-found-foreground: var(--foreground, 210 40% 96%);
    --not-found-card: var(--card, 222 40% 12%);
    --not-found-primary: var(--primary, 217 91% 68%);
    --not-found-primary-foreground: var(--primary-foreground, 222 47% 8%);
    --not-found-secondary: var(--secondary, 211 89% 64%);
    --not-found-muted: var(--muted, 217 25% 18%);
    --not-found-muted-foreground: var(--muted-foreground, 215 18% 68%);
    --not-found-border: var(--border, 217 22% 26%);
    color-scheme: dark;
  }

  @media (prefers-color-scheme: dark) {
    html.not-found-page:not([data-theme-mode='light']):not(.dark) {
      --not-found-background: var(--background, 222 47% 8%);
      --not-found-foreground: var(--foreground, 210 40% 96%);
      --not-found-card: var(--card, 222 40% 12%);
      --not-found-primary: var(--primary, 217 91% 68%);
      --not-found-primary-foreground: var(--primary-foreground, 222 47% 8%);
      --not-found-secondary: var(--secondary, 211 89% 64%);
      --not-found-muted: var(--muted, 217 25% 18%);
      --not-found-muted-foreground: var(--muted-foreground, 215 18% 68%);
      --not-found-border: var(--border, 217 22% 26%);
      color-scheme: dark;
    }
  }

  html.not-found-page,
  body.not-found-page-body {
    min-height: 100%;
  }

  body.not-found-page-body,
  .not-found-page-body {
    margin: 0;
    min-height: 100vh;
    min-height: 100dvh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background:
      radial-gradient(1000px 520px at 6% -8%, hsl(var(--not-found-primary) / 0.18), transparent 62%),
      radial-gradient(900px 460px at 95% -14%, hsl(var(--not-found-secondary) / 0.16), transparent 64%),
      linear-gradient(
        180deg,
        hsl(var(--not-found-background)) 0%,
        hsl(var(--not-found-background)) 62%,
        hsl(var(--not-found-muted) / 0.6) 100%
      ),
      hsl(var(--not-found-background));
    color: hsl(var(--not-found-foreground));
  }

  .not-found-shell {
    max-width: 520px;
    width: min(100%, 520px);
    padding: clamp(32px, 5vw, 48px) clamp(24px, 5vw, 40px);
    border-radius: 28px;
    background: hsl(var(--not-found-card) / 0.82);
    border: 1px solid hsl(var(--not-found-border) / 0.72);
    backdrop-filter: blur(18px);
    box-shadow: 0 28px 65px hsl(222 47% 11% / 0.16);
    text-align: center;
    color: hsl(var(--not-found-foreground));
  }

  .dark .not-found-shell {
    box-shadow: 0 32px 65px hsl(222 47% 4% / 0.54);
  }

  .not-found-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 16px;
    border-radius: 999px;
    background: hsl(var(--not-found-primary) / 0.16);
    color: hsl(var(--not-found-primary));
    font-size: 0.85rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .not-found-code {
    margin: 28px 0 12px;
    font-size: clamp(4rem, 13vw, 6rem);
    font-weight: 800;
    line-height: 1;
    letter-spacing: -0.04em;
    color: hsl(var(--not-found-primary));
  }

  .not-found-title {
    margin: 0;
    color: hsl(var(--not-found-foreground));
    font-size: clamp(1.5rem, 4vw, 2.25rem);
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .not-found-description {
    margin: 20px 0 30px;
    color: hsl(var(--not-found-muted-foreground));
    font-size: 1rem;
    line-height: 1.7;
  }

  .not-found-description code {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid hsl(var(--not-found-border) / 0.9);
    background: hsl(var(--not-found-muted) / 0.82);
    color: hsl(var(--not-found-foreground));
    font-size: 0.96em;
  }

  .not-found-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 28px;
  }

  .not-found-primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 12px 22px;
    border-radius: 999px;
    border: 1px solid transparent;
    font-weight: 600;
    color: hsl(var(--not-found-primary-foreground));
    background: linear-gradient(135deg, hsl(var(--not-found-primary)), hsl(var(--not-found-secondary)));
    box-shadow: 0 16px 35px hsl(var(--not-found-primary) / 0.35);
    text-decoration: none;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .not-found-primary:hover {
    transform: translateY(-1px);
    box-shadow: 0 20px 40px hsl(var(--not-found-primary) / 0.42);
  }

  .not-found-footer {
    margin-top: 36px;
    font-size: 0.85rem;
    color: hsl(var(--not-found-muted-foreground) / 0.9);
  }
"#;

fn build_404_theme_bootstrap() -> String {
    format!(
        r#"<script>!function(){{try{{var root=document.documentElement;var media=window.matchMedia?window.matchMedia('(prefers-color-scheme: dark)'):null;var mode='system';var apply=function(resolved){{root.classList.toggle('dark',resolved==='dark');root.style.colorScheme=resolved;}};try{{var stored=window.localStorage.getItem('{storage_key}');if(stored==='light'||stored==='dark'||stored==='system')mode=stored;}}catch(_e){{}}var resolve=function(){{return mode==='system'?(media&&media.matches?'dark':'light'):mode;}};root.setAttribute('data-theme-mode',mode);apply(resolve());if(mode==='system'&&media){{var onChange=function(){{apply(media.matches?'dark':'light');}};if(typeof media.addEventListener==='function'){{media.addEventListener('change',onChange);}}else if(typeof media.addListener==='function'){{media.addListener(onChange);}}}}}}catch(_e){{}}}}()</script>"#,
        storage_key = THEME_STORAGE_KEY
    )
}

fn find_frontend_css_href(static_dir: Option<&FsPath>) -> Option<String> {
    let dir = static_dir?;
    let index_path = dir.join("index.html");
    let mut s = String::new();
    if fs::File::open(&index_path)
        .ok()?
        .read_to_string(&mut s)
        .is_ok()
    {
        // naive scan for first stylesheet href
        if let Some(idx) = s.find("rel=\"stylesheet\"") {
            let frag = &s[idx..];
            if let Some(href_idx) = frag.find("href=\"") {
                let frag2 = &frag[href_idx + 6..];
                if let Some(end_idx) = frag2.find('\"') {
                    let href = &frag2[..end_idx];
                    return Some(href.to_string());
                }
            }
        }
    }
    None
}

fn load_frontend_css_content(static_dir: Option<&FsPath>) -> Option<String> {
    let dir = static_dir?;
    let href = find_frontend_css_href(Some(dir))?;
    // href like "/assets/index-xxxx.css" => remove leading slash and read from static_dir root
    let rel = href.trim_start_matches('/');
    let path = dir.join(
        rel.strip_prefix("assets/")
            .map(|s| FsPath::new("assets").join(s))
            .unwrap_or_else(|| FsPath::new(rel).to_path_buf()),
    );
    fs::read_to_string(path).ok()
}

#[derive(Deserialize)]
struct FallbackQuery {
    path: Option<String>,
}

async fn not_found_landing(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FallbackQuery>,
) -> Response<Body> {
    let css = load_frontend_css_content(state.static_dir.as_deref());
    let html = build_404_landing_inline(css.as_deref(), q.path.unwrap_or_else(|| "/".to_string()));
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CONTENT_LENGTH, html.len().to_string())
        .body(Body::from(html))
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
}

fn build_404_landing_inline(css_content: Option<&str>, original: String) -> String {
    let mut style_block = String::from("<style>\n");
    if let Some(content) = css_content {
        style_block.push_str(content);
        style_block.push('\n');
    }
    style_block.push_str(BASE_404_STYLES);
    style_block.push_str("\n</style>\n");
    let theme_bootstrap = build_404_theme_bootstrap();
    // Safer: pass original path via data attribute and read it in script without string concatenation
    let script = format!(
        "<script data-p=\"{}\">!function(){{try{{var s=document.currentScript;var p=s&&s.getAttribute('data-p')||'/';history.replaceState(null,'', p)}}catch(_e){{}}}}()</script>",
        html_escape::encode_double_quoted_attribute(&original)
    );
    format!(
        "<!doctype html>\n<html lang=\"en\" class=\"not-found-page\">\n  <head>\n    <meta charset=\"UTF-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n    <meta name=\"color-scheme\" content=\"light dark\" />\n    <title>404 Not Found</title>\n    {}\n    {}  </head>\n  <body class=\"not-found-page-body\">\n    <main class=\"not-found-shell\" role=\"main\">\n      <span class=\"not-found-badge\" aria-hidden=\"true\">Tavily Hikari Proxy</span>\n      <p class=\"not-found-code\">404</p>\n      <h1 class=\"not-found-title\">Page not found</h1>\n      <p class=\"not-found-description\">The page you’re trying to visit, <code>{}</code>, isn’t available right now.</p>\n      <div class=\"not-found-actions\">\n        <a href=\"/\" class=\"not-found-primary\" aria-label=\"Back to dashboard\">Return to dashboard</a>\n      </div>\n      <p class=\"not-found-footer\">Error reference: 404</p>\n    </main>\n    {}\n  </body>\n</html>",
        theme_bootstrap,
        style_block,
        html_escape::encode_text(&original),
        script
    )
}

#[cfg(test)]
mod spa_404_tests {
    use super::{THEME_STORAGE_KEY, build_404_landing_inline};

    #[test]
    fn build_404_landing_inline_bootstraps_theme_and_keeps_404_overrides_last() {
        let frontend_css = "body{background:#f5f6fb;color:#1f2937}.dark body{background:#0f172a;color:#e2e8f0}";
        let html = build_404_landing_inline(Some(frontend_css), "/accounts?view=all".to_string());

        let frontend_index = html
            .find(frontend_css)
            .expect("frontend css should be inlined before fallback overrides");
        let override_index = html
            .rfind(".not-found-page-body")
            .expect("fallback styles should include the not-found page body override");

        assert!(
            frontend_index < override_index,
            "frontend css must appear before 404 override styles so the fallback page wins the cascade"
        );
        assert!(html.contains(&format!("localStorage.getItem('{THEME_STORAGE_KEY}')")));
        assert!(html.contains("root.setAttribute('data-theme-mode',mode)"));
        assert!(html.contains("@media (prefers-color-scheme: dark)"));
        assert!(html.contains("html.not-found-page:not([data-theme-mode='light']):not(.dark)"));
        assert!(html.contains("media.addEventListener('change',onChange)"));
        assert!(html.contains("body.not-found-page-body,"));
        assert!(html.contains("<html lang=\"en\" class=\"not-found-page\">"));
        assert!(html.contains("<body class=\"not-found-page-body\">"));
        assert!(html.contains("history.replaceState(null,'', p)"));
        assert!(html.contains("/accounts?view=all"));
    }

    #[test]
    fn build_404_landing_inline_escapes_original_path_in_text_and_script() {
        let html = build_404_landing_inline(None, "/accounts?<script>alert(1)</script>&\"".to_string());

        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("data-p=\"/accounts?&lt;script&gt;alert(1)&lt;/script&gt;&amp;&quot;\""));
        assert!(html.contains("Return to dashboard"));
    }
}
