use super::*;
use axum::http::HeaderValue;

fn settings() -> TrustedClientIpSettings {
    TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["127.0.0.0/8".to_string()],
        trusted_client_ip_headers: vec![
            "x-real-ip".to_string(),
            "x-forwarded-for".to_string(),
            "forwarded".to_string(),
        ],
    }
}

#[test]
fn trusted_proxy_uses_configured_header_order() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("203.0.113.9, 10.0.0.2"),
    );
    headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.7"));

    let info = resolve_client_ip_info(
        Some("127.0.0.1:4321".parse().unwrap()),
        &headers,
        &settings(),
    );

    assert_eq!(info.remote_addr.as_deref(), Some("127.0.0.1:4321"));
    assert_eq!(info.client_ip.as_deref(), Some("198.51.100.7"));
    assert_eq!(info.client_ip_source.as_deref(), Some("x-real-ip"));
    assert!(info.client_ip_trusted);
    assert_eq!(info.ip_headers.len(), 2);
}

#[test]
fn untrusted_remote_addr_ignores_forwarded_header() {
    let mut headers = HeaderMap::new();
    headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.7"));

    let info = resolve_client_ip_info(
        Some("203.0.113.10:4321".parse().unwrap()),
        &headers,
        &settings(),
    );

    assert_eq!(info.client_ip.as_deref(), Some("203.0.113.10"));
    assert_eq!(info.client_ip_source.as_deref(), Some("remote_addr"));
    assert!(!info.client_ip_trusted);
    assert_eq!(info.ip_headers[0].value, "198.51.100.7");
}

#[test]
fn trusted_proxy_parses_forwarded_for_parameter() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static(r#"for="[2001:db8::7]";proto=https"#),
    );

    let info = resolve_client_ip_info(
        Some("127.0.0.1:4321".parse().unwrap()),
        &headers,
        &settings(),
    );

    assert_eq!(info.client_ip.as_deref(), Some("2001:db8::7"));
    assert_eq!(info.client_ip_source.as_deref(), Some("forwarded"));
}

#[test]
fn audits_safe_preset_headers_without_trusting_them() {
    let mut headers = HeaderMap::new();
    headers.insert("eo-connecting-ip", HeaderValue::from_static("203.0.113.20"));
    headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.7"));
    let settings = TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["127.0.0.0/8".to_string()],
        trusted_client_ip_headers: vec!["x-real-ip".to_string()],
    };

    let info = resolve_client_ip_info(Some("127.0.0.1:4321".parse().unwrap()), &headers, &settings);

    assert_eq!(info.client_ip.as_deref(), Some("198.51.100.7"));
    assert_eq!(info.client_ip_source.as_deref(), Some("x-real-ip"));
    assert!(
        info.ip_headers
            .iter()
            .any(|value| value.name == "eo-connecting-ip" && value.value == "203.0.113.20")
    );
}

#[test]
fn trusted_client_ip_settings_reject_invalid_entries() {
    let invalid_cidr = TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["127.0.0.0/not-a-prefix".to_string()],
        trusted_client_ip_headers: vec!["x-real-ip".to_string()],
    };
    assert!(validate_trusted_client_ip_settings(&invalid_cidr).is_err());

    let invalid_header = TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["127.0.0.0/8".to_string()],
        trusted_client_ip_headers: vec!["bad header".to_string()],
    };
    assert!(validate_trusted_client_ip_settings(&invalid_header).is_err());

    let sensitive_header = TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["127.0.0.0/8".to_string()],
        trusted_client_ip_headers: vec!["authorization".to_string()],
    };
    assert!(validate_trusted_client_ip_settings(&sensitive_header).is_err());

    let invalid_default_count_cidrs = TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["bad-cidr-a".to_string(), "bad-cidr-b".to_string()],
        trusted_client_ip_headers: vec!["x-real-ip".to_string()],
    };
    assert!(validate_trusted_client_ip_settings(&invalid_default_count_cidrs).is_err());

    let invalid_default_count_headers = TrustedClientIpSettings {
        trusted_proxy_cidrs: vec!["127.0.0.0/8".to_string()],
        trusted_client_ip_headers: vec![
            "authorization".to_string(),
            "cookie".to_string(),
            "bad header".to_string(),
            "x-api-key".to_string(),
            "set-cookie".to_string(),
        ],
    };
    assert!(validate_trusted_client_ip_settings(&invalid_default_count_headers).is_err());
}
