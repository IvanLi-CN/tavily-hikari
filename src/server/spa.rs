// kept for potential future direct serving; currently ServeDir handles '/'
#[allow(dead_code)]
async fn load_spa_response(
    state: &AppState,
    file_name: &str,
) -> Result<Response<Body>, StatusCode> {
    let Some(dir) = state.static_dir.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let path = dir.join(file_name);
    let Ok(bytes) = tokio::fs::read(path).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn spa_file_exists(state: &AppState, file_name: &str) -> bool {
    let Some(dir) = state.static_dir.as_ref() else {
        return false;
    };
    tokio::fs::metadata(dir.join(file_name)).await.is_ok()
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

const BASE_404_STYLES: &str = r#"
  :root {
    color-scheme: light;
    font-family: 'Inter', 'SF Pro Display', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    text-rendering: optimizeLegibility;
  }

  body {
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: radial-gradient(circle at top left, rgba(99, 102, 241, 0.12), transparent 45%),
      radial-gradient(circle at bottom right, rgba(236, 72, 153, 0.12), transparent 50%),
      #f5f6fb;
    color: #1f2937;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color-scheme: dark;
    }
    body {
      background: radial-gradient(circle at top left, rgba(129, 140, 248, 0.22), transparent 45%),
        radial-gradient(circle at bottom right, rgba(236, 72, 153, 0.18), transparent 50%),
        #0f172a;
      color: #e2e8f0;
    }
  }

  .not-found-shell {
    max-width: 520px;
    width: calc(100% - 48px);
    padding: 48px 40px;
    border-radius: 28px;
    background: rgba(255, 255, 255, 0.82);
    border: 1px solid rgba(15, 23, 42, 0.08);
    backdrop-filter: blur(18px);
    box-shadow: 0 28px 65px rgba(15, 23, 42, 0.12);
    text-align: center;
  }

  @media (prefers-color-scheme: dark) {
    .not-found-shell {
      background: rgba(15, 23, 42, 0.7);
      border: 1px solid rgba(148, 163, 184, 0.18);
      box-shadow: 0 32px 65px rgba(15, 23, 42, 0.5);
    }
  }

  .not-found-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 16px;
    border-radius: 999px;
    background: rgba(99, 102, 241, 0.16);
    color: #4338ca;
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
    color: #4f46e5;
  }

  @media (prefers-color-scheme: dark) {
    .not-found-code {
      color: #a5b4fc;
    }
  }

  .not-found-title {
    margin: 0;
    font-size: clamp(1.5rem, 4vw, 2.25rem);
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .not-found-description {
    margin: 20px 0 30px;
    color: rgba(100, 116, 139, 0.95);
    font-size: 1rem;
    line-height: 1.7;
  }

  @media (prefers-color-scheme: dark) {
    .not-found-description {
      color: rgba(203, 213, 225, 0.82);
    }
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
    font-weight: 600;
    color: #fff;
    background: linear-gradient(135deg, #6366f1, #8b5cf6);
    box-shadow: 0 16px 35px rgba(99, 102, 241, 0.35);
    text-decoration: none;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .not-found-primary:hover {
    transform: translateY(-1px);
    box-shadow: 0 20px 40px rgba(99, 102, 241, 0.4);
  }

  .not-found-footer {
    margin-top: 36px;
    font-size: 0.85rem;
    color: rgba(100, 116, 139, 0.75);
  }

  @media (prefers-color-scheme: dark) {
    .not-found-footer {
      color: rgba(148, 163, 184, 0.78);
    }
  }
"#;

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
    style_block.push_str(BASE_404_STYLES);
    if let Some(content) = css_content {
        style_block.push_str(content);
    }
    style_block.push_str("\n</style>\n");
    // Safer: pass original path via data attribute and read it in script without string concatenation
    let script = format!(
        "<script data-p=\"{}\">!function(){{try{{var s=document.currentScript;var p=s&&s.getAttribute('data-p')||'/';history.replaceState(null,'', p)}}catch(_e){{}}}}()</script>",
        html_escape::encode_double_quoted_attribute(&original)
    );
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"UTF-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n    <title>404 Not Found</title>\n    {}  </head>\n  <body>\n    <main class=\"not-found-shell\" role=\"main\">\n      <span class=\"not-found-badge\" aria-hidden=\"true\">Tavily Hikari Proxy</span>\n      <p class=\"not-found-code\">404</p>\n      <h1 class=\"not-found-title\">Page not found</h1>\n      <p class=\"not-found-description\">The page you’re trying to visit, <code>{}</code>, isn’t available right now.</p>\n      <div class=\"not-found-actions\">\n        <a href=\"/\" class=\"not-found-primary\" aria-label=\"Back to dashboard\">Return to dashboard</a>\n      </div>\n      <p class=\"not-found-footer\">Error reference: 404</p>\n    </main>\n    {}\n  </body>\n</html>",
        style_block,
        html_escape::encode_text(&original),
        script
    )
}
