use tavily_hikari::McpSessionBinding;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaResearchRequestLookup {
    pub key_id: String,
    pub token_id: String,
    pub expires_at: i64,
}

async fn lookup_active_mcp_session_local_or_peer(
    state: &Arc<AppState>,
    proxy_session_id: &str,
) -> Result<Option<McpSessionBinding>, ProxyError> {
    if let Some(session) = state.proxy.get_active_mcp_session(proxy_session_id).await? {
        return Ok(Some(session));
    }

    if !state.ha.dual_active_enabled() {
        return Ok(None);
    }

    let peer_nodes = state.ha.peer_nodes();
    let internal_token = match state.ha.internal_token() {
        Some(token) => token,
        None => return Ok(None),
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let local_node_id = state.ha.status().await.node_id;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    for peer in peer_nodes
        .into_iter()
        .filter(|peer| peer.node_id != local_node_id)
    {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let url = format!(
            "{}/api/internal/ha/mcp-sessions/{}",
            peer.admin_base_url.trim_end_matches('/'),
            urlencoding::encode(proxy_session_id)
        );
        let response = match client
            .get(url)
            .timeout(remaining)
            .header("x-ha-internal-token", &internal_token)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => continue,
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        if !response.status().is_success() {
            continue;
        }
        let session = match response.json::<McpSessionBinding>().await {
            Ok(session) => session,
            Err(_) => continue,
        };
        state
            .proxy
            .create_or_replace_mcp_session_binding(&session)
            .await?;
        return Ok(Some(session));
    }
    Ok(None)
}

async fn lookup_research_request_local_or_peer(
    state: &Arc<AppState>,
    request_id: &str,
) -> Result<Option<HaResearchRequestLookup>, ProxyError> {
    if let Some((key_id, token_id, expires_at)) = state
        .proxy
        .get_research_request_affinity(request_id)
        .await?
    {
        return Ok(Some(HaResearchRequestLookup {
            key_id,
            token_id,
            expires_at,
        }));
    }

    if !state.ha.dual_active_enabled() {
        return Ok(None);
    }

    let peer_nodes = state.ha.peer_nodes();
    let internal_token = match state.ha.internal_token() {
        Some(token) => token,
        None => return Ok(None),
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let local_node_id = state.ha.status().await.node_id;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut transport_error: Option<String> = None;
    for peer in peer_nodes
        .into_iter()
        .filter(|peer| peer.node_id != local_node_id)
    {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let url = format!(
            "{}/api/internal/ha/research-requests/{}",
            peer.admin_base_url.trim_end_matches('/'),
            urlencoding::encode(request_id)
        );
        let response = match client
            .get(url)
            .timeout(remaining)
            .header("x-ha-internal-token", &internal_token)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                transport_error = Some(err.to_string());
                continue;
            }
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        if !response.status().is_success() {
            transport_error = Some(format!(
                "peer {} returned {} for research request lookup",
                peer.node_id,
                response.status()
            ));
            continue;
        }
        let lookup = match response.json::<HaResearchRequestLookup>().await {
            Ok(lookup) => lookup,
            Err(err) => {
                transport_error = Some(err.to_string());
                continue;
            }
        };
        state
            .proxy
            .upsert_research_request_affinity(request_id, &lookup.key_id, &lookup.token_id, lookup.expires_at)
            .await?;
        return Ok(Some(lookup));
    }
    if let Some(err) = transport_error {
        return Err(ProxyError::Other(format!(
            "HA peer research lookup failed for {request_id}: {err}"
        )));
    }
    Ok(None)
}
