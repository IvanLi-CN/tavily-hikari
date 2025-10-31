use std::net::SocketAddr;

use axum::http::header::{CONNECTION, CONTENT_LENGTH, TRANSFER_ENCODING};
use axum::{
    Router,
    body::{self, Body},
    extract::State,
    http::{HeaderMap, Method, Request, Response, StatusCode},
    routing::any,
};
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};
use tavily_hikari::{ProxyRequest, ProxyResponse, TavilyProxy};

#[derive(Clone)]
struct AppState {
    proxy: TavilyProxy,
}

pub async fn serve(addr: SocketAddr, proxy: TavilyProxy) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState { proxy };

    let app = Router::new()
        .route("/*path", any(proxy_handler))
        .with_state(state);

    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

const BODY_LIMIT: usize = 16 * 1024 * 1024; // 16 MiB 默认限制

async fn proxy_handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    let query = parts.uri.query().map(|q| q.to_owned());

    if method == Method::GET && accepts_event_stream(&parts.headers) {
        let response = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(response);
    }

    let headers = clone_headers(&parts.headers);
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let proxy_request = ProxyRequest {
        method,
        path,
        query,
        headers,
        body: body_bytes.clone(),
    };

    match state.proxy.proxy_request(proxy_request).await {
        Ok(resp) => Ok(build_response(resp)),
        Err(err) => {
            eprintln!("proxy error: {err}");
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

fn clone_headers(headers: &HeaderMap) -> ReqHeaderMap {
    let mut map = ReqHeaderMap::new();
    for (name, value) in headers.iter() {
        if let Ok(cloned) = ReqHeaderValue::from_bytes(value.as_bytes()) {
            map.insert(name.clone(), cloned);
        }
    }
    map
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        })
        .unwrap_or(false)
}

fn build_response(resp: ProxyResponse) -> Response<Body> {
    let mut builder = Response::builder().status(resp.status);
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in resp.headers.iter() {
            if name == TRANSFER_ENCODING || name == CONNECTION || name == CONTENT_LENGTH {
                continue;
            }
            headers.append(name.clone(), value.clone());
        }
        headers.insert(CONTENT_LENGTH, value_from_len(resp.body.len()));
    }
    builder
        .body(Body::from(resp.body))
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
}

fn value_from_len(len: usize) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(len.to_string().as_str())
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("0"))
}
