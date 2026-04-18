pub async fn ensure_forward_proxy_schema(pool: &SqlitePool) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_settings (
            id INTEGER PRIMARY KEY,
            proxy_urls_json TEXT NOT NULL DEFAULT '[]',
            subscription_urls_json TEXT NOT NULL DEFAULT '[]',
            subscription_update_interval_secs INTEGER NOT NULL DEFAULT 3600,
            insert_direct INTEGER NOT NULL DEFAULT 1,
            egress_socks5_enabled INTEGER NOT NULL DEFAULT 0,
            egress_socks5_url TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    ensure_forward_proxy_settings_column(
        pool,
        "egress_socks5_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    ensure_forward_proxy_settings_column(pool, "egress_socks5_url", "TEXT NOT NULL DEFAULT ''")
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_runtime (
            proxy_key TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            source TEXT NOT NULL,
            endpoint_url TEXT,
            resolved_ip_source TEXT NOT NULL DEFAULT '',
            resolved_ips_json TEXT NOT NULL DEFAULT '[]',
            resolved_regions_json TEXT NOT NULL DEFAULT '[]',
            geo_refreshed_at INTEGER NOT NULL DEFAULT 0,
            weight REAL NOT NULL,
            success_ema REAL NOT NULL,
            latency_ema_ms REAL,
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            is_penalized INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    ensure_forward_proxy_runtime_column(pool, "resolved_ips_json", "TEXT NOT NULL DEFAULT '[]'")
        .await?;
    ensure_forward_proxy_runtime_column(
        pool,
        "resolved_regions_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )
    .await?;
    ensure_forward_proxy_runtime_column(pool, "resolved_ip_source", "TEXT NOT NULL DEFAULT ''")
        .await?;
    ensure_forward_proxy_runtime_column(pool, "geo_refreshed_at", "INTEGER NOT NULL DEFAULT 0")
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_attempts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            proxy_key TEXT NOT NULL,
            is_success INTEGER NOT NULL,
            latency_ms REAL,
            failure_kind TEXT,
            is_probe INTEGER NOT NULL DEFAULT 0,
            occurred_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_weight_hourly (
            proxy_key TEXT NOT NULL,
            bucket_start_epoch INTEGER NOT NULL,
            sample_count INTEGER NOT NULL,
            min_weight REAL NOT NULL,
            max_weight REAL NOT NULL,
            avg_weight REAL NOT NULL,
            last_weight REAL NOT NULL,
            last_sample_epoch_us INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            PRIMARY KEY (proxy_key, bucket_start_epoch)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_key_affinity (
            key_id TEXT PRIMARY KEY,
            primary_proxy_key TEXT,
            secondary_proxy_key TEXT,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_forward_proxy_attempts_proxy_time
           ON forward_proxy_attempts (proxy_key, occurred_at)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_forward_proxy_attempts_time_proxy
           ON forward_proxy_attempts (occurred_at, proxy_key)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_forward_proxy_weight_hourly_time_proxy
           ON forward_proxy_weight_hourly (bucket_start_epoch, proxy_key)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO forward_proxy_settings (
            id,
            proxy_urls_json,
            subscription_urls_json,
            subscription_update_interval_secs,
            insert_direct,
            egress_socks5_enabled,
            egress_socks5_url,
            updated_at
        ) VALUES (?1, '[]', '[]', ?2, ?3, 0, '', strftime('%s', 'now'))
        "#,
    )
    .bind(FORWARD_PROXY_SETTINGS_SINGLETON_ID)
    .bind(DEFAULT_FORWARD_PROXY_SUBSCRIPTION_INTERVAL_SECS as i64)
    .bind(DEFAULT_FORWARD_PROXY_INSERT_DIRECT as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn load_forward_proxy_settings(
    pool: &SqlitePool,
) -> Result<ForwardProxySettings, ProxyError> {
    let row = sqlx::query_as::<_, ForwardProxySettingsRow>(
        r#"
        SELECT
            proxy_urls_json,
            subscription_urls_json,
            subscription_update_interval_secs,
            insert_direct,
            egress_socks5_enabled,
            egress_socks5_url
        FROM forward_proxy_settings
        WHERE id = ?1
        LIMIT 1
        "#,
    )
    .bind(FORWARD_PROXY_SETTINGS_SINGLETON_ID)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into).unwrap_or_default())
}

pub async fn save_forward_proxy_settings(
    pool: &SqlitePool,
    settings: ForwardProxySettings,
) -> Result<(), ProxyError> {
    let normalized = settings.normalized();
    let proxy_urls_json = serde_json::to_string(&normalized.proxy_urls).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy urls: {err}"))
    })?;
    let subscription_urls_json =
        serde_json::to_string(&normalized.subscription_urls).map_err(|err| {
            ProxyError::Other(format!(
                "failed to serialize forward proxy subscription urls: {err}"
            ))
        })?;
    sqlx::query(
        r#"
        UPDATE forward_proxy_settings
        SET proxy_urls_json = ?1,
            subscription_urls_json = ?2,
            subscription_update_interval_secs = ?3,
            insert_direct = ?4,
            egress_socks5_enabled = ?5,
            egress_socks5_url = ?6,
            updated_at = strftime('%s', 'now')
        WHERE id = ?7
        "#,
    )
    .bind(proxy_urls_json)
    .bind(subscription_urls_json)
    .bind(normalized.subscription_update_interval_secs as i64)
    .bind(normalized.insert_direct as i64)
    .bind(normalized.egress_socks5_enabled as i64)
    .bind(normalized.egress_socks5_url)
    .bind(FORWARD_PROXY_SETTINGS_SINGLETON_ID)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_forward_proxy_runtime_states(
    pool: &SqlitePool,
) -> Result<Vec<ForwardProxyRuntimeState>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyRuntimeRow>(
        r#"
        SELECT
            proxy_key,
            display_name,
            source,
            endpoint_url,
            resolved_ip_source,
            resolved_ips_json,
            resolved_regions_json,
            geo_refreshed_at,
            weight,
            success_ema,
            latency_ema_ms,
            consecutive_failures
        FROM forward_proxy_runtime
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn persist_forward_proxy_runtime_snapshot(
    pool: &SqlitePool,
    runtime_snapshot: Vec<ForwardProxyRuntimeState>,
) -> Result<(), ProxyError> {
    let active_keys = runtime_snapshot
        .iter()
        .map(|entry| entry.proxy_key.clone())
        .collect::<Vec<_>>();
    delete_forward_proxy_runtime_rows_not_in(pool, &active_keys).await?;
    for runtime in &runtime_snapshot {
        persist_forward_proxy_runtime_state(pool, runtime).await?;
    }
    Ok(())
}

pub async fn persist_forward_proxy_runtime_states_atomic(
    pool: &SqlitePool,
    states: &[ForwardProxyRuntimeState],
) -> Result<(), ProxyError> {
    if states.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for state in states {
        persist_forward_proxy_runtime_state_tx(&mut tx, state).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn persist_forward_proxy_runtime_geo_metadata_atomic(
    pool: &SqlitePool,
    updates: &[ForwardProxyRuntimeGeoMetadataUpdate],
) -> Result<(), ProxyError> {
    if updates.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for update in updates {
        persist_forward_proxy_runtime_geo_metadata_tx(&mut tx, update).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn persist_forward_proxy_runtime_health_state(
    pool: &SqlitePool,
    state: &ForwardProxyRuntimeState,
) -> Result<(), ProxyError> {
    let resolved_ips_json = serde_json::to_string(&state.resolved_ips).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy ips: {err}"))
    })?;
    let resolved_regions_json = serde_json::to_string(&state.resolved_regions).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy regions: {err}"))
    })?;
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_runtime (
            proxy_key,
            display_name,
            source,
            endpoint_url,
            resolved_ip_source,
            resolved_ips_json,
            resolved_regions_json,
            geo_refreshed_at,
            weight,
            success_ema,
            latency_ema_ms,
            consecutive_failures,
            is_penalized,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, strftime('%s', 'now'))
        ON CONFLICT(proxy_key) DO UPDATE SET
            display_name = excluded.display_name,
            source = excluded.source,
            endpoint_url = excluded.endpoint_url,
            weight = excluded.weight,
            success_ema = excluded.success_ema,
            latency_ema_ms = excluded.latency_ema_ms,
            consecutive_failures = excluded.consecutive_failures,
            is_penalized = excluded.is_penalized,
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(&state.proxy_key)
    .bind(&state.display_name)
    .bind(&state.source)
    .bind(&state.endpoint_url)
    .bind(&state.resolved_ip_source)
    .bind(resolved_ips_json)
    .bind(resolved_regions_json)
    .bind(state.geo_refreshed_at)
    .bind(state.weight)
    .bind(state.success_ema)
    .bind(state.latency_ema_ms)
    .bind(i64::from(state.consecutive_failures))
    .bind(state.is_penalized() as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn persist_forward_proxy_runtime_state(
    pool: &SqlitePool,
    state: &ForwardProxyRuntimeState,
) -> Result<(), ProxyError> {
    let mut tx = pool.begin().await?;
    persist_forward_proxy_runtime_state_tx(&mut tx, state).await?;
    tx.commit().await?;
    Ok(())
}

async fn persist_forward_proxy_runtime_state_tx(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    state: &ForwardProxyRuntimeState,
) -> Result<(), ProxyError> {
    let resolved_ips_json = serde_json::to_string(&state.resolved_ips).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy ips: {err}"))
    })?;
    let resolved_regions_json = serde_json::to_string(&state.resolved_regions).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy regions: {err}"))
    })?;
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_runtime (
            proxy_key,
            display_name,
            source,
            endpoint_url,
            resolved_ip_source,
            resolved_ips_json,
            resolved_regions_json,
            geo_refreshed_at,
            weight,
            success_ema,
            latency_ema_ms,
            consecutive_failures,
            is_penalized,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, strftime('%s', 'now'))
        ON CONFLICT(proxy_key) DO UPDATE SET
            display_name = excluded.display_name,
            source = excluded.source,
            endpoint_url = excluded.endpoint_url,
            resolved_ip_source = excluded.resolved_ip_source,
            resolved_ips_json = excluded.resolved_ips_json,
            resolved_regions_json = excluded.resolved_regions_json,
            geo_refreshed_at = excluded.geo_refreshed_at,
            weight = excluded.weight,
            success_ema = excluded.success_ema,
            latency_ema_ms = excluded.latency_ema_ms,
            consecutive_failures = excluded.consecutive_failures,
            is_penalized = excluded.is_penalized,
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(&state.proxy_key)
    .bind(&state.display_name)
    .bind(&state.source)
    .bind(&state.endpoint_url)
    .bind(&state.resolved_ip_source)
    .bind(resolved_ips_json)
    .bind(resolved_regions_json)
    .bind(state.geo_refreshed_at)
    .bind(state.weight)
    .bind(state.success_ema)
    .bind(state.latency_ema_ms)
    .bind(i64::from(state.consecutive_failures))
    .bind(state.is_penalized() as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn persist_forward_proxy_runtime_geo_metadata_tx(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    update: &ForwardProxyRuntimeGeoMetadataUpdate,
) -> Result<(), ProxyError> {
    let resolved_ips_json = serde_json::to_string(&update.resolved_ips).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy ips: {err}"))
    })?;
    let resolved_regions_json = serde_json::to_string(&update.resolved_regions).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy regions: {err}"))
    })?;
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_runtime (
            proxy_key,
            display_name,
            source,
            endpoint_url,
            resolved_ip_source,
            resolved_ips_json,
            resolved_regions_json,
            geo_refreshed_at,
            weight,
            success_ema,
            latency_ema_ms,
            consecutive_failures,
            is_penalized,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, strftime('%s', 'now'))
        ON CONFLICT(proxy_key) DO UPDATE SET
            resolved_ip_source = excluded.resolved_ip_source,
            resolved_ips_json = excluded.resolved_ips_json,
            resolved_regions_json = excluded.resolved_regions_json,
            geo_refreshed_at = excluded.geo_refreshed_at,
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(&update.proxy_key)
    .bind(&update.display_name)
    .bind(&update.source)
    .bind(&update.endpoint_url)
    .bind(&update.resolved_ip_source)
    .bind(resolved_ips_json)
    .bind(resolved_regions_json)
    .bind(update.geo_refreshed_at)
    .bind(update.weight)
    .bind(update.success_ema)
    .bind(update.latency_ema_ms)
    .bind(i64::from(update.consecutive_failures))
    .bind(update.is_penalized as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_forward_proxy_runtime_column(
    pool: &SqlitePool,
    column_name: &str,
    column_def: &str,
) -> Result<(), ProxyError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pragma_table_info('forward_proxy_runtime') WHERE name = ?1",
    )
    .bind(column_name)
    .fetch_one(pool)
    .await?;
    if exists == 0 {
        sqlx::query(&format!(
            "ALTER TABLE forward_proxy_runtime ADD COLUMN {column_name} {column_def}"
        ))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn ensure_forward_proxy_settings_column(
    pool: &SqlitePool,
    column_name: &str,
    column_def: &str,
) -> Result<(), ProxyError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pragma_table_info('forward_proxy_settings') WHERE name = ?1",
    )
    .bind(column_name)
    .fetch_one(pool)
    .await?;
    if exists == 0 {
        sqlx::query(&format!(
            "ALTER TABLE forward_proxy_settings ADD COLUMN {column_name} {column_def}"
        ))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn delete_forward_proxy_runtime_rows_not_in(
    pool: &SqlitePool,
    active_keys: &[String],
) -> Result<(), ProxyError> {
    if active_keys.is_empty() {
        sqlx::query("DELETE FROM forward_proxy_runtime")
            .execute(pool)
            .await?;
        return Ok(());
    }
    let mut builder =
        QueryBuilder::<Sqlite>::new("DELETE FROM forward_proxy_runtime WHERE proxy_key NOT IN (");
    {
        let mut separated = builder.separated(", ");
        for key in active_keys {
            separated.push_bind(key);
        }
    }
    builder.push(")");
    builder.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_forward_proxy_attempt(
    pool: &SqlitePool,
    proxy_key: &str,
    success: bool,
    latency_ms: Option<f64>,
    failure_kind: Option<&str>,
    is_probe: bool,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_attempts (proxy_key, is_success, latency_ms, failure_kind, is_probe, occurred_at)
        VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s', 'now'))
        "#,
    )
    .bind(proxy_key)
    .bind(success as i64)
    .bind(latency_ms)
    .bind(failure_kind)
    .bind(is_probe as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_forward_proxy_weight_hourly_bucket(
    pool: &SqlitePool,
    proxy_key: &str,
    bucket_start_epoch: i64,
    weight: f64,
    sample_epoch_us: i64,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_weight_hourly (
            proxy_key,
            bucket_start_epoch,
            sample_count,
            min_weight,
            max_weight,
            avg_weight,
            last_weight,
            last_sample_epoch_us,
            updated_at
        ) VALUES (?1, ?2, 1, ?3, ?3, ?3, ?3, ?4, strftime('%s', 'now'))
        ON CONFLICT(proxy_key, bucket_start_epoch) DO UPDATE SET
            sample_count = forward_proxy_weight_hourly.sample_count + 1,
            min_weight = MIN(forward_proxy_weight_hourly.min_weight, excluded.min_weight),
            max_weight = MAX(forward_proxy_weight_hourly.max_weight, excluded.max_weight),
            avg_weight = ((forward_proxy_weight_hourly.avg_weight * forward_proxy_weight_hourly.sample_count) + excluded.avg_weight)
                / (forward_proxy_weight_hourly.sample_count + 1),
            last_weight = CASE WHEN excluded.last_sample_epoch_us >= forward_proxy_weight_hourly.last_sample_epoch_us
                THEN excluded.last_weight ELSE forward_proxy_weight_hourly.last_weight END,
            last_sample_epoch_us = MAX(forward_proxy_weight_hourly.last_sample_epoch_us, excluded.last_sample_epoch_us),
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(proxy_key)
    .bind(bucket_start_epoch)
    .bind(weight)
    .bind(sample_epoch_us)
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_forward_proxy_affinity(
    pool: &SqlitePool,
    key_id: &str,
) -> Result<Option<ForwardProxyKeyAffinity>, ProxyError> {
    let row = sqlx::query(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ? LIMIT 1",
    )
    .bind(key_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ForwardProxyKeyAffinity {
        primary_proxy_key: row
            .try_get("primary_proxy_key")
            .ok()
            .filter(|value: &String| !value.trim().is_empty()),
        secondary_proxy_key: row
            .try_get("secondary_proxy_key")
            .ok()
            .filter(|value: &String| !value.trim().is_empty()),
    }))
}

async fn save_forward_proxy_affinity(
    pool: &SqlitePool,
    key_id: &str,
    affinity: &ForwardProxyKeyAffinity,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
        VALUES (?1, ?2, ?3, strftime('%s', 'now'))
        ON CONFLICT(key_id) DO UPDATE SET
            primary_proxy_key = excluded.primary_proxy_key,
            secondary_proxy_key = excluded.secondary_proxy_key,
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(key_id)
    .bind(&affinity.primary_proxy_key)
    .bind(&affinity.secondary_proxy_key)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_forward_proxy_key_affinity(
    pool: &SqlitePool,
    key_id: &str,
) -> Result<Option<ForwardProxyAffinityRecord>, ProxyError> {
    Ok(load_forward_proxy_affinity(pool, key_id)
        .await?
        .map(|record| ForwardProxyAffinityRecord {
            primary_proxy_key: record.primary_proxy_key,
            secondary_proxy_key: record.secondary_proxy_key,
            updated_at: Utc::now().timestamp(),
        }))
}

pub async fn save_forward_proxy_key_affinity(
    pool: &SqlitePool,
    key_id: &str,
    record: &ForwardProxyAffinityRecord,
) -> Result<(), ProxyError> {
    save_forward_proxy_affinity(
        pool,
        key_id,
        &ForwardProxyKeyAffinity {
            primary_proxy_key: record.primary_proxy_key.clone(),
            secondary_proxy_key: record.secondary_proxy_key.clone(),
        },
    )
    .await
}

pub async fn sync_manager_runtime_to_store(
    key_store: &KeyStore,
    manager: &ForwardProxyManager,
) -> Result<(), ProxyError> {
    persist_forward_proxy_runtime_snapshot(&key_store.pool, manager.snapshot_runtime()).await
}

async fn load_forward_proxy_assignment_counts(
    pool: &SqlitePool,
) -> Result<HashMap<String, ForwardProxyAssignmentCounts>, ProxyError> {
    let rows = sqlx::query(
        r#"
        SELECT
            proxy_key,
            SUM(primary_count) AS primary_count,
            SUM(secondary_count) AS secondary_count
        FROM (
            SELECT primary_proxy_key AS proxy_key, 1 AS primary_count, 0 AS secondary_count
            FROM forward_proxy_key_affinity
            WHERE primary_proxy_key IS NOT NULL AND primary_proxy_key != ''
            UNION ALL
            SELECT secondary_proxy_key AS proxy_key, 0 AS primary_count, 1 AS secondary_count
            FROM forward_proxy_key_affinity
            WHERE secondary_proxy_key IS NOT NULL AND secondary_proxy_key != ''
        )
        GROUP BY proxy_key
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut counts = HashMap::new();
    for row in rows {
        let proxy_key: String = row.try_get("proxy_key")?;
        let primary: i64 = row.try_get::<i64, _>("primary_count")?;
        let secondary: i64 = row.try_get::<i64, _>("secondary_count")?;
        counts.insert(
            proxy_key,
            ForwardProxyAssignmentCounts { primary, secondary },
        );
    }
    Ok(counts)
}

#[derive(Debug, FromRow)]
struct ForwardProxyAttemptStatsRow {
    proxy_key: String,
    attempts: i64,
    success_count: i64,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, FromRow)]
struct ForwardProxyHourlyStatsRow {
    proxy_key: String,
    bucket_start_epoch: i64,
    success_count: i64,
    failure_count: i64,
}

#[derive(Debug, FromRow)]
struct ForwardProxyWeightHourlyStatsRow {
    proxy_key: String,
    bucket_start_epoch: i64,
    sample_count: i64,
    min_weight: f64,
    max_weight: f64,
    avg_weight: f64,
    last_weight: f64,
}

#[derive(Debug, FromRow)]
struct ForwardProxyWeightLastBeforeRangeRow {
    proxy_key: String,
    last_weight: f64,
}

async fn query_forward_proxy_window_stats(
    pool: &SqlitePool,
    since_epoch: i64,
) -> Result<HashMap<String, ForwardProxyAttemptWindowStats>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyAttemptStatsRow>(
        r#"
        SELECT proxy_key,
               COUNT(*) AS attempts,
               SUM(CASE WHEN is_success != 0 THEN 1 ELSE 0 END) AS success_count,
               AVG(CASE WHEN is_success != 0 THEN latency_ms END) AS avg_latency_ms
        FROM forward_proxy_attempts
        WHERE occurred_at >= ?1
        GROUP BY proxy_key
        "#,
    )
    .bind(since_epoch)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.proxy_key,
                ForwardProxyAttemptWindowStats {
                    attempts: row.attempts,
                    success_count: row.success_count,
                    avg_latency_ms: row.avg_latency_ms,
                },
            )
        })
        .collect())
}

async fn query_forward_proxy_hourly_stats(
    pool: &SqlitePool,
    range_start_epoch: i64,
    range_end_epoch: i64,
) -> Result<HashMap<String, HashMap<i64, ForwardProxyHourlyStatsPoint>>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyHourlyStatsRow>(
        r#"
        SELECT proxy_key,
               (occurred_at / 3600) * 3600 AS bucket_start_epoch,
               SUM(CASE WHEN is_success != 0 THEN 1 ELSE 0 END) AS success_count,
               SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END) AS failure_count
        FROM forward_proxy_attempts
        WHERE occurred_at >= ?1 AND occurred_at < ?2
        GROUP BY proxy_key, bucket_start_epoch
        "#,
    )
    .bind(range_start_epoch)
    .bind(range_end_epoch)
    .fetch_all(pool)
    .await?;

    let mut grouped = HashMap::new();
    for row in rows {
        grouped
            .entry(row.proxy_key)
            .or_insert_with(HashMap::new)
            .insert(
                row.bucket_start_epoch,
                ForwardProxyHourlyStatsPoint {
                    success_count: row.success_count,
                    failure_count: row.failure_count,
                },
            );
    }
    Ok(grouped)
}

async fn query_forward_proxy_weight_hourly_stats(
    pool: &SqlitePool,
    range_start_epoch: i64,
    range_end_epoch: i64,
) -> Result<HashMap<String, HashMap<i64, ForwardProxyWeightHourlyStatsPoint>>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyWeightHourlyStatsRow>(
        r#"
        SELECT proxy_key, bucket_start_epoch, sample_count, min_weight, max_weight, avg_weight, last_weight
        FROM forward_proxy_weight_hourly
        WHERE bucket_start_epoch >= ?1 AND bucket_start_epoch < ?2
        "#,
    )
    .bind(range_start_epoch)
    .bind(range_end_epoch)
    .fetch_all(pool)
    .await?;
    let mut grouped = HashMap::new();
    for row in rows {
        grouped
            .entry(row.proxy_key)
            .or_insert_with(HashMap::new)
            .insert(
                row.bucket_start_epoch,
                ForwardProxyWeightHourlyStatsPoint {
                    sample_count: row.sample_count,
                    min_weight: row.min_weight,
                    max_weight: row.max_weight,
                    avg_weight: row.avg_weight,
                    last_weight: row.last_weight,
                },
            );
    }
    Ok(grouped)
}

async fn query_forward_proxy_weight_last_before(
    pool: &SqlitePool,
    range_start_epoch: i64,
    proxy_keys: &[String],
) -> Result<HashMap<String, f64>, ProxyError> {
    if proxy_keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mut builder = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT latest.proxy_key, latest.last_weight
        FROM forward_proxy_weight_hourly AS latest
        INNER JOIN (
            SELECT proxy_key, MAX(bucket_start_epoch) AS bucket_start_epoch
            FROM forward_proxy_weight_hourly
            WHERE bucket_start_epoch < "#,
    );
    builder.push_bind(range_start_epoch);
    builder.push(" AND proxy_key IN (");
    {
        let mut separated = builder.separated(", ");
        for key in proxy_keys {
            separated.push_bind(key);
        }
    }
    builder.push(") GROUP BY proxy_key) AS prior ON latest.proxy_key = prior.proxy_key AND latest.bucket_start_epoch = prior.bucket_start_epoch");
    let rows = builder
        .build_query_as::<ForwardProxyWeightLastBeforeRangeRow>()
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.proxy_key, row.last_weight))
        .collect())
}

