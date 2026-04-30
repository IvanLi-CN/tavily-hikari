use std::io::Cursor;

#[derive(Debug, Clone)]
pub(crate) struct EncodedRequestLogBody {
    pub(crate) body: Vec<u8>,
    pub(crate) codec: Option<&'static str>,
    pub(crate) uncompressed_bytes: Option<i64>,
}

fn normalize_body_codec(codec: Option<&str>) -> Option<&str> {
    codec
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
}

pub(crate) fn encode_request_log_body_for_storage(
    body: &[u8],
) -> Result<EncodedRequestLogBody, ProxyError> {
    encode_request_log_body_for_storage_with_enabled(body, request_log_body_compression_enabled())
}

fn encode_request_log_body_for_storage_with_enabled(
    body: &[u8],
    compression_enabled: bool,
) -> Result<EncodedRequestLogBody, ProxyError> {
    let threshold = request_log_body_compression_threshold_bytes();
    if !compression_enabled || body.len() < threshold {
        return Ok(EncodedRequestLogBody {
            body: body.to_vec(),
            codec: None,
            uncompressed_bytes: None,
        });
    }

    let encoded = zstd::stream::encode_all(Cursor::new(body), 3)
        .map_err(|err| ProxyError::Other(format!("request log body zstd encode failed: {err}")))?;
    if encoded.len() >= body.len() {
        return Ok(EncodedRequestLogBody {
            body: body.to_vec(),
            codec: None,
            uncompressed_bytes: None,
        });
    }

    Ok(EncodedRequestLogBody {
        body: encoded,
        codec: Some(REQUEST_LOG_BODY_CODEC_ZSTD),
        uncompressed_bytes: Some(body.len() as i64),
    })
}

pub(crate) fn decode_request_log_body_from_storage(
    body: Option<Vec<u8>>,
    codec: Option<&str>,
) -> Result<Option<Vec<u8>>, ProxyError> {
    let Some(body) = body else {
        return Ok(None);
    };
    match normalize_body_codec(codec) {
        None => Ok(Some(body)),
        Some(REQUEST_LOG_BODY_CODEC_ZSTD) => zstd::stream::decode_all(Cursor::new(body))
            .map(Some)
            .map_err(|err| {
                ProxyError::Other(format!("request log body zstd decode failed: {err}"))
            }),
        Some(codec) => Err(ProxyError::Other(format!(
            "unsupported request log body codec: {codec}"
        ))),
    }
}

#[derive(Debug)]
struct RequestLogBodyMigrationRow {
    id: i64,
    path: String,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    request_body: Option<Vec<u8>>,
    request_body_codec: Option<String>,
    response_body: Option<Vec<u8>>,
    response_body_codec: Option<String>,
    counts_business_quota: Option<i64>,
    request_value_bucket: Option<String>,
}

impl KeyStore {
    pub(crate) async fn migrate_request_log_bodies_batch(
        &self,
        max_rows: i64,
        max_raw_bytes: i64,
    ) -> Result<RequestLogBodyMigrationBatch, ProxyError> {
        let max_rows = max_rows.max(1);
        let max_raw_bytes = max_raw_bytes.max(1);
        let cursor_before = self
            .get_meta_i64(META_KEY_REQUEST_LOG_BODY_COMPRESSION_CURSOR_V1)
            .await?
            .unwrap_or(0);
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                path,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                request_body,
                request_body_codec,
                response_body,
                response_body_codec,
                counts_business_quota,
                request_value_bucket
            FROM request_logs
            WHERE id > ?
              AND (
                   request_body_codec IS NULL
                OR response_body_codec IS NULL
                OR counts_business_quota IS NULL
                OR request_value_bucket IS NULL
              )
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(cursor_before)
        .bind(max_rows)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            self.set_meta_i64(META_KEY_REQUEST_LOG_BODY_COMPRESSION_DONE_V1, 1)
                .await?;
            return Ok(RequestLogBodyMigrationBatch {
                cursor_before,
                cursor_after: cursor_before,
                done: true,
                ..RequestLogBodyMigrationBatch::default()
            });
        }

        let mut selected = Vec::new();
        let mut raw_bytes = 0_i64;
        for row in rows {
            let parsed = RequestLogBodyMigrationRow {
                id: row.try_get("id")?,
                path: row.try_get("path")?,
                request_kind_key: row.try_get("request_kind_key")?,
                request_kind_label: row.try_get("request_kind_label")?,
                request_kind_detail: row.try_get("request_kind_detail")?,
                request_body: row.try_get("request_body")?,
                request_body_codec: row.try_get("request_body_codec")?,
                response_body: row.try_get("response_body")?,
                response_body_codec: row.try_get("response_body_codec")?,
                counts_business_quota: row.try_get("counts_business_quota")?,
                request_value_bucket: row.try_get("request_value_bucket")?,
            };
            let row_raw_bytes = parsed
                .request_body
                .as_ref()
                .map(|body| body.len() as i64)
                .unwrap_or(0)
                + parsed
                    .response_body
                    .as_ref()
                    .map(|body| body.len() as i64)
                    .unwrap_or(0);
            if !selected.is_empty() && raw_bytes + row_raw_bytes > max_raw_bytes {
                break;
            }
            raw_bytes += row_raw_bytes;
            selected.push(parsed);
        }

        let mut tx = self.pool.begin().await?;
        let mut report = RequestLogBodyMigrationBatch {
            scanned_rows: selected.len() as i64,
            raw_bytes,
            cursor_before,
            ..RequestLogBodyMigrationBatch::default()
        };

        for row in selected {
            let decoded_request_body = decode_request_log_body_from_storage(
                row.request_body.clone(),
                row.request_body_codec.as_deref(),
            )?;
            let decoded_response_body = decode_request_log_body_from_storage(
                row.response_body.clone(),
                row.response_body_codec.as_deref(),
            )?;
            let stored_request_kind = canonicalize_request_log_request_kind(
                row.path.as_str(),
                decoded_request_body.as_deref(),
                row.request_kind_key.clone(),
                row.request_kind_label.clone(),
                row.request_kind_detail.clone(),
            );
            let counts_business_quota = row.counts_business_quota.unwrap_or_else(|| {
                if request_log_counts_business_quota(
                    &stored_request_kind.key,
                    decoded_request_body.as_deref(),
                ) {
                    1
                } else {
                    0
                }
            });
            let request_value_bucket = row.request_value_bucket.unwrap_or_else(|| {
                request_value_bucket_for_request_log(
                    &stored_request_kind.key,
                    decoded_request_body.as_deref(),
                )
                .as_str()
                .to_string()
            });

            let request_body_before = row
                .request_body
                .as_ref()
                .map(|body| body.len() as i64)
                .unwrap_or(0);
            let response_body_before = row
                .response_body
                .as_ref()
                .map(|body| body.len() as i64)
                .unwrap_or(0);

            let stored_request_body = if let Some(decoded) = decoded_request_body.as_deref() {
                if row.request_body_codec.is_none() {
                    let encoded = encode_request_log_body_for_storage_with_enabled(decoded, true)?;
                    (
                        Some(encoded.body),
                        encoded.codec,
                        encoded.uncompressed_bytes,
                    )
                } else {
                    (
                        row.request_body,
                        row.request_body_codec
                            .as_deref()
                            .filter(|codec| !codec.trim().is_empty())
                            .map(|_| REQUEST_LOG_BODY_CODEC_ZSTD),
                        Some(decoded.len() as i64),
                    )
                }
            } else {
                (None, None, None)
            };
            let stored_response_body = if let Some(decoded) = decoded_response_body.as_deref() {
                if row.response_body_codec.is_none() {
                    let encoded = encode_request_log_body_for_storage_with_enabled(decoded, true)?;
                    (
                        Some(encoded.body),
                        encoded.codec,
                        encoded.uncompressed_bytes,
                    )
                } else {
                    (
                        row.response_body,
                        row.response_body_codec
                            .as_deref()
                            .filter(|codec| !codec.trim().is_empty())
                            .map(|_| REQUEST_LOG_BODY_CODEC_ZSTD),
                        Some(decoded.len() as i64),
                    )
                }
            } else {
                (None, None, None)
            };

            report.stored_bytes_before += request_body_before + response_body_before;
            report.stored_bytes_after += stored_request_body
                .0
                .as_ref()
                .map(|body| body.len() as i64)
                .unwrap_or(0)
                + stored_response_body
                    .0
                    .as_ref()
                    .map(|body| body.len() as i64)
                    .unwrap_or(0);
            if stored_request_body.1.is_some() && row.request_body_codec.is_none() {
                report.compressed_fields += 1;
            }
            if stored_response_body.1.is_some() && row.response_body_codec.is_none() {
                report.compressed_fields += 1;
            }

            sqlx::query(
                r#"
                UPDATE request_logs
                SET request_body = ?,
                    request_body_codec = ?,
                    request_body_uncompressed_bytes = ?,
                    response_body = ?,
                    response_body_codec = ?,
                    response_body_uncompressed_bytes = ?,
                    counts_business_quota = ?,
                    request_value_bucket = ?
                WHERE id = ?
                "#,
            )
            .bind(stored_request_body.0)
            .bind(stored_request_body.1)
            .bind(stored_request_body.2)
            .bind(stored_response_body.0)
            .bind(stored_response_body.1)
            .bind(stored_response_body.2)
            .bind(counts_business_quota)
            .bind(request_value_bucket)
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
            report.updated_rows += 1;
            report.cursor_after = row.id;
        }

        if report.cursor_after > cursor_before {
            set_meta_i64_executor(
                &mut *tx,
                META_KEY_REQUEST_LOG_BODY_COMPRESSION_CURSOR_V1,
                report.cursor_after,
            )
            .await?;
            set_meta_i64_executor(&mut *tx, META_KEY_REQUEST_LOG_BODY_COMPRESSION_DONE_V1, 0)
                .await?;
        }
        tx.commit().await?;

        Ok(report)
    }
}
