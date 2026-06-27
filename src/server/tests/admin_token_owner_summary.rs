use super::*;
use super::core_support_and_parsing::temp_db_path;
use super::upstream_support_and_manual_jobs::spawn_admin_tokens_server;

#[tokio::test]
async fn admin_token_management_returns_owner_summary() {
    let db_path = temp_db_path("admin-token-owners");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-token-owner-alice".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alice");

    let bound = proxy
        .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
        .await
        .expect("bind alice token");
    let unbound = proxy
        .create_access_token(Some("manual-unbound"))
        .await
        .expect("create unbound token");

    let addr = spawn_admin_tokens_server(proxy, true).await;
    let client = Client::new();

    let list_resp = client
        .get(format!("http://{}/api/tokens?page=1&per_page=20", addr))
        .send()
        .await
        .expect("list tokens request");
    assert_eq!(list_resp.status(), reqwest::StatusCode::OK);
    let list_body: serde_json::Value = list_resp.json().await.expect("list tokens json");
    let items = list_body
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items is array");

    let bound_item = items
        .iter()
        .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(bound.id.as_str()))
        .expect("bound item exists");
    assert_eq!(
        bound_item
            .get("owner")
            .and_then(|value| value.get("userId"))
            .and_then(|value| value.as_str()),
        Some(alice.user_id.as_str())
    );
    assert_eq!(
        bound_item
            .get("owner")
            .and_then(|value| value.get("displayName"))
            .and_then(|value| value.as_str()),
        Some("Alice")
    );
    assert_eq!(
        bound_item
            .get("owner")
            .and_then(|value| value.get("username"))
            .and_then(|value| value.as_str()),
        Some("alice")
    );

    let unbound_item = items
        .iter()
        .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(unbound.id.as_str()))
        .expect("unbound item exists");
    assert!(
        unbound_item
            .get("owner")
            .is_some_and(|value| value.is_null()),
        "unbound token owner should be null"
    );

    let detail_resp = client
        .get(format!("http://{}/api/tokens/{}", addr, bound.id))
        .send()
        .await
        .expect("token detail request");
    assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
    let detail_body: serde_json::Value = detail_resp.json().await.expect("token detail json");
    assert_eq!(
        detail_body
            .get("owner")
            .and_then(|value| value.get("userId"))
            .and_then(|value| value.as_str()),
        Some(alice.user_id.as_str())
    );

    let unbound_detail_resp = client
        .get(format!("http://{}/api/tokens/{}", addr, unbound.id))
        .send()
        .await
        .expect("unbound token detail request");
    assert_eq!(unbound_detail_resp.status(), reqwest::StatusCode::OK);
    let unbound_detail: serde_json::Value = unbound_detail_resp
        .json()
        .await
        .expect("unbound token detail json");
    assert!(
        unbound_detail
            .get("owner")
            .is_some_and(|value| value.is_null()),
        "unbound token detail owner should be null"
    );

    let _ = std::fs::remove_file(db_path);
}
