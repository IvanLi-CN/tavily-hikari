// Per-provider quota-sync adapters (plan todo 19; amendments M4/M8/M11).
//
// Generalizes the usage-fetch seam (`reconciliation_quota_sync_api.rs` +
// `proxy_quota_sync_and_jobs.rs` probe/fetch path) into per-provider
// adapters without touching the Tavily lane's byte-stable behavior:
//
// - TAVILY: unchanged — `TAVILY_USAGE_BASE` + `/usage` + `Bearer` (the
//   legacy `fetch_usage_quota_for_secret*` family stays the only Tavily
//   path; the scheduler lanes additionally gain the M4 hard guards).
// - SERPAPI: `GET {rest_base}/account?api_key=<secret>` — documented
//   free/uncounted (M13 zero-cost validation probe). Maps
//   `searches_per_month` -> quota_limit, `total_searches_left` ->
//   quota_remaining, `plan_renewal_date` -> quota_renews_at (UTC
//   midnight), quota_synced_at = now. `account_status != "Active"` is a
//   maintenance-record NOTE only (never an auto-disable). Recovery stays
//   renewal-driven: the existing T9 `reset_monthly(RenewalDate)` policy is
//   reused (never reimplemented); when it flips THIS key exhausted->active
//   an `auto_restore_active` audit row is written (T18 handoff parity).
// - BRAVE: PASSIVE — no remote fetch anywhere in this module; the
//   scheduler never polls brave (no lane, no job type) and manual
//   sync-usage keeps returning the last-harvest snapshot (T10 API side).
// - EXA: typed unsupported — the scheduler never polls exa; the admin API
//   keeps its T10 409 `sync_unsupported` contract.
//
// M8 (binding): the SerpApi response feeds ONLY the quota fields — this
// module performs ZERO `api_key_low_quota_depletions` writes for any new
// provider (negative-asserted in tests::provider_quota_sync_adapters).
//
// Anonymity: the /account secret travels in the native `api_key` query
// slot (never a Bearer header, never a Tavily slot — M1/C1), and transport
// errors are mapped to literal messages because reqwest's `Display` would
// otherwise embed the secret-bearing URL (same discipline as the T10
// admin-side handler).

/// Maintenance vocabulary for the serpapi /account NOTE row. The shared
/// lib.rs constants stay untouched (closed Tavily-era vocabulary); this
/// additive op code never collides with an existing row kind.
const MAINTENANCE_OP_SERPAPI_ACCOUNT_STATUS_NOTE: &str = "serpapi_account_status_note";
const SERPAPI_ACCOUNT_STATUS_ACTIVE: &str = "Active";
const SERPAPI_ACCOUNT_NOTE_REASON_CODE: &str = "account_status_not_active";
const SERPAPI_RENEWAL_RECOVERY_REASON_CODE: &str = "serpapi_renewal_recovery";

/// Typed SerpApi `/account` snapshot (plan todo 19 field mapping).
///
/// `quota_renews_at` is `None` when `plan_renewal_date` is absent or not a
/// `%Y-%m-%d` UTC date; the persistence seam then COALESCE-preserves the
/// previously stored renewal date (T10 seam semantics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerpapiAccountSnapshot {
    pub quota_limit: i64,
    pub quota_remaining: i64,
    pub quota_renews_at: Option<i64>,
    pub quota_synced_at: i64,
    pub account_status: String,
}

/// Observable side effects of persisting one SerpApi /account snapshot.
/// Surfaced in the scheduled-job finish message for operator diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SerpapiAccountSyncEffects {
    /// T9 RenewalDate policy flipped THIS key exhausted -> active during
    /// the sync (audit row written for parity with T18).
    pub renewed_to_active: bool,
    /// A new account_status NOTE row was recorded (deduped against the
    /// key's latest note with the same status detail).
    pub account_note_recorded: bool,
}

/// Pure field mapping shared by the scheduler adapter and unit tests.
/// Mirrors the T10 admin-side mapping exactly: missing
/// `searches_per_month`/`total_searches_left` is a typed
/// `QuotaDataMissing`; an unparsable `plan_renewal_date` degrades to
/// `None` (COALESCE preservation downstream), never an error.
///
/// Deliberate divergence from the Tavily /usage parser: `remaining == 0`
/// is a VALID SerpApi state (run-out until renewal) and must persist so the
/// renewal-driven recovery keeps its `quota_renews_at` anchor — no
/// "limit and usage both zero" rejection here.
pub fn parse_serpapi_account_snapshot(
    payload: &Value,
    synced_at: i64,
) -> Result<SerpapiAccountSnapshot, ProxyError> {
    let (Some(quota_limit), Some(quota_remaining)) = (
        payload["searches_per_month"].as_i64(),
        payload["total_searches_left"].as_i64(),
    ) else {
        return Err(ProxyError::QuotaDataMissing {
            reason: "SerpApi account response lacks searches_per_month/total_searches_left"
                .to_owned(),
        });
    };
    let quota_renews_at = payload["plan_renewal_date"]
        .as_str()
        .and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|renewal| renewal.and_utc().timestamp());
    Ok(SerpapiAccountSnapshot {
        quota_limit,
        quota_remaining,
        quota_renews_at,
        quota_synced_at: synced_at,
        account_status: payload["account_status"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}

impl TavilyProxy {
    /// Scheduler-facing SerpApi /account fetch with the shared single
    /// outbound remote-attempt lease (M4 C4a: Tavily /usage and SerpApi
    /// /account jobs contend for ONE lease; exactly one outbound in
    /// flight). Geometry mirrors `fetch_usage_quota_for_sync_secret_with_
    /// admission`: local plan preparation OUTSIDE the lease, lease held
    /// through the outbound send, `mark_request_started` at request build.
    pub async fn fetch_serpapi_account_for_sync(
        &self,
        secret: &str,
        key_id: &str,
        admission: std::sync::Arc<crate::RemoteAttemptAdmissionController>,
        manual_remote_attempt: bool,
    ) -> Result<SerpapiAccountSnapshot, ProxyError> {
        let base = self.provider_rest_base(providers::ProviderId::Serpapi)?;
        let url = build_path_prefixed_url(&base, "/account");
        let plan = self.prepare_forward_proxy_plan(key_id).await;
        let remote_attempt = if manual_remote_attempt {
            admission.acquire_manual_attempt().await
        } else {
            admission.acquire_attempt().await
        }
        .map_err(|reason| ProxyError::Other(reason.to_string()))?;

        let secret_param = secret.to_string();
        let request_url = url.clone();
        let send_result = self
            .send_with_forward_proxy_plan(
                key_id,
                Some(key_id),
                "quota_sync",
                plan,
                |client| {
                    remote_attempt.mark_request_started();
                    client
                        .get(request_url.clone())
                        .query(&[("api_key", secret_param.as_str())])
                        .timeout(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS))
                },
            )
            .await;
        let synced_at = self.backend_time.now_ts();
        let (resp, _relay_lease) = match send_result {
            Ok(pair) => pair,
            // Secret-safety: reqwest Display embeds the query string, which
            // carries the SerpApi key. Only a literal, timeout-classified
            // message may surface (T10 admin-path discipline).
            Err(ProxyError::Http(error)) => {
                return Err(ProxyError::Other(if error.is_timeout() {
                    "serpapi account request timed out".to_string()
                } else {
                    "serpapi account request transport failed".to_string()
                }));
            }
            Err(err) => return Err(err),
        };
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|_| ProxyError::Other("serpapi account response body read failed".into()))?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Err(ProxyError::UsageHttp { status, body });
        }
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ProxyError::Other(format!("invalid serpapi account json: {}", e)))?;
        parse_serpapi_account_snapshot(&json, synced_at)
    }

    /// Persist one SerpApi /account snapshot (quota fields ONLY — M8), then
    /// reuse the existing T9 renewal-driven recovery and record the T18-parity
    /// audit rows. Caller (scheduler gate) holds the job-execution and
    /// maintenance gates around this call, mirroring
    /// `record_quota_sync_result`.
    pub async fn record_serpapi_account_sync(
        &self,
        key_id: &str,
        snapshot: &SerpapiAccountSnapshot,
        source: &str,
    ) -> Result<SerpapiAccountSyncEffects, ProxyError> {
        let before = self.key_store.fetch_key_state_snapshot(key_id).await?;
        self.update_provider_key_quota(
            key_id,
            snapshot.quota_limit,
            snapshot.quota_remaining,
            snapshot.quota_renews_at,
            snapshot.quota_synced_at,
        )
        .await?;
        // Renewal-driven recovery: reuse the T9 policy sweep (RenewalDate)
        // so passing quota_renews_at flips exhausted -> active NOW instead
        // of waiting for the next acquire-path sweep. Never reimplemented.
        self.key_store
            .reset_monthly(providers::ProviderId::Serpapi)
            .await?;
        let after = self.key_store.fetch_key_state_snapshot(key_id).await?;
        let mut effects = SerpapiAccountSyncEffects::default();
        let renewed_to_active = before.status.as_deref() == Some(STATUS_EXHAUSTED)
            && after.status.as_deref() == Some(STATUS_ACTIVE);
        if renewed_to_active {
            let created_at = self.backend_time.now_ts();
            self.key_store
                .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                    id: nanoid!(12),
                    key_id: key_id.to_string(),
                    source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                    operation_code: MAINTENANCE_OP_AUTO_RESTORE_ACTIVE.to_string(),
                    operation_summary: "自动恢复为 active".to_string(),
                    reason_code: Some(SERPAPI_RENEWAL_RECOVERY_REASON_CODE.to_string()),
                    reason_summary: Some("SerpApi 计划续期，额度恢复".to_string()),
                    reason_detail: Some(format!(
                        "source={source} quota_renews_at={}",
                        snapshot
                            .quota_renews_at
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string())
                    )),
                    request_log_id: None,
                    auth_token_log_id: None,
                    auth_token_id: None,
                    actor_user_id: None,
                    actor_display_name: None,
                    status_before: before.status.clone(),
                    status_after: after.status.clone(),
                    quarantine_before: before.quarantined,
                    quarantine_after: after.quarantined,
                    created_at,
                })
                .await?;
        }
        effects.renewed_to_active = renewed_to_active;
        if snapshot.account_status != SERPAPI_ACCOUNT_STATUS_ACTIVE {
            effects.account_note_recorded = self
                .record_serpapi_account_status_note(key_id, &snapshot.account_status, source)
                .await?;
        }
        // Success clears provider-agnostic transient backoffs exactly like
        // the Tavily sync path (scopes include serpapi's http_global arm).
        self.clear_transient_backoffs_after_success(key_id, source, None)
            .await?;
        Ok(effects)
    }

    /// account_status != "Active" => NOTE row ONLY (never a status write,
    /// never a disable/quarantine). Deduped: a repeated sync with the same
    /// status does not stack rows (one bounded row per status transition,
    /// matching the 24h cold-lane cadence).
    async fn record_serpapi_account_status_note(
        &self,
        key_id: &str,
        account_status: &str,
        source: &str,
    ) -> Result<bool, ProxyError> {
        let reason_detail = format!("account_status={account_status} source={source}");
        let latest_detail: Option<String> = sqlx::query_scalar(
            "SELECT reason_detail FROM api_key_maintenance_records \
             WHERE key_id = ? AND operation_code = ? \
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
        )
        .bind(key_id)
        .bind(MAINTENANCE_OP_SERPAPI_ACCOUNT_STATUS_NOTE)
        .fetch_optional(&self.key_store.pool)
        .await?;
        if latest_detail.as_deref() == Some(reason_detail.as_str()) {
            return Ok(false);
        }
        let snapshot = self.key_store.fetch_key_state_snapshot(key_id).await?;
        let now = self.backend_time.now_ts();
        self.key_store
            .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                id: nanoid!(12),
                key_id: key_id.to_string(),
                source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                operation_code: MAINTENANCE_OP_SERPAPI_ACCOUNT_STATUS_NOTE.to_string(),
                operation_summary: "SerpApi 账户状态提示".to_string(),
                reason_code: Some(SERPAPI_ACCOUNT_NOTE_REASON_CODE.to_string()),
                reason_summary: Some(format!("SerpApi 账户状态非 Active：{account_status}")),
                reason_detail: Some(reason_detail),
                request_log_id: None,
                auth_token_log_id: None,
                auth_token_id: None,
                actor_user_id: None,
                actor_display_name: None,
                status_before: snapshot.status.clone(),
                status_after: snapshot.status.clone(),
                quarantine_before: snapshot.quarantined,
                quarantine_after: snapshot.quarantined,
                created_at: now,
            })
            .await?;
        tracing::warn!(
            component = "quota_sync",
            event = "serpapi_account_status_note",
            provider = %providers::ProviderId::Serpapi,
            api_key_id = %key_id,
            account_status = %account_status,
            "serpapi /account reported a non-Active account status (note only, key NOT disabled)"
        );
        Ok(true)
    }

    /// M4 hard guard for the Tavily /usage probe seam
    /// (`probe_api_key_quota` family): a STORED non-Tavily secret is
    /// rejected with a typed error BEFORE any outbound request, so the
    /// Tavily usage endpoint can never receive a third-party key. Unstored
    /// secrets pass through untouched — registration/validation flows probe
    /// candidate keys before they exist (byte-stable for all Tavily flows).
    pub(crate) async fn guard_tavily_probe_secret(&self, api_key: &str) -> Result<(), ProxyError> {
        match self.key_store.fetch_api_key_provider_by_value(api_key).await? {
            Some(provider) if provider != providers::ProviderId::Tavily => {
                Err(ProxyError::Other(format!(
                    "provider_guard_rejected: tavily usage probe is tavily-only, stored key provider is {provider}"
                )))
            }
            _ => Ok(()),
        }
    }
}
