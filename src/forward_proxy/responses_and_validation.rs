pub async fn build_forward_proxy_settings_response(
    pool: &SqlitePool,
    manager: &ForwardProxyManager,
) -> Result<ForwardProxySettingsResponse, ProxyError> {
    let settings = manager.settings.clone();
    let runtime_rows = manager.snapshot_runtime();
    let counts = load_forward_proxy_assignment_counts(pool).await?;
    let now = Utc::now().timestamp();
    let windows = [60, 15 * 60, 3600, 24 * 3600, 7 * 24 * 3600];
    let mut window_maps = Vec::new();
    for seconds in windows {
        window_maps.push(query_forward_proxy_window_stats(pool, now - seconds).await?);
    }
    let mut nodes =
        runtime_rows
            .into_iter()
            .map(|runtime| {
                let stats_for = |index: usize| {
                    window_maps[index]
                        .get(&runtime.proxy_key)
                        .cloned()
                        .map(ForwardProxyWindowStatsResponse::from)
                        .unwrap_or_default()
                };
                let assignment = counts.get(&runtime.proxy_key).cloned().unwrap_or(
                    ForwardProxyAssignmentCounts {
                        primary: 0,
                        secondary: 0,
                    },
                );
                ForwardProxyNodeResponse {
                    key: runtime.proxy_key.clone(),
                    source: runtime.source.clone(),
                    display_name: runtime.display_name.clone(),
                    endpoint_url: runtime.endpoint_url.clone(),
                    resolved_ips: runtime.resolved_ips.clone(),
                    resolved_regions: runtime.resolved_regions.clone(),
                    weight: runtime.weight,
                    available: runtime.available,
                    last_error: runtime.last_error.clone(),
                    penalized: runtime.is_penalized(),
                    primary_assignment_count: assignment.primary,
                    secondary_assignment_count: assignment.secondary,
                    stats: ForwardProxyStatsResponse {
                        one_minute: stats_for(0),
                        fifteen_minutes: stats_for(1),
                        one_hour: stats_for(2),
                        one_day: stats_for(3),
                        seven_days: stats_for(4),
                    },
                }
            })
            .collect::<Vec<_>>();
    nodes.sort_by(|lhs, rhs| lhs.display_name.cmp(&rhs.display_name));
    Ok(ForwardProxySettingsResponse {
        proxy_urls: settings.proxy_urls,
        subscription_urls: settings.subscription_urls,
        subscription_update_interval_secs: settings.subscription_update_interval_secs,
        insert_direct: settings.insert_direct,
        egress_socks5_enabled: settings.egress_socks5_enabled,
        egress_socks5_url: settings.egress_socks5_url,
        nodes,
    })
}

pub async fn build_forward_proxy_live_stats_response(
    pool: &SqlitePool,
    manager: &ForwardProxyManager,
) -> Result<ForwardProxyLiveStatsResponse, ProxyError> {
    const BUCKET_SECONDS: i64 = 3600;
    const BUCKET_COUNT: i64 = 24;
    let runtime_rows = manager.snapshot_runtime();
    let runtime_proxy_keys = runtime_rows
        .iter()
        .map(|runtime| runtime.proxy_key.clone())
        .collect::<Vec<_>>();
    let counts = load_forward_proxy_assignment_counts(pool).await?;
    let now_epoch = Utc::now().timestamp();
    let windows = [60, 15 * 60, 3600, 24 * 3600, 7 * 24 * 3600];
    let mut window_maps = Vec::new();
    for seconds in windows {
        window_maps.push(query_forward_proxy_window_stats(pool, now_epoch - seconds).await?);
    }
    let range_end_epoch = align_bucket_epoch(now_epoch, BUCKET_SECONDS, 0) + BUCKET_SECONDS;
    let range_start_epoch = range_end_epoch - BUCKET_COUNT * BUCKET_SECONDS;
    let hourly_map =
        query_forward_proxy_hourly_stats(pool, range_start_epoch, range_end_epoch).await?;
    let weight_hourly_map =
        query_forward_proxy_weight_hourly_stats(pool, range_start_epoch, range_end_epoch).await?;
    let weight_carry_map =
        query_forward_proxy_weight_last_before(pool, range_start_epoch, &runtime_proxy_keys)
            .await?;

    let mut nodes = Vec::new();
    for runtime in runtime_rows {
        let key = runtime.proxy_key.clone();
        let assignment = counts
            .get(&key)
            .cloned()
            .unwrap_or(ForwardProxyAssignmentCounts {
                primary: 0,
                secondary: 0,
            });
        let stats_key = key.clone();
        let stats_for = |index: usize| {
            window_maps[index]
                .get(&stats_key)
                .cloned()
                .map(ForwardProxyWindowStatsResponse::from)
                .unwrap_or_default()
        };
        let hourly = hourly_map.get(&key);
        let weight_hourly = weight_hourly_map.get(&key);
        let mut carry_weight = weight_carry_map
            .get(&key)
            .copied()
            .unwrap_or(runtime.weight);
        let penalized = runtime.is_penalized();
        let stats = ForwardProxyStatsResponse {
            one_minute: stats_for(0),
            fifteen_minutes: stats_for(1),
            one_hour: stats_for(2),
            one_day: stats_for(3),
            seven_days: stats_for(4),
        };
        let last24h = (0..BUCKET_COUNT)
            .map(|index| {
                let bucket_start_epoch = range_start_epoch + index * BUCKET_SECONDS;
                let bucket_end_epoch = bucket_start_epoch + BUCKET_SECONDS;
                let point = hourly
                    .and_then(|items| items.get(&bucket_start_epoch))
                    .cloned()
                    .unwrap_or_default();
                Ok(ForwardProxyHourlyBucketResponse {
                    bucket_start: format_utc_iso(bucket_start_epoch)?,
                    bucket_end: format_utc_iso(bucket_end_epoch)?,
                    success_count: point.success_count,
                    failure_count: point.failure_count,
                })
            })
            .collect::<Result<Vec<_>, ProxyError>>()?;
        let weight24h = (0..BUCKET_COUNT)
            .map(|index| {
                let bucket_start_epoch = range_start_epoch + index * BUCKET_SECONDS;
                let bucket_end_epoch = bucket_start_epoch + BUCKET_SECONDS;
                let point = weight_hourly.and_then(|items| items.get(&bucket_start_epoch));
                let (sample_count, min_weight, max_weight, avg_weight, last_weight) =
                    if let Some(point) = point {
                        carry_weight = point.last_weight;
                        (
                            point.sample_count,
                            point.min_weight,
                            point.max_weight,
                            point.avg_weight,
                            point.last_weight,
                        )
                    } else {
                        (0, carry_weight, carry_weight, carry_weight, carry_weight)
                    };
                Ok(ForwardProxyWeightHourlyBucketResponse {
                    bucket_start: format_utc_iso(bucket_start_epoch)?,
                    bucket_end: format_utc_iso(bucket_end_epoch)?,
                    sample_count,
                    min_weight,
                    max_weight,
                    avg_weight,
                    last_weight,
                })
            })
            .collect::<Result<Vec<_>, ProxyError>>()?;
        nodes.push(ForwardProxyLiveNodeResponse {
            key,
            source: runtime.source,
            display_name: runtime.display_name,
            endpoint_url: runtime.endpoint_url,
            resolved_ips: runtime.resolved_ips,
            resolved_regions: runtime.resolved_regions,
            weight: runtime.weight,
            available: runtime.available,
            last_error: runtime.last_error,
            penalized,
            primary_assignment_count: assignment.primary,
            secondary_assignment_count: assignment.secondary,
            stats,
            last24h,
            weight24h,
        });
    }
    nodes.sort_by(|lhs, rhs| lhs.display_name.cmp(&rhs.display_name));
    Ok(ForwardProxyLiveStatsResponse {
        range_start: format_utc_iso(range_start_epoch)?,
        range_end: format_utc_iso(range_end_epoch)?,
        bucket_seconds: BUCKET_SECONDS,
        nodes,
    })
}

fn default_forward_proxy_subscription_interval_secs() -> u64 {
    DEFAULT_FORWARD_PROXY_SUBSCRIPTION_INTERVAL_SECS
}

fn default_forward_proxy_insert_direct() -> bool {
    DEFAULT_FORWARD_PROXY_INSERT_DIRECT
}

fn decode_string_vec_json(raw: Option<&str>) -> Vec<String> {
    match raw {
        Some(serialized) => serde_json::from_str::<Vec<String>>(serialized).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn normalize_egress_socks5_url(raw: String) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    parse_egress_socks5_url(trimmed)
        .map(|url| url.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

fn parse_egress_socks5_url(raw: &str) -> Option<Url> {
    let parsed = parse_forward_proxy_entry(raw)?;
    if !matches!(
        parsed.protocol,
        ForwardProxyProtocol::Socks5 | ForwardProxyProtocol::Socks5h
    ) {
        return None;
    }
    parsed.endpoint_url
}

pub fn normalize_subscription_entries(raw_entries: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for entry in raw_entries {
        for token in split_proxy_entry_tokens(&entry) {
            let Ok(url) = Url::parse(token) else {
                continue;
            };
            if !matches!(url.scheme(), "http" | "https") {
                continue;
            }
            let canonical = url.to_string();
            if seen.insert(canonical.clone()) {
                normalized.push(canonical);
            }
        }
    }
    normalized
}

pub fn normalize_proxy_url_entries(raw_entries: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for entry in raw_entries {
        for token in split_proxy_entry_tokens(&entry) {
            if let Some(parsed) = parse_forward_proxy_entry(token)
                && seen.insert(parsed.normalized.clone())
            {
                normalized.push(parsed.normalized);
            }
        }
    }
    normalized
}

fn split_proxy_entry_tokens(raw: &str) -> Vec<&str> {
    raw.split(['\n', ',', ';'])
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.starts_with('#'))
        .collect()
}

pub fn normalize_proxy_endpoints_from_urls(urls: &[String]) -> Vec<ForwardProxyEndpoint> {
    let mut seen = HashSet::new();
    let mut endpoints = Vec::new();
    for raw in urls {
        if let Some(parsed) = parse_forward_proxy_entry(raw) {
            let key = parsed.normalized.clone();
            if !seen.insert(key.clone()) {
                continue;
            }
            endpoints.push(ForwardProxyEndpoint::new_manual(
                key,
                parsed.display_name,
                parsed.protocol,
                parsed.endpoint_url,
                Some(parsed.normalized),
            ));
        }
    }
    endpoints
}

pub fn normalize_subscription_endpoints_from_urls(
    urls: &[String],
    subscription_source: &str,
) -> Vec<ForwardProxyEndpoint> {
    let mut seen = HashSet::new();
    let mut endpoints = Vec::new();
    for raw in urls {
        if let Some(parsed) = parse_forward_proxy_entry(raw) {
            let key = parsed.normalized.clone();
            if !seen.insert(key.clone()) {
                continue;
            }
            endpoints.push(ForwardProxyEndpoint::new_subscription(
                key,
                parsed.display_name,
                parsed.protocol,
                parsed.endpoint_url,
                Some(parsed.normalized),
                subscription_source.to_string(),
            ));
        }
    }
    endpoints
}

#[derive(Debug, Clone)]
pub struct ParsedForwardProxyEntry {
    pub normalized: String,
    pub display_name: String,
    pub protocol: ForwardProxyProtocol,
    pub endpoint_url: Option<Url>,
}

pub fn parse_forward_proxy_entry(raw: &str) -> Option<ParsedForwardProxyEntry> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        return None;
    }
    if !candidate.contains("://") {
        return parse_native_forward_proxy(&format!("http://{candidate}"));
    }
    let (scheme_raw, _) = candidate.split_once("://")?;
    let scheme = scheme_raw.to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" | "socks5" | "socks5h" | "socks" => parse_native_forward_proxy(candidate),
        "vmess" => parse_vmess_forward_proxy(candidate),
        "vless" => parse_vless_forward_proxy(candidate),
        "trojan" => parse_trojan_forward_proxy(candidate),
        "ss" => parse_shadowsocks_forward_proxy(candidate),
        _ => None,
    }
}

fn parse_native_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let parsed = Url::parse(candidate).ok()?;
    let raw_scheme = parsed.scheme();
    let (protocol, normalized_scheme) = match raw_scheme {
        "http" => (ForwardProxyProtocol::Http, "http"),
        "https" => (ForwardProxyProtocol::Https, "https"),
        "socks5" | "socks" => (ForwardProxyProtocol::Socks5, "socks5"),
        "socks5h" => (ForwardProxyProtocol::Socks5h, "socks5h"),
        _ => return None,
    };
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    let mut normalized = format!("{normalized_scheme}://");
    if !parsed.username().is_empty() {
        normalized.push_str(parsed.username());
        if let Some(password) = parsed.password() {
            normalized.push(':');
            normalized.push_str(password);
        }
        normalized.push('@');
    }
    if host.contains(':') {
        normalized.push('[');
        normalized.push_str(host);
        normalized.push(']');
    } else {
        normalized.push_str(&host.to_ascii_lowercase());
    }
    normalized.push(':');
    normalized.push_str(&port.to_string());
    let endpoint_url = Url::parse(&normalized).ok()?;
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name: format!("{host}:{port}"),
        protocol,
        endpoint_url: Some(endpoint_url),
    })
}

fn parse_vmess_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "vmess")?;
    let parsed = parse_vmess_share_link(&normalized).ok()?;
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name: parsed.display_name,
        protocol: ForwardProxyProtocol::Vmess,
        endpoint_url: None,
    })
}

fn parse_vless_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "vless")?;
    let parsed = Url::parse(&normalized).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    let display_name =
        proxy_display_name_from_url(&parsed).unwrap_or_else(|| format!("{host}:{port}"));
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name,
        protocol: ForwardProxyProtocol::Vless,
        endpoint_url: None,
    })
}

fn parse_trojan_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "trojan")?;
    let parsed = Url::parse(&normalized).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    let display_name =
        proxy_display_name_from_url(&parsed).unwrap_or_else(|| format!("{host}:{port}"));
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name,
        protocol: ForwardProxyProtocol::Trojan,
        endpoint_url: None,
    })
}

fn parse_shadowsocks_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "ss")?;
    let parsed = parse_shadowsocks_share_link(&normalized).ok()?;
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name: parsed.display_name,
        protocol: ForwardProxyProtocol::Shadowsocks,
        endpoint_url: None,
    })
}

fn proxy_display_name_from_url(url: &Url) -> Option<String> {
    if let Some(fragment) = url.fragment() {
        let decoded = percent_decode_once_lossy(fragment);
        if !decoded.trim().is_empty() {
            return Some(decoded);
        }
    }
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    Some(format!("{host}:{port}"))
}

fn normalize_share_link_scheme(candidate: &str, scheme: &str) -> Option<String> {
    let (_, remainder) = candidate.split_once("://")?;
    let normalized = format!("{scheme}://{}", remainder.trim());
    if normalized.len() <= scheme.len() + 3 {
        return None;
    }
    Some(normalized)
}

fn decode_base64_any(raw: &str) -> Option<Vec<u8>> {
    let compact = raw
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    if compact.is_empty() {
        return None;
    }
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(compact.as_bytes()) {
            return Some(decoded);
        }
    }
    None
}

fn decode_base64_string(raw: &str) -> Option<String> {
    decode_base64_any(raw).and_then(|bytes| String::from_utf8(bytes).ok())
}

#[derive(Debug, Clone)]
struct VmessShareLink {
    address: String,
    port: u16,
    id: String,
    alter_id: u32,
    security: String,
    network: String,
    host: Option<String>,
    path: Option<String>,
    tls_mode: Option<String>,
    sni: Option<String>,
    alpn: Option<Vec<String>>,
    fingerprint: Option<String>,
    display_name: String,
}

fn parse_vmess_share_link(raw: &str) -> Result<VmessShareLink, ProxyError> {
    let payload = raw
        .strip_prefix("vmess://")
        .ok_or_else(|| ProxyError::Other("invalid vmess share link".to_string()))?;
    let decoded = decode_base64_string(payload)
        .ok_or_else(|| ProxyError::Other("failed to decode vmess payload".to_string()))?;
    let value: Value = serde_json::from_str(&decoded)
        .map_err(|err| ProxyError::Other(format!("invalid vmess json payload: {err}")))?;

    let address = value
        .get("add")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::Other("vmess payload missing add".to_string()))?
        .to_string();
    let port = parse_port_value(value.get("port"))
        .ok_or_else(|| ProxyError::Other("vmess payload missing port".to_string()))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::Other("vmess payload missing id".to_string()))?
        .to_string();
    let alter_id = parse_u32_value(value.get("aid")).unwrap_or(0);
    let security = value
        .get("scy")
        .and_then(Value::as_str)
        .or_else(|| value.get("security").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto")
        .to_string();
    let network = value
        .get("net")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let host = value
        .get("host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let tls_mode = value
        .get("tls")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let sni = value
        .get("sni")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let alpn = value
        .get("alpn")
        .and_then(Value::as_str)
        .map(parse_alpn_csv)
        .filter(|items| !items.is_empty());
    let fingerprint = value
        .get("fp")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let display_name = value
        .get("ps")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{address}:{port}"));
    Ok(VmessShareLink {
        address,
        port,
        id,
        alter_id,
        security,
        network,
        host,
        path,
        tls_mode,
        sni,
        alpn,
        fingerprint,
        display_name,
    })
}

fn parse_u32_value(value: Option<&Value>) -> Option<u32> {
    match value {
        Some(Value::Number(num)) => num.as_u64().and_then(|value| u32::try_from(value).ok()),
        Some(Value::String(raw)) => raw.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn parse_port_value(value: Option<&Value>) -> Option<u16> {
    match value {
        Some(Value::Number(num)) => num.as_u64().and_then(|value| u16::try_from(value).ok()),
        Some(Value::String(raw)) => raw.trim().parse::<u16>().ok(),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct ShadowsocksShareLink {
    method: String,
    password: String,
    host: String,
    port: u16,
    display_name: String,
}

fn parse_shadowsocks_share_link(raw: &str) -> Result<ShadowsocksShareLink, ProxyError> {
    let normalized = raw
        .strip_prefix("ss://")
        .ok_or_else(|| ProxyError::Other("invalid shadowsocks share link".to_string()))?;
    let (main, fragment) = split_once_first(normalized, '#');
    let (main, _) = split_once_first(main, '?');
    let display_name = fragment
        .map(percent_decode_once_lossy)
        .filter(|value| !value.trim().is_empty());

    if let Ok(url) = Url::parse(raw)
        && let Some(host) = url.host_str()
        && let Some(port) = url.port_or_known_default()
    {
        let credentials = if !url.username().is_empty() && url.password().is_some() {
            Some((
                percent_decode_once_lossy(url.username()),
                percent_decode_once_lossy(url.password().unwrap_or_default()),
            ))
        } else if !url.username().is_empty() {
            let username = percent_decode_once_lossy(url.username());
            decode_base64_string(&username).and_then(|decoded| {
                let (method, password) = decoded.split_once(':')?;
                Some((method.to_string(), password.to_string()))
            })
        } else {
            None
        };
        if let Some((method, password)) = credentials {
            return Ok(ShadowsocksShareLink {
                method,
                password,
                host: host.to_string(),
                port,
                display_name: display_name
                    .clone()
                    .unwrap_or_else(|| format!("{host}:{port}")),
            });
        }
    }

    let decoded_main = if main.contains('@') {
        main.to_string()
    } else {
        let main_for_decode = percent_decode_once_lossy(main);
        decode_base64_string(&main_for_decode)
            .ok_or_else(|| ProxyError::Other("failed to decode shadowsocks payload".to_string()))?
    };

    let (credential, host_port) = decoded_main
        .rsplit_once('@')
        .ok_or_else(|| ProxyError::Other("invalid shadowsocks payload".to_string()))?;
    let (method, password) = if let Some((method, password)) = credential.split_once(':') {
        (
            percent_decode_once_lossy(method),
            percent_decode_once_lossy(password),
        )
    } else {
        let decoded_credential = decode_base64_string(credential).ok_or_else(|| {
            ProxyError::Other("failed to decode shadowsocks credentials".to_string())
        })?;
        let (method, password) = decoded_credential
            .split_once(':')
            .ok_or_else(|| ProxyError::Other("invalid shadowsocks credentials".to_string()))?;
        (
            percent_decode_once_lossy(method),
            percent_decode_once_lossy(password),
        )
    };
    let parsed_host = Url::parse(&format!("http://{host_port}"))
        .map_err(|err| ProxyError::Other(format!("invalid shadowsocks server endpoint: {err}")))?;
    let host = parsed_host
        .host_str()
        .ok_or_else(|| ProxyError::Other("shadowsocks host missing".to_string()))?
        .to_string();
    let port = parsed_host
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("shadowsocks port missing".to_string()))?;
    Ok(ShadowsocksShareLink {
        method,
        password,
        host: host.clone(),
        port,
        display_name: display_name.unwrap_or_else(|| format!("{host}:{port}")),
    })
}

fn split_once_first(raw: &str, delimiter: char) -> (&str, Option<&str>) {
    if let Some((lhs, rhs)) = raw.split_once(delimiter) {
        (lhs, Some(rhs))
    } else {
        (raw, None)
    }
}

fn parse_alpn_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn percent_decode_once_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'%'
            && idx + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                decode_hex_nibble(bytes[idx + 1]),
                decode_hex_nibble(bytes[idx + 2]),
            )
        {
            decoded.push((hi << 4) | lo);
            idx += 3;
            continue;
        }
        decoded.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn deterministic_unit_f64(seed: u64) -> f64 {
    let mut value = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
    value ^= value >> 33;
    (value as f64) / (u64::MAX as f64)
}

fn align_bucket_epoch(epoch: i64, bucket_seconds: i64, offset_seconds: i64) -> i64 {
    if bucket_seconds <= 0 {
        return epoch;
    }
    (epoch - offset_seconds).div_euclid(bucket_seconds) * bucket_seconds + offset_seconds
}

fn format_utc_iso(epoch: i64) -> Result<String, ProxyError> {
    let dt = Utc
        .timestamp_opt(epoch, 0)
        .single()
        .ok_or_else(|| ProxyError::Other(format!("invalid epoch {epoch}")))?;
    Ok(dt.to_rfc3339())
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

pub async fn fetch_subscription_proxy_urls(
    client: &Client,
    subscription_url: &str,
    request_timeout: Duration,
) -> Result<Vec<String>, ProxyError> {
    let response = timeout(request_timeout, client.get(subscription_url).send())
        .await
        .map_err(|_| ProxyError::Other("subscription request timed out".to_string()))?
        .map_err(ProxyError::Http)?;
    if !response.status().is_success() {
        return Err(ProxyError::Other(format!(
            "subscription url returned status {}: {}",
            response.status(),
            subscription_url
        )));
    }
    let body = timeout(request_timeout, response.text())
        .await
        .map_err(|_| ProxyError::Other("subscription body read timed out".to_string()))?
        .map_err(ProxyError::Http)?;
    let urls = parse_proxy_urls_from_subscription_body(&body);
    if urls.is_empty() && subscription_body_uses_unsupported_structure(&body) {
        return Err(ProxyError::Other(
            "subscription contains no supported proxy entries".to_string(),
        ));
    }
    Ok(urls)
}

pub(crate) async fn fetch_subscription_proxy_urls_with_validation_budget(
    client: &Client,
    subscription_url: &str,
    total_timeout: Duration,
    started: Instant,
) -> Result<Vec<String>, ProxyError> {
    let request_timeout = remaining_timeout_budget(total_timeout, started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?;
    let response = timeout(request_timeout, client.get(subscription_url).send())
        .await
        .map_err(|_| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?
        .map_err(ProxyError::Http)?;
    if !response.status().is_success() {
        return Err(ProxyError::Other(format!(
            "subscription url returned status {}: {}",
            response.status(),
            subscription_url
        )));
    }
    let read_timeout = remaining_timeout_budget(total_timeout, started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?;
    let body = timeout(read_timeout, response.text())
        .await
        .map_err(|_| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?
        .map_err(ProxyError::Http)?;
    let urls = parse_proxy_urls_from_subscription_body(&body);
    if urls.is_empty() && subscription_body_uses_unsupported_structure(&body) {
        return Err(ProxyError::Other(
            "subscription contains no supported proxy entries".to_string(),
        ));
    }
    Ok(urls)
}

fn parse_proxy_urls_from_subscription_body(raw: &str) -> Vec<String> {
    let decoded = decode_subscription_payload(raw);
    if subscription_body_uses_unsupported_structure(&decoded) {
        return Vec::new();
    }
    normalize_proxy_url_entries(vec![decoded])
}

fn subscription_body_uses_unsupported_structure(raw: &str) -> bool {
    raw.lines().map(str::trim).any(|line| {
        line == "proxies:"
            || line == "proxy-providers:"
            || line == "proxy-groups:"
            || line == "rule-providers:"
    })
}

pub fn decode_subscription_payload(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("://")
        || trimmed
            .lines()
            .filter(|line| !line.trim().is_empty())
            .any(|line| line.contains("://"))
    {
        return trimmed.to_string();
    }
    let compact = trimmed
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(compact.as_bytes())
            && let Ok(text) = String::from_utf8(decoded)
            && text.contains("://")
        {
            return text;
        }
    }
    trimmed.to_string()
}

fn is_validation_probe_reachable_status(status: StatusCode) -> bool {
    status.is_success()
        || status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status == StatusCode::NOT_FOUND
}

fn forward_proxy_validation_timeout(kind: ForwardProxyValidationKind) -> Duration {
    match kind {
        ForwardProxyValidationKind::ProxyUrl => {
            Duration::from_secs(FORWARD_PROXY_VALIDATION_TIMEOUT_SECS)
        }
        ForwardProxyValidationKind::SubscriptionUrl => {
            Duration::from_secs(FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS)
        }
    }
}

fn remaining_timeout_budget(total_timeout: Duration, elapsed: Duration) -> Option<Duration> {
    total_timeout.checked_sub(elapsed)
}

fn classify_forward_proxy_error(err: &ProxyError) -> &'static str {
    match err {
        ProxyError::Http(source) if source.is_timeout() => FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT,
        ProxyError::Http(_) => FORWARD_PROXY_FAILURE_SEND_ERROR,
        ProxyError::Other(message) if message.contains("timed out") => {
            FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT
        }
        _ => FORWARD_PROXY_FAILURE_SEND_ERROR,
    }
}

fn build_forward_proxy_probe_target(usage_base: &str) -> Result<Url, ProxyError> {
    let url = Url::parse(usage_base).map_err(|err| ProxyError::InvalidEndpoint {
        endpoint: usage_base.to_string(),
        source: err,
    })?;
    Ok(build_path_prefixed_url(&url, "/usage"))
}

pub async fn probe_forward_proxy_endpoint(
    client_pool: &ForwardProxyClientPool,
    endpoint: &ForwardProxyEndpoint,
    probe_url: &Url,
    timeout_budget: Duration,
) -> Result<f64, ProxyError> {
    if (endpoint.requires_xray() || endpoint.uses_local_relay) && endpoint.endpoint_url.is_none() {
        return Err(ProxyError::Other("xray_missing".to_string()));
    }
    let client = client_pool
        .client_for(endpoint.endpoint_url.as_ref())
        .await?;
    let started = Instant::now();
    let response = timeout(timeout_budget, client.get(probe_url.clone()).send())
        .await
        .map_err(|_| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                timeout_budget.as_millis()
            ))
        })?
        .map_err(ProxyError::Http)?;
    if !is_validation_probe_reachable_status(response.status()) {
        return Err(ProxyError::Other(format!(
            "validation probe returned status {}",
            response.status()
        )));
    }
    Ok(elapsed_ms(started))
}

pub fn failure_kind_from_http_error(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT
    } else if err.is_status() {
        FORWARD_PROXY_FAILURE_UPSTREAM_HTTP_5XX
    } else {
        FORWARD_PROXY_FAILURE_SEND_ERROR
    }
}

async fn wait_for_xray_api_ready(
    child: &mut Child,
    api_port: u16,
    ready_timeout: Duration,
) -> Result<(), ProxyError> {
    let deadline = Instant::now() + ready_timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|err| {
            ProxyError::Other(format!("failed to poll xray proxy process status: {err}"))
        })? {
            return Err(ProxyError::Other(format!(
                "xray process exited before ready: {status}"
            )));
        }
        if timeout(
            Duration::from_millis(250),
            TcpStream::connect(("127.0.0.1", api_port)),
        )
        .await
        .is_ok_and(|connection| connection.is_ok())
        {
            sleep(Duration::from_millis(50)).await;
            if let Some(status) = child.try_wait().map_err(|err| {
                ProxyError::Other(format!("failed to poll xray proxy process status: {err}"))
            })? {
                return Err(ProxyError::Other(format!(
                    "xray process exited before ready: {status}"
                )));
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ProxyError::Other(
                "xray api endpoint was not ready in time".to_string(),
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_local_socks_ready(
    local_port: u16,
    ready_timeout: Duration,
) -> Result<(), ProxyError> {
    let deadline = Instant::now() + ready_timeout;
    loop {
        if let Ok(Ok(mut stream)) = timeout(
            Duration::from_millis(250),
            TcpStream::connect(("127.0.0.1", local_port)),
        )
        .await
        {
            let mut response = [0_u8; 2];
            let handshake = timeout(Duration::from_millis(250), async {
                stream.write_all(&[0x05, 0x01, 0x00]).await?;
                stream.read_exact(&mut response).await?;
                Ok::<_, io::Error>(response)
            })
            .await;
            if handshake.is_ok_and(|result| result.is_ok_and(|reply| reply == [0x05, 0x00])) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(ProxyError::Other(
                "xray local socks endpoint was not ready in time".to_string(),
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn terminate_child_process(
    child: &mut Child,
    grace_period: Duration,
) -> Result<(), io::Error> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if result == 0
                && !grace_period.is_zero()
                && timeout(grace_period, child.wait()).await.is_ok()
            {
                return Ok(());
            }
        }
    }
    child.kill().await?;
    let _ = timeout(grace_period, child.wait()).await;
    Ok(())
}

fn reserve_unused_local_port() -> Result<ReservedLocalPort, ProxyError> {
    ReservedLocalPort::bind()
}

pub fn stable_hash_u64(raw: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    hasher.finish()
}

fn build_xray_route_key(
    endpoint: &ForwardProxyEndpoint,
    egress_socks5_url: Option<&Url>,
) -> String {
    let mut key = String::with_capacity(endpoint.key.len() + 64);
    key.push_str(&endpoint.key);
    key.push('|');
    if let Some(raw_url) = endpoint.raw_url.as_deref() {
        key.push_str(raw_url);
    } else if let Some(endpoint_url) = endpoint.endpoint_url.as_ref() {
        key.push_str(endpoint_url.as_str());
    }
    key.push('|');
    if let Some(egress_socks5_url) = egress_socks5_url {
        key.push_str(egress_socks5_url.as_str());
    }
    if endpoint.key.starts_with("__validate_xray__") {
        endpoint.key.clone()
    } else {
        format!("relay::{:016x}", stable_hash_u64(&key))
    }
}

fn write_xray_runtime_json(path: &PathBuf, value: &Value) -> Result<(), ProxyError> {
    let serialized = serde_json::to_vec_pretty(value)
        .map_err(|err| ProxyError::Other(format!("failed to serialize xray config: {err}")))?;
    fs::write(path, serialized).map_err(|err| {
        ProxyError::Other(format!(
            "failed to write xray config {}: {err}",
            path.display()
        ))
    })
}

fn cleanup_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn join_cleanup_errors<const N: usize>(
    results: [Result<(), ProxyError>; N],
) -> Result<(), ProxyError> {
    let errors = results
        .into_iter()
        .filter_map(Result::err)
        .map(|err| err.to_string())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ProxyError::Other(errors.join("; ")))
    }
}

fn build_xray_outbound_for_endpoint(
    endpoint: &ForwardProxyEndpoint,
    egress_tag: Option<&str>,
) -> Result<Value, ProxyError> {
    let raw = endpoint.raw_url.as_deref();
    let native_url = endpoint_transport_url(endpoint);
    let mut outbound = match endpoint.protocol {
        ForwardProxyProtocol::Http => {
            build_http_xray_outbound(native_url.as_ref().ok_or_else(|| {
                ProxyError::Other("xray endpoint missing native proxy url".to_string())
            })?)?
        }
        ForwardProxyProtocol::Https => {
            build_http_xray_outbound(native_url.as_ref().ok_or_else(|| {
                ProxyError::Other("xray endpoint missing native proxy url".to_string())
            })?)?
        }
        ForwardProxyProtocol::Socks5 | ForwardProxyProtocol::Socks5h => {
            build_socks_xray_outbound(native_url.as_ref().ok_or_else(|| {
                ProxyError::Other("xray endpoint missing native proxy url".to_string())
            })?)?
        }
        ForwardProxyProtocol::Vmess => build_vmess_xray_outbound(raw.ok_or_else(|| {
            ProxyError::Other("xray endpoint missing share link url".to_string())
        })?)?,
        ForwardProxyProtocol::Vless => build_vless_xray_outbound(raw.ok_or_else(|| {
            ProxyError::Other("xray endpoint missing share link url".to_string())
        })?)?,
        ForwardProxyProtocol::Trojan => build_trojan_xray_outbound(raw.ok_or_else(|| {
            ProxyError::Other("xray endpoint missing share link url".to_string())
        })?)?,
        ForwardProxyProtocol::Shadowsocks => {
            build_shadowsocks_xray_outbound(raw.ok_or_else(|| {
                ProxyError::Other("xray endpoint missing share link url".to_string())
            })?)?
        }
        _ => {
            return Err(ProxyError::Other(
                "unsupported xray protocol for endpoint".to_string(),
            ));
        }
    };
    if let Some(egress_tag) = egress_tag {
        attach_xray_proxy_settings(&mut outbound, egress_tag);
    }
    Ok(outbound)
}

fn build_http_xray_outbound(proxy_url: &Url) -> Result<Value, ProxyError> {
    let host = proxy_url
        .host_str()
        .ok_or_else(|| ProxyError::Other("http proxy host missing".to_string()))?;
    let port = proxy_url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("http proxy port missing".to_string()))?;
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "http",
        "settings": {
            "servers": [{
                "address": host,
                "port": port,
            }]
        }
    });
    if !proxy_url.username().is_empty() || proxy_url.password().is_some() {
        outbound["settings"]["servers"][0]["users"] = json!([{
            "user": percent_decode_once_lossy(proxy_url.username()),
            "pass": proxy_url.password().map(percent_decode_once_lossy).unwrap_or_default(),
        }]);
    }
    if proxy_url.scheme() == "https" {
        outbound["streamSettings"] = json!({
            "security": "tls",
            "tlsSettings": {
                "serverName": host,
            }
        });
    }
    Ok(outbound)
}

fn build_socks_xray_outbound(proxy_url: &Url) -> Result<Value, ProxyError> {
    let host = proxy_url
        .host_str()
        .ok_or_else(|| ProxyError::Other("socks proxy host missing".to_string()))?;
    let port = proxy_url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("socks proxy port missing".to_string()))?;
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "socks",
        "settings": {
            "servers": [{
                "address": host,
                "port": port,
            }]
        }
    });
    if !proxy_url.username().is_empty() || proxy_url.password().is_some() {
        outbound["settings"]["servers"][0]["users"] = json!([{
            "user": percent_decode_once_lossy(proxy_url.username()),
            "pass": proxy_url.password().map(percent_decode_once_lossy).unwrap_or_default(),
        }]);
    }
    if proxy_url.scheme() == "socks5" {
        outbound["targetStrategy"] = Value::String("UseIP".to_string());
    }
    Ok(outbound)
}

fn build_xray_egress_outbound(egress_socks5_url: Option<&Url>, tag: Option<&str>) -> Option<Value> {
    let egress_socks5_url = egress_socks5_url?;
    let mut outbound = build_socks_xray_outbound(egress_socks5_url).ok()?;
    set_xray_outbound_tag(&mut outbound, tag.unwrap_or("egress-socks"));
    Some(outbound)
}

fn set_xray_outbound_tag(outbound: &mut Value, tag: &str) {
    if let Some(object) = outbound.as_object_mut() {
        object.insert("tag".to_string(), Value::String(tag.to_string()));
    }
}

fn attach_xray_proxy_settings(outbound: &mut Value, tag: &str) {
    if let Some(object) = outbound.as_object_mut() {
        object.insert("proxySettings".to_string(), json!({ "tag": tag }));
    }
}

fn build_vmess_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let link = parse_vmess_share_link(raw)?;
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "vmess",
        "settings": {
            "vnext": [{
                "address": link.address,
                "port": link.port,
                "users": [{ "id": link.id, "alterId": link.alter_id, "security": link.security }]
            }]
        }
    });
    if let Some(stream_settings) = build_vmess_stream_settings(&link)
        && let Some(object) = outbound.as_object_mut()
    {
        object.insert("streamSettings".to_string(), stream_settings);
    }
    Ok(outbound)
}

fn build_vmess_stream_settings(link: &VmessShareLink) -> Option<Value> {
    let mut stream = serde_json::Map::new();
    stream.insert("network".to_string(), Value::String(link.network.clone()));
    let mut has_non_default_options = link.network != "tcp";
    let security = link
        .tls_mode
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "none")
        .map(|value| value.to_ascii_lowercase());
    if let Some(security) = security.as_ref() {
        stream.insert("security".to_string(), Value::String(security.clone()));
        has_non_default_options = true;
    }
    match link.network.as_str() {
        "ws" => {
            let mut ws = serde_json::Map::new();
            if let Some(path) = link.path.as_ref().filter(|value| !value.trim().is_empty()) {
                ws.insert("path".to_string(), Value::String(path.clone()));
            }
            if let Some(host) = link.host.as_ref().filter(|value| !value.trim().is_empty()) {
                ws.insert("headers".to_string(), json!({ "Host": host }));
            }
            if !ws.is_empty() {
                stream.insert("wsSettings".to_string(), Value::Object(ws));
                has_non_default_options = true;
            }
        }
        "grpc" => {
            let service_name = link
                .path
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_default();
            stream.insert(
                "grpcSettings".to_string(),
                json!({ "serviceName": service_name }),
            );
            has_non_default_options = true;
        }
        _ => {}
    }
    if let Some(security) = security
        && security == "tls"
    {
        let mut tls_settings = serde_json::Map::new();
        if let Some(server_name) = link
            .sni
            .as_ref()
            .or(link.host.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            tls_settings.insert("serverName".to_string(), Value::String(server_name.clone()));
        }
        if let Some(alpn) = link.alpn.as_ref().filter(|items| !items.is_empty()) {
            tls_settings.insert("alpn".to_string(), json!(alpn));
        }
        if let Some(fingerprint) = link
            .fingerprint
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            tls_settings.insert(
                "fingerprint".to_string(),
                Value::String(fingerprint.clone()),
            );
        }
        if !tls_settings.is_empty() {
            stream.insert("tlsSettings".to_string(), Value::Object(tls_settings));
            has_non_default_options = true;
        }
    }
    if has_non_default_options {
        Some(Value::Object(stream))
    } else {
        None
    }
}

fn build_vless_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let url = Url::parse(raw)
        .map_err(|err| ProxyError::Other(format!("invalid vless share link: {err}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProxyError::Other("vless host missing".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("vless port missing".to_string()))?;
    let user_id = url.username();
    if user_id.trim().is_empty() {
        return Err(ProxyError::Other("vless id missing".to_string()));
    }
    let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    let encryption = query
        .get("encryption")
        .cloned()
        .unwrap_or_else(|| "none".to_string());
    let mut user = serde_json::Map::new();
    user.insert("id".to_string(), Value::String(user_id.to_string()));
    user.insert("encryption".to_string(), Value::String(encryption));
    if let Some(flow) = query.get("flow").filter(|value| !value.trim().is_empty()) {
        user.insert("flow".to_string(), Value::String(flow.clone()));
    }
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "vless",
        "settings": { "vnext": [{ "address": host, "port": port, "users": [Value::Object(user)] }] }
    });
    if let Some(stream_settings) = build_stream_settings_from_url(&url, None)
        && let Some(object) = outbound.as_object_mut()
    {
        object.insert("streamSettings".to_string(), stream_settings);
    }
    Ok(outbound)
}

fn build_trojan_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let url = Url::parse(raw)
        .map_err(|err| ProxyError::Other(format!("invalid trojan share link: {err}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProxyError::Other("trojan host missing".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("trojan port missing".to_string()))?;
    let password = url.username();
    if password.trim().is_empty() {
        return Err(ProxyError::Other("trojan password missing".to_string()));
    }
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "trojan",
        "settings": { "servers": [{ "address": host, "port": port, "password": password }] }
    });
    if let Some(stream_settings) = build_stream_settings_from_url(&url, Some("tls"))
        && let Some(object) = outbound.as_object_mut()
    {
        object.insert("streamSettings".to_string(), stream_settings);
    }
    Ok(outbound)
}

fn build_shadowsocks_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let parsed = parse_shadowsocks_share_link(raw)?;
    Ok(json!({
        "tag": "proxy",
        "protocol": "shadowsocks",
        "settings": { "servers": [{ "address": parsed.host, "port": parsed.port, "method": parsed.method, "password": parsed.password }] }
    }))
}

fn build_stream_settings_from_url(url: &Url, default_security: Option<&str>) -> Option<Value> {
    let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    let network = query
        .get("type")
        .or_else(|| query.get("net"))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "tcp".to_string());
    let security = query
        .get("security")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .or_else(|| default_security.map(str::to_string))
        .unwrap_or_else(|| "none".to_string());
    let host = query
        .get("host")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let path = query
        .get("path")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let service_name = query
        .get("serviceName")
        .or_else(|| query.get("service_name"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| path.clone());
    let mut stream = serde_json::Map::new();
    stream.insert("network".to_string(), Value::String(network.clone()));
    let mut has_non_default_options = network != "tcp";
    if security != "none" {
        stream.insert("security".to_string(), Value::String(security.clone()));
        has_non_default_options = true;
    }
    match network.as_str() {
        "ws" => {
            let mut ws = serde_json::Map::new();
            if let Some(path) = path.as_ref() {
                ws.insert("path".to_string(), Value::String(path.clone()));
            }
            if let Some(host) = host.as_ref() {
                ws.insert("headers".to_string(), json!({ "Host": host }));
            }
            if !ws.is_empty() {
                stream.insert("wsSettings".to_string(), Value::Object(ws));
                has_non_default_options = true;
            }
        }
        "grpc" => {
            stream.insert(
                "grpcSettings".to_string(),
                json!({ "serviceName": service_name.unwrap_or_default() }),
            );
            has_non_default_options = true;
        }
        _ => {}
    }
    if security == "tls" {
        let mut tls_settings = serde_json::Map::new();
        if let Some(server_name) = query
            .get("sni")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| host.clone())
            .or_else(|| url.host_str().map(str::to_string))
        {
            tls_settings.insert("serverName".to_string(), Value::String(server_name));
        }
        if query_flag_true(&query, "allowInsecure") || query_flag_true(&query, "insecure") {
            tls_settings.insert("allowInsecure".to_string(), Value::Bool(true));
        }
        if let Some(fingerprint) = query
            .get("fp")
            .or_else(|| query.get("fingerprint"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            tls_settings.insert("fingerprint".to_string(), Value::String(fingerprint));
        }
        if let Some(alpn) = query
            .get("alpn")
            .map(|value| parse_alpn_csv(value))
            .filter(|items| !items.is_empty())
        {
            tls_settings.insert("alpn".to_string(), json!(alpn));
        }
        if !tls_settings.is_empty() {
            stream.insert("tlsSettings".to_string(), Value::Object(tls_settings));
            has_non_default_options = true;
        }
    } else if security == "reality" {
        let mut reality_settings = serde_json::Map::new();
        if let Some(server_name) = query
            .get("sni")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| host.clone())
            .or_else(|| url.host_str().map(str::to_string))
        {
            reality_settings.insert("serverName".to_string(), Value::String(server_name));
        }
        if let Some(fingerprint) = query
            .get("fp")
            .or_else(|| query.get("fingerprint"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("fingerprint".to_string(), Value::String(fingerprint));
        }
        if let Some(public_key) = query
            .get("pbk")
            .or_else(|| query.get("publicKey"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("publicKey".to_string(), Value::String(public_key));
        }
        if let Some(short_id) = query
            .get("sid")
            .or_else(|| query.get("shortId"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("shortId".to_string(), Value::String(short_id));
        }
        if let Some(spider_x) = query
            .get("spx")
            .or_else(|| query.get("spiderX"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("spiderX".to_string(), Value::String(spider_x));
        }
        if !reality_settings.is_empty() {
            stream.insert(
                "realitySettings".to_string(),
                Value::Object(reality_settings),
            );
            has_non_default_options = true;
        }
    }
    if has_non_default_options {
        Some(Value::Object(stream))
    } else {
        None
    }
}

fn query_flag_true(query: &HashMap<String, String>, key: &str) -> bool {
    query.get(key).is_some_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

