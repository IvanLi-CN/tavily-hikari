#[derive(Debug)]
struct ReconciliationKeyObservationSourceIdentity {
    candidate_identity: String,
    key_set_identity: String,
    key_identity_by_id: std::collections::HashMap<String, String>,
}

impl ReconciliationKeyObservationSourceIdentity {
    fn key_identity(&self, key_id: &str) -> String {
        self.key_identity_by_id
            .get(key_id)
            .cloned()
            // An observation without a current source row must never match a
            // future source row. The fallback keeps fence-only test fixtures
            // durable without creating a reusable observation.
            .unwrap_or_else(|| stable_reconciliation_identity(&["missing-source-v1", key_id]))
    }
}

fn stable_reconciliation_identity(parts: &[&str]) -> String {
    let mut encoded = String::new();
    for part in parts {
        encoded.push_str(&part.len().to_string());
        encoded.push(':');
        encoded.push_str(part);
        encoded.push('|');
    }
    sha256_hex_bytes(encoded.as_bytes())
}

/// A successful usage response for one upstream key. These rows are local,
/// rebuildable observations; the work row and settlement ledger remain the
/// replicated reconciliation truth.
pub(crate) struct ReconciliationKeyObservation {
    pub(crate) key_id: String,
    pub(crate) upstream_usage: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconciliationKeyObservationPersistOutcome {
    Persisted,
    StaleGeneration,
    StaleClaim,
}

impl KeyStore {
    pub(crate) async fn reconciliation_key_observations(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        _work_generation: i64,
        key_ids: &[String],
    ) -> Result<std::collections::HashMap<String, i64>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let source_identity = self
            .reconciliation_key_observation_source_identity(candidate)
            .await?;
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::CandidateHydrate)
            .await?;
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT key_id, upstream_usage, key_source_identity \
             FROM upstream_reconciliation_key_observations WHERE token_id = ",
        );
        query
            .push_bind(&candidate.token_id)
            .push(" AND period_code = ")
            .push_bind(&candidate.period_code)
            .push(" AND candidate_identity = ")
            .push_bind(&source_identity.candidate_identity)
            .push(" AND key_set_identity = ")
            .push_bind(&source_identity.key_set_identity)
            .push(" AND key_id IN (");
        {
            let mut separated = query.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        query.push(") AND (");
        for (index, key_id) in key_ids.iter().enumerate() {
            if index > 0 {
                query.push(" OR ");
            }
            query
                .push("(key_id = ")
                .push_bind(key_id)
                .push(" AND key_source_identity = ")
                .push_bind(source_identity.key_identity(key_id))
                .push(")");
        }
        query.push(") ORDER BY observed_at DESC");
        let rows_result = query
            .build_query_as::<(String, i64, String)>()
            .fetch_all(&mut *session)
            .await;
        let rows = session.complete_query_or_defer(rows_result).await?;
        let matching = rows.into_iter().fold(
            std::collections::HashMap::new(),
            |mut matching, (key_id, upstream_usage, _key_source_identity)| {
                matching.entry(key_id).or_insert(upstream_usage);
                matching
            },
        );
        self.sqlite_runtime.record_reconciliation_key_observation_identity(
            matching.len() as u64,
            key_ids.len().saturating_sub(matching.len()) as u64,
        );
        Ok(matching)
    }

    pub(crate) async fn persist_reconciliation_key_observations(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        work_generation: i64,
        observations: &[ReconciliationKeyObservation],
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<ReconciliationKeyObservationPersistOutcome, ProxyError> {
        if observations.is_empty() {
            return Ok(ReconciliationKeyObservationPersistOutcome::Persisted);
        }
        // Derive the identity before BEGIN IMMEDIATE. A source read is subject
        // to the reconciliation read budget and must never hold the writer.
        // The claim/generation fence below rejects an identity made stale in
        // the short interval before the durable observation commit.
        let source_identity = self
            .reconciliation_key_observation_source_identity(candidate)
            .await?;
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;

        match self
            .reconciliation_key_observation_fence_status(&mut tx, candidate, work_generation, fence)
            .await?
        {
            ReconciliationKeyObservationPersistOutcome::Persisted => {}
            stale => {
                tx.rollback().await?;
                return Ok(stale);
            }
        }
        if !self
            .lock_reconciliation_work_generation(&mut tx, candidate, fence)
            .await?
        {
            tx.rollback().await?;
            return Ok(ReconciliationKeyObservationPersistOutcome::StaleClaim);
        }
        // Retain obsolete observations until terminal completion. The source
        // identities below, rather than work_generation alone, control reuse.
        for observation in observations {
            sqlx::query(
                r#"INSERT INTO upstream_reconciliation_key_observations (
                       token_id, period_code, work_generation, key_id,
                       upstream_usage, candidate_identity, key_set_identity,
                       key_source_identity, observed_at
                   ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                   ON CONFLICT(token_id, period_code, work_generation, key_id)
                   DO UPDATE SET upstream_usage = excluded.upstream_usage,
                                 candidate_identity = excluded.candidate_identity,
                                 key_set_identity = excluded.key_set_identity,
                                 key_source_identity = excluded.key_source_identity,
                                 observed_at = excluded.observed_at"#,
            )
            .bind(&candidate.token_id)
            .bind(&candidate.period_code)
            .bind(work_generation)
            .bind(&observation.key_id)
            .bind(observation.upstream_usage)
            .bind(&source_identity.candidate_identity)
            .bind(&source_identity.key_set_identity)
            .bind(source_identity.key_identity(&observation.key_id))
            .bind(self.backend_time.now_ts())
            .execute(&mut *tx)
            .await?;
        }
        tx.finish(Ok(())).await?;
        Ok(ReconciliationKeyObservationPersistOutcome::Persisted)
    }

    async fn reconciliation_key_observation_source_rows(
        &self,
        connection: &mut sqlx::SqliteConnection,
        candidate: &UpstreamReconciliationCandidate,
    ) -> Result<Vec<(String, i64, i64, i64)>, sqlx::Error> {
        sqlx::query_as::<_, (String, i64, i64, i64)>(
            "SELECT key_id, request_count, first_used_at, last_used_at \
             FROM upstream_reconciliation_usage \
             WHERE token_id = ? AND period_code = ? ORDER BY key_id ASC",
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .fetch_all(connection)
        .await
    }

    async fn reconciliation_key_observation_source_identity(
        &self,
        candidate: &UpstreamReconciliationCandidate,
    ) -> Result<ReconciliationKeyObservationSourceIdentity, ProxyError> {
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::CandidateHydrate)
            .await?;
        let rows_result = self
            .reconciliation_key_observation_source_rows(&mut session, candidate)
            .await;
        let rows = session.complete_query_or_defer(rows_result).await?;
        Ok(Self::reconciliation_key_observation_source_identity_from_rows(
            candidate, rows,
        ))
    }

    fn reconciliation_key_observation_source_identity_from_rows(
        candidate: &UpstreamReconciliationCandidate,
        source_rows: Vec<(String, i64, i64, i64)>,
    ) -> ReconciliationKeyObservationSourceIdentity {
        let key_set_parts = std::iter::once("key-set-v1")
            .chain(source_rows.iter().map(|(key_id, ..)| key_id.as_str()))
            .collect::<Vec<_>>();
        let key_set_identity = stable_reconciliation_identity(&key_set_parts);
        let key_identity_by_id = source_rows
            .into_iter()
            .map(|(key_id, request_count, first_used_at, last_used_at)| {
                let request_count = request_count.to_string();
                let first_used_at = first_used_at.to_string();
                let last_used_at = last_used_at.to_string();
                let key_identity = stable_reconciliation_identity(&[
                    "key-source-v1",
                    &key_id,
                    &request_count,
                    &first_used_at,
                    &last_used_at,
                ]);
                (key_id, key_identity)
            })
            .collect();
        let period_start = candidate.period_start.to_string();
        let period_end = candidate.period_end.to_string();
        ReconciliationKeyObservationSourceIdentity {
            candidate_identity: stable_reconciliation_identity(&[
                "candidate-source-v1",
                &candidate.token_id,
                &candidate.period_code,
                &candidate.project_id,
                &candidate.billing_subject,
                &candidate.settlement_mode,
                &period_start,
                &period_end,
            ]),
            key_set_identity,
            key_identity_by_id,
        }
    }

    async fn reconciliation_key_observation_fence_status(
        &self,
        tx: &mut SqliteImmediateTransaction,
        candidate: &UpstreamReconciliationCandidate,
        work_generation: i64,
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<ReconciliationKeyObservationPersistOutcome, ProxyError> {
        if fence.is_none() {
            return Ok(ReconciliationKeyObservationPersistOutcome::Persisted);
        }
        if let Some((job_id, claim_generation)) = fence.and_then(|fence| fence.claimed_job) {
            let current_claim = sqlx::query_scalar::<_, i64>(
                "SELECT claim_generation FROM scheduled_jobs
                  WHERE id = ? AND status = 'running'",
            )
            .bind(job_id)
            .fetch_optional(&mut **tx)
            .await?;
            if current_claim != Some(claim_generation) {
                return Ok(ReconciliationKeyObservationPersistOutcome::StaleClaim);
            }
        }

        let Some((current_generation, completed_generation)) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT work_generation, completed_generation
               FROM upstream_reconciliation_work
              WHERE token_id = ? AND period_code = ?",
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .fetch_optional(&mut **tx)
        .await?
        else {
            return Ok(ReconciliationKeyObservationPersistOutcome::StaleGeneration);
        };

        if current_generation != work_generation || completed_generation >= current_generation {
            return Ok(ReconciliationKeyObservationPersistOutcome::StaleGeneration);
        }

        Ok(ReconciliationKeyObservationPersistOutcome::Persisted)
    }

    async fn clear_reconciliation_key_observations(
        &self,
        tx: &mut SqliteImmediateTransaction,
        candidate: &UpstreamReconciliationCandidate,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            "DELETE FROM upstream_reconciliation_key_observations \
             WHERE token_id = ? AND period_code = ?",
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}
