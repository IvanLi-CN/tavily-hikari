use super::*;

fn bootstrap_with(mutate: impl FnOnce(&mut ProviderBootstrap)) -> ProviderBootstrap {
    let mut config = ProviderBootstrap::default();
    mutate(&mut config);
    config
}

async fn key_row(store: &KeyStore, api_key: &str) -> Option<(String, bool)> {
    sqlx::query_as::<_, (String, Option<i64>)>(
        "SELECT provider, deleted_at FROM api_keys WHERE api_key = ? LIMIT 1",
    )
    .bind(api_key)
    .fetch_optional(&store.pool)
    .await
    .expect("api key row query")
    .map(|(provider, deleted_at)| (provider, deleted_at.is_none()))
}

#[test]
fn provider_bootstrap_defaults_match_registry_and_parse() {
    let config = ProviderBootstrap::default();
    assert!(config.serpapi_keys.is_empty());
    assert!(config.exa_keys.is_empty());
    assert!(config.brave_keys.is_empty());

    let upstreams = ProviderUpstreams::parse(&config).expect("registry defaults parse");
    assert_eq!(
        upstreams.serpapi_rest,
        url::Url::parse(providers::SERPAPI.rest_base_default).expect("serpapi rest default")
    );
    assert_eq!(
        upstreams.serpapi_mcp,
        url::Url::parse(
            providers::SERPAPI
                .mcp_upstream_default
                .expect("serpapi mcp default exists")
        )
        .expect("serpapi mcp default")
    );
    assert_eq!(
        upstreams.exa_rest,
        url::Url::parse(providers::EXA.rest_base_default).expect("exa rest default")
    );
    assert_eq!(
        upstreams.brave_rest,
        url::Url::parse(providers::BRAVE.rest_base_default).expect("brave rest default")
    );
}

#[test]
fn provider_upstreams_parse_rejects_invalid_urls_with_typed_error() {
    for config in [
        bootstrap_with(|c| c.serpapi_upstream = "::bad::".to_owned()),
        bootstrap_with(|c| c.serpapi_mcp_upstream = "::bad::".to_owned()),
        bootstrap_with(|c| c.exa_upstream = "::bad::".to_owned()),
        bootstrap_with(|c| c.brave_upstream = "::bad::".to_owned()),
    ] {
        match ProviderUpstreams::parse(&config) {
            Err(ProxyError::InvalidEndpoint { endpoint, .. }) => {
                assert_eq!(endpoint, "::bad::");
            }
            other => panic!("expected InvalidEndpoint for bad provider url, got {other:?}"),
        }
    }
}

#[test]
fn provider_upstream_bases_support_path_prefixed_reverse_proxies() {
    let config = bootstrap_with(|c| {
        c.serpapi_upstream = "http://127.0.0.1:9/gw".to_owned();
        c.serpapi_mcp_upstream = "http://127.0.0.1:9/gw/mcp".to_owned();
    });
    let upstreams = ProviderUpstreams::parse(&config).expect("path-prefixed bases parse");
    assert_eq!(upstreams.serpapi_mcp.path(), "/gw/mcp");
    assert_eq!(
        crate::build_path_prefixed_url(&upstreams.serpapi_rest, "search").as_str(),
        "http://127.0.0.1:9/gw/search"
    );
    assert_eq!(
        crate::build_path_prefixed_url(&upstreams.serpapi_mcp, "search").as_str(),
        "http://127.0.0.1:9/gw/mcp/search"
    );
}

#[tokio::test]
async fn sync_keys_stamps_provider_and_scopes_soft_delete_to_same_provider() {
    let db_path = temp_db_path("provider-key-sync-scope");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    for (provider, key) in [
        (providers::ProviderId::Tavily, "tvly-1"),
        (providers::ProviderId::Serpapi, "serp-1"),
        (providers::ProviderId::Exa, "exa-1"),
        (providers::ProviderId::Brave, "brave-1"),
    ] {
        store
            .sync_keys(provider, &[key.to_owned()])
            .await
            .expect("seed sync");
    }
    // Bootstrap stamps the provider per env source.
    assert_eq!(
        key_row(&store, "tvly-1").await,
        Some(("tavily".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "serp-1").await,
        Some(("serpapi".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "exa-1").await,
        Some(("exa".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "brave-1").await,
        Some(("brave".to_owned(), true))
    );

    // Tavily re-sync soft-deletes ONLY same-provider extras (M5 direction 1).
    store
        .sync_keys(providers::ProviderId::Tavily, &["tvly-2".to_owned()])
        .await
        .expect("tavily resync");
    assert_eq!(
        key_row(&store, "tvly-1").await,
        Some(("tavily".to_owned(), false))
    );
    assert_eq!(
        key_row(&store, "tvly-2").await,
        Some(("tavily".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "serp-1").await,
        Some(("serpapi".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "exa-1").await,
        Some(("exa".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "brave-1").await,
        Some(("brave".to_owned(), true))
    );

    // Vice versa: a Brave re-sync never deletes tavily/serpapi/exa rows (M5
    // direction 2).
    store
        .sync_keys(providers::ProviderId::Brave, &["brave-2".to_owned()])
        .await
        .expect("brave resync");
    assert_eq!(
        key_row(&store, "brave-1").await,
        Some(("brave".to_owned(), false))
    );
    assert_eq!(
        key_row(&store, "brave-2").await,
        Some(("brave".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "tvly-2").await,
        Some(("tavily".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "serp-1").await,
        Some(("serpapi".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "exa-1").await,
        Some(("exa".to_owned(), true))
    );
}

#[tokio::test]
async fn sync_keys_empty_list_and_revive_stay_inside_the_provider_scope() {
    let db_path = temp_db_path("provider-key-sync-empty-revive");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    for (provider, key) in [
        (providers::ProviderId::Tavily, "tvly-keep"),
        (providers::ProviderId::Serpapi, "serp-keep"),
        (providers::ProviderId::Exa, "exa-gone"),
    ] {
        store
            .sync_keys(provider, &[key.to_owned()])
            .await
            .expect("seed sync");
    }

    // Empty list ⇒ scoped full wipe for THAT provider only; other providers
    // keep their rows.
    store
        .sync_keys(providers::ProviderId::Exa, &[])
        .await
        .expect("exa empty sync");
    assert_eq!(
        key_row(&store, "exa-gone").await,
        Some(("exa".to_owned(), false))
    );
    assert_eq!(
        key_row(&store, "tvly-keep").await,
        Some(("tavily".to_owned(), true))
    );
    assert_eq!(
        key_row(&store, "serp-keep").await,
        Some(("serpapi".to_owned(), true))
    );

    // Revive matches same-provider rows and clears deleted_at.
    store
        .sync_keys(providers::ProviderId::Exa, &["exa-gone".to_owned()])
        .await
        .expect("exa revive");
    assert_eq!(
        key_row(&store, "exa-gone").await,
        Some(("exa".to_owned(), true))
    );
}

#[tokio::test]
async fn provider_bootstrap_absent_env_leaves_provider_rows_untouched() {
    let db_path = temp_db_path("provider-bootstrap-absent");
    let db_str = db_path.to_string_lossy().to_string();
    let seed_store = KeyStore::new(&db_str).await.expect("keystore created");
    seed_store
        .sync_keys(providers::ProviderId::Brave, &["brave-keep".to_owned()])
        .await
        .expect("seed brave row");
    drop(seed_store);

    let bootstrap = bootstrap_with(|config| {
        config.serpapi_keys = vec![" serp-boot ".to_owned(), "".to_owned()];
    });
    let proxy = TavilyProxy::with_providers_in_ha_mode(
        vec!["tvly-boot".to_owned()],
        "http://127.0.0.1:9/mcp",
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        bootstrap,
        HaMode::Single,
    )
    .await
    .expect("proxy with provider bootstrap");

    // Absent flags ⇒ registry-default endpoints resolved once at construction.
    assert_eq!(
        proxy.provider_upstreams.serpapi_rest,
        url::Url::parse(providers::SERPAPI.rest_base_default).expect("serpapi rest default")
    );
    assert_eq!(
        proxy.provider_upstreams.serpapi_mcp,
        url::Url::parse(crate::SERPAPI_MCP_UPSTREAM_DEFAULT).expect("serpapi mcp default")
    );

    // Serpapi keys are trimmed, empties dropped, and stamped per env source.
    assert_eq!(
        key_row(&proxy.key_store, "serp-boot").await,
        Some(("serpapi".to_owned(), true))
    );
    // The Tavily bootstrap keeps flowing through the same provider-scoped path.
    assert_eq!(
        key_row(&proxy.key_store, "tvly-boot").await,
        Some(("tavily".to_owned(), true))
    );
    // Brave env UNSET ⇒ the brave row stays untouched (admin-API-managed only).
    assert_eq!(
        key_row(&proxy.key_store, "brave-keep").await,
        Some(("brave".to_owned(), true))
    );
}

#[tokio::test]
async fn provider_bootstrap_invalid_upstream_fails_construction_with_typed_error() {
    let db_path = temp_db_path("provider-bootstrap-bad-url");
    let db_str = db_path.to_string_lossy().to_string();
    let bootstrap = bootstrap_with(|config| {
        config.serpapi_upstream = "::bad::".to_owned();
    });
    let error = TavilyProxy::with_providers_in_ha_mode(
        Vec::<String>::new(),
        "http://127.0.0.1:9/mcp",
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        bootstrap,
        HaMode::Single,
    )
    .await
    .expect_err("invalid serpapi upstream must fail startup");
    match error {
        ProxyError::InvalidEndpoint { endpoint, .. } => {
            assert_eq!(endpoint, "::bad::");
        }
        other => panic!("expected InvalidEndpoint startup error, got {other:?}"),
    }
}
