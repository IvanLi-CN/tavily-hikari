use super::*;

    #[test]
    fn extract_token_from_query_slot_strips_provider_key_slots() {
        // SerpApi facade slot: strip every `api_key` occurrence, keep the rest.
        let (q, t) = extract_token_from_query_slot(
            Some("q=hello&api_key=th-serpapi-client&engine=google"),
            "api_key",
        );
        assert_eq!(t.as_deref(), Some("th-serpapi-client"));
        assert_eq!(q.as_deref(), Some("q=hello&engine=google"));

        // Case-insensitive; first non-empty value wins (Tavily semantics).
        let (q, t) = extract_token_from_query_slot(
            Some("API_KEY=&api_key=th-1&Api_Key=th-2&foo=bar"),
            "api_key",
        );
        assert_eq!(t.as_deref(), Some("th-1"));
        assert_eq!(q.as_deref(), Some("foo=bar"));

        // Only the named slot is stripped: `tavilyApiKey` is not an
        // `api_key` match and survives a SerpApi-slot strip.
        let (q, t) = extract_token_from_query_slot(Some("tavilyApiKey=th-keep&foo=1"), "api_key");
        assert_eq!(t, None);
        assert_eq!(q.as_deref(), Some("tavilyApiKey=th-keep&foo=1"));

        // Tavily delegation is byte-identical to the generalized strip.
        let raw = Some("foo=1&tavilyApiKey=th-abc-xyz&bar=2");
        assert_eq!(
            extract_token_from_query(raw),
            extract_token_from_query_slot(raw, "tavilyApiKey")
        );
    }

    #[test]
    fn redact_sensitive_redacts_every_provider_key_slot() {
        // Tavily query slot — behavior unchanged.
        assert_eq!(
            redact_sensitive("/mcp?tavilyApiKey=th-abc-xyz&foo=1"),
            "/mcp?tavilyApiKey=<redacted>&foo=1"
        );
        // SerpApi query slot.
        assert_eq!(
            redact_sensitive("/search?api_key=th-serpapi-client&q=rust"),
            "/search?api_key=<redacted>&q=rust"
        );
        // Tavily header phrase — behavior unchanged.
        assert_eq!(
            redact_sensitive("Tavily-Api-Key: tvly-dev-secret"),
            "Tavily-Api-Key: <redacted>"
        );
        // Exa + Brave header phrases in one blob.
        let out = redact_sensitive(
            "x-api-key: th-exa-client\nX-Subscription-Token: th-brave-client\ntail ok",
        );
        assert_eq!(
            out,
            "x-api-key: <redacted>\nX-Subscription-Token: <redacted>\ntail ok"
        );
        // No provider secret survives any redacted surface.
        for surface in [
            "?api_key=mock-serpapi-key",
            "tavily-api-key: tvly-secret-1",
            "x-api-key: exa-secret-1",
            "X-Subscription-Token: brave-secret-1",
            "err: tavilyApiKey=th-secret-2 api_key=th-secret-3",
        ] {
            let out = redact_sensitive(surface);
            for secret in [
                "mock-serpapi-key",
                "tvly-secret-1",
                "exa-secret-1",
                "brave-secret-1",
                "th-secret-2",
                "th-secret-3",
            ] {
                assert!(
                    !out.contains(secret),
                    "secret {secret} leaked through redaction: {out}"
                );
            }
        }
    }
