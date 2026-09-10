use super::AlertEventProjectionRow;
use crate::{AlertEventRecord, ProxyError};

// Alerts are a display projection, not a second raw-error archive. The raw
// request and job records remain authoritative; every copied diagnostic field
// is bounded before it can be repeated in Events and Groups payloads.
pub(crate) const ALERT_EVENT_DISPLAY_TEXT_MAX_CHARS: usize = 1024;
pub(crate) const ALERT_EVENT_PROJECTION_MAX_BYTES: usize = 64 * 1024;
pub(crate) const ALERT_EVENT_IDENTIFIER_MAX_CHARS: usize = 256;

fn is_sensitive_alert_display_key(key: &str) -> bool {
    let key = key
        .trim()
        .trim_matches(|character| matches!(character, '?' | '&' | '"' | '\'' | ':' | ' '))
        .to_ascii_lowercase();
    key.contains("api_key")
        || key.contains("apikey")
        || key.contains("access_token")
        || key.contains("refresh_token")
        || key == "token"
        || key.ends_with("_token")
        || key.contains("password")
        || key.contains("secret")
        || key.contains("authorization")
        || key.contains("credential")
        || key.contains("private_key")
}

fn redact_sensitive_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                if is_sensitive_alert_display_key(key) {
                    *value = serde_json::Value::String("***redacted***".to_string());
                } else {
                    redact_sensitive_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_sensitive_json(value);
            }
        }
        _ => {}
    }
}

fn redact_sensitive_query_parameters(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    for (index, segment) in value.split('&').enumerate() {
        if index > 0 {
            redacted.push('&');
        }
        let Some(equal) = segment.find('=') else {
            redacted.push_str(segment);
            continue;
        };
        let raw_key = segment[..equal]
            .rsplit(['?', ' ', '\t', '"', '\'', ':'])
            .next()
            .unwrap_or(&segment[..equal]);
        if is_sensitive_alert_display_key(raw_key) {
            redacted.push_str(&segment[..=equal]);
            redacted.push_str("***redacted***");
        } else {
            redacted.push_str(segment);
        }
    }
    redacted
}

fn redact_sensitive_alert_display_text(value: &str) -> String {
    let normalized = serde_json::from_str::<serde_json::Value>(value)
        .map(|mut json| {
            redact_sensitive_json(&mut json);
            serde_json::to_string(&json).unwrap_or_else(|_| value.to_string())
        })
        .unwrap_or_else(|_| value.to_string());
    redact_sensitive_query_parameters(&normalized)
}

fn bounded_alert_event_display_text(value: Option<String>) -> Option<String> {
    value.map(|value| {
        let value = redact_sensitive_alert_display_text(&value);
        crate::analysis::truncate_text(&value, ALERT_EVENT_DISPLAY_TEXT_MAX_CHARS.saturating_sub(1))
    })
}

pub(crate) fn normalize_alert_event_projection_display_text(row: &mut AlertEventProjectionRow) {
    for value in [
        &mut row.method,
        &mut row.path,
        &mut row.query,
        &mut row.request_kind_key,
        &mut row.request_kind_label,
        &mut row.request_kind_detail,
        &mut row.result_status,
        &mut row.failure_kind,
        &mut row.error_message,
        &mut row.user_display_name,
        &mut row.user_username,
        &mut row.reason_code,
        &mut row.reason_summary,
        &mut row.reason_detail,
        &mut row.job_type,
        &mut row.job_trigger_source,
        &mut row.job_status,
        &mut row.job_message,
    ] {
        *value = bounded_alert_event_display_text(value.take());
    }
}

fn bounded_alert_event_identifier(value: Option<String>) -> Option<String> {
    value.map(|value| bounded_alert_event_identifier_value(&value))
}

pub(crate) fn bounded_alert_event_identifier_value(value: &str) -> String {
    // `truncate_text` appends an ellipsis after the requested number of
    // characters. Reserve one character so the serialized identifier stays
    // within the advertised bound even when truncation occurs.
    crate::analysis::truncate_text(value, ALERT_EVENT_IDENTIFIER_MAX_CHARS.saturating_sub(1))
}

pub(crate) fn retain_alert_group_child_events(
    events: Vec<AlertEventRecord>,
) -> Vec<AlertEventRecord> {
    let mut bytes = 0usize;
    let mut bounded_events = Vec::with_capacity(events.len());
    for event in events {
        let Ok((event, event_json)) = serialize_alert_event_record_for_projection(event) else {
            return Vec::new();
        };
        bytes = bytes.saturating_add(event_json.len());
        if bytes > ALERT_EVENT_PROJECTION_MAX_BYTES {
            // Group summaries and the latest event remain available. The
            // detail drawer can fetch the same child history through the
            // existing paginated Events endpoint when inline history is large.
            return Vec::new();
        }
        bounded_events.push(event);
    }
    bounded_events
}

pub(crate) fn serialize_alert_event_projection_payload(
    mut row: AlertEventProjectionRow,
) -> Result<String, ProxyError> {
    normalize_alert_event_projection_display_text(&mut row);
    let payload = serde_json::to_string(&row).map_err(|error| {
        ProxyError::Other(format!("serialize alert projection payload: {error}"))
    })?;
    if payload.len() <= ALERT_EVENT_PROJECTION_MAX_BYTES {
        return Ok(payload);
    }

    row.source_kind = bounded_alert_event_identifier_value(&row.source_kind);
    row.source_id = bounded_alert_event_identifier_value(&row.source_id);
    row.row_sort_id = bounded_alert_event_identifier_value(&row.row_sort_id);
    row.alert_type = bounded_alert_event_identifier_value(&row.alert_type);
    row.token_id = bounded_alert_event_identifier(row.token_id.take());
    row.key_id = bounded_alert_event_identifier(row.key_id.take());
    row.user_id = bounded_alert_event_identifier(row.user_id.take());
    let compact_identities = serde_json::to_string(&row).map_err(|error| {
        ProxyError::Other(format!(
            "serialize compact alert projection identities: {error}"
        ))
    })?;
    if compact_identities.len() <= ALERT_EVENT_PROJECTION_MAX_BYTES {
        return Ok(compact_identities);
    }

    // The projection is a bounded display copy, not a second raw-log archive.
    // If legacy data still exceeds the fragment budget, remove optional detail
    // before persisting it again. Counts, type, time and filter identities stay
    // available; the raw source remains the authority for full diagnostics.
    row.query = None;
    row.request_kind_detail = None;
    row.reason_detail = None;
    row.job_message = None;
    row.error_message = None;
    row.reason_summary = None;
    row.request_kind_label = None;
    row.user_display_name = None;
    row.user_username = None;
    row.method = None;
    row.path = None;
    let compact = serde_json::to_string(&row).map_err(|error| {
        ProxyError::Other(format!(
            "serialize compact alert projection payload: {error}"
        ))
    })?;
    if compact.len() > ALERT_EVENT_PROJECTION_MAX_BYTES {
        return Err(ProxyError::Other(
            "alert projection payload exceeds its display budget".to_string(),
        ));
    }
    Ok(compact)
}

pub(crate) fn bound_alert_event_record_for_projection(
    mut event: AlertEventRecord,
) -> AlertEventRecord {
    // AlertEventRecord is assembled after projection decoding, so a legacy
    // source row can still carry oversized identifiers or nested labels. Keep
    // the summary/count/latest-event contract, but make every persisted
    // derived copy bounded before it reaches fragments or reduction tables.
    for value in [
        &mut event.title,
        &mut event.summary,
        &mut event.subject_label,
    ] {
        *value = bounded_alert_event_display_text(Some(std::mem::take(value))).unwrap_or_default();
    }
    for value in [
        &mut event.failure_kind,
        &mut event.result_status,
        &mut event.error_message,
        &mut event.reason_code,
        &mut event.reason_summary,
        &mut event.reason_detail,
    ] {
        *value = bounded_alert_event_display_text(value.take());
    }
    if let Some(user) = event.user.as_mut() {
        user.display_name = bounded_alert_event_display_text(user.display_name.take());
        user.username = bounded_alert_event_display_text(user.username.take());
    }
    for entity in [&mut event.token, &mut event.key] {
        if let Some(entity) = entity.as_mut() {
            entity.label =
                bounded_alert_event_display_text(Some(std::mem::take(&mut entity.label)))
                    .unwrap_or_default();
        }
    }
    if let Some(job) = event.job.as_mut() {
        job.job_type = bounded_alert_event_display_text(Some(std::mem::take(&mut job.job_type)))
            .unwrap_or_default();
        job.trigger_source =
            bounded_alert_event_display_text(Some(std::mem::take(&mut job.trigger_source)))
                .unwrap_or_default();
        job.status = bounded_alert_event_display_text(Some(std::mem::take(&mut job.status)))
            .unwrap_or_default();
        job.message = bounded_alert_event_display_text(job.message.take());
    }
    if let Some(request) = event.request.as_mut() {
        request.method =
            bounded_alert_event_display_text(Some(std::mem::take(&mut request.method)))
                .unwrap_or_default();
        request.path = bounded_alert_event_display_text(Some(std::mem::take(&mut request.path)))
            .unwrap_or_default();
        request.query = bounded_alert_event_display_text(request.query.take());
    }
    if let Some(request_kind) = event.request_kind.as_mut() {
        request_kind.label =
            bounded_alert_event_display_text(Some(std::mem::take(&mut request_kind.label)))
                .unwrap_or_default();
        request_kind.detail = bounded_alert_event_display_text(request_kind.detail.take());
    }
    if let Some(semantic) = event.semantic_window.as_mut() {
        semantic.window_key = semantic
            .window_key
            .take()
            .map(|value| bounded_alert_event_identifier_value(&value));
    }

    if serde_json::to_vec(&event)
        .map(|payload| payload.len() > ALERT_EVENT_PROJECTION_MAX_BYTES)
        .unwrap_or(true)
    {
        event.id = bounded_alert_event_identifier_value(&event.id);
        event.alert_type = bounded_alert_event_identifier_value(&event.alert_type);
        event.subject_kind = bounded_alert_event_identifier_value(&event.subject_kind);
        event.subject_id = bounded_alert_event_identifier_value(&event.subject_id);
        event.source.kind = bounded_alert_event_identifier_value(&event.source.kind);
        event.source.id = bounded_alert_event_identifier_value(&event.source.id);
        if let Some(user) = event.user.as_mut() {
            user.user_id = bounded_alert_event_identifier_value(&user.user_id);
        }
        for entity in [&mut event.token, &mut event.key] {
            if let Some(entity) = entity.as_mut() {
                entity.id = bounded_alert_event_identifier_value(&entity.id);
            }
        }
        if let Some(request_kind) = event.request_kind.as_mut() {
            request_kind.key = bounded_alert_event_identifier_value(&request_kind.key);
        }
    }

    if serde_json::to_vec(&event)
        .map(|payload| payload.len() > ALERT_EVENT_PROJECTION_MAX_BYTES)
        .unwrap_or(true)
    {
        // Optional diagnostic detail is never allowed to turn a derived
        // record into an unbounded blob. The raw source log remains the
        // authoritative place for those details.
        event.request = None;
        event.request_kind = None;
        if let Some(job) = event.job.as_mut() {
            job.message = None;
        }
        if let Some(user) = event.user.as_mut() {
            user.display_name = None;
            user.username = None;
        }
        if let Some(entity) = event.token.as_mut() {
            entity.label.clear();
        }
        if let Some(entity) = event.key.as_mut() {
            entity.label.clear();
        }
        event.error_message = None;
        event.reason_detail = None;
    }
    if serde_json::to_vec(&event)
        .map(|payload| payload.len() > ALERT_EVENT_PROJECTION_MAX_BYTES)
        .unwrap_or(true)
    {
        // This is an unreachable-sized legacy fallback (for example, a
        // database containing unbounded identity columns). Keep only the
        // stable summary identity and timestamps rather than allowing an
        // oversized derived row to be written.
        event.failure_kind = None;
        event.result_status = None;
        event.reason_code = None;
        event.reason_summary = None;
        event.title = crate::analysis::truncate_text(&event.title, 256);
        event.summary = crate::analysis::truncate_text(&event.summary, 512);
        event.subject_label = crate::analysis::truncate_text(&event.subject_label, 256);
        event.user = None;
        event.token = None;
        event.key = None;
        event.job = None;
    }
    debug_assert!(
        serde_json::to_vec(&event)
            .map(|payload| payload.len() <= ALERT_EVENT_PROJECTION_MAX_BYTES)
            .unwrap_or(false),
        "derived alert event must stay within its persistence budget"
    );
    event
}

pub(crate) fn serialize_alert_event_record_for_projection(
    event: AlertEventRecord,
) -> Result<(AlertEventRecord, String), ProxyError> {
    let event = bound_alert_event_record_for_projection(event);
    let payload = serde_json::to_string(&event)
        .map_err(|error| ProxyError::Other(format!("serialize bounded alert event: {error}")))?;
    if payload.len() > ALERT_EVENT_PROJECTION_MAX_BYTES {
        return Err(ProxyError::Other(
            "derived alert event exceeds its persistence budget".to_string(),
        ));
    }
    Ok((event, payload))
}

#[cfg(test)]
mod tests {
    use super::redact_sensitive_alert_display_text;

    #[test]
    fn alert_projection_redacts_sensitive_query_parameters() {
        let redacted = redact_sensitive_alert_display_text(
            "https://example.test/mcp?api_key=secret&password=hunter2&q=public",
        );
        assert!(redacted.contains("api_key=***redacted***"));
        assert!(redacted.contains("password=***redacted***"));
        assert!(redacted.contains("q=public"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("hunter2"));
    }

    #[test]
    fn alert_projection_redacts_sensitive_json_fields() {
        let redacted = redact_sensitive_alert_display_text(
            r#"{"error":"failed","access_token":"secret","nested":{"private_key":"key"}}"#,
        );
        assert!(redacted.contains("\"access_token\":\"***redacted***\""));
        assert!(redacted.contains("\"private_key\":\"***redacted***\""));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("\"key\""));
    }
}
