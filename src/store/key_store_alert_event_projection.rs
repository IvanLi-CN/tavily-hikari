use super::AlertEventProjectionRow;
use crate::{AlertEventRecord, AlertGroupRecord, ProxyError, RecentAlertsSummary};

// Alerts are a display projection, not a second raw-error archive. The raw
// request and job records remain authoritative; every copied diagnostic field
// is bounded before it can be repeated in Events and Groups payloads.
pub(crate) const ALERT_EVENT_DISPLAY_TEXT_MAX_CHARS: usize = 1024;
pub(crate) const ALERT_EVENT_PROJECTION_MAX_BYTES: usize = 64 * 1024;
pub(crate) const ALERT_EVENT_IDENTIFIER_MAX_CHARS: usize = 256;

fn is_sensitive_alert_display_key(key: &str) -> bool {
    let key = key
        .trim()
        .trim_matches(|character| matches!(character, '?' | '&' | '"' | '\'' | ':'));
    let decoded = urlencoding::decode(key).unwrap_or_else(|_| key.into());
    // Fallback diagnostics can contain a malformed JSON fragment whose key
    // still uses JSON unicode escapes (for example, `\u0061piKey`). Decode
    // those escapes before normalizing the label so malformed input cannot
    // bypass the sensitive-key filter.
    let decoded = serde_json::from_str::<String>(&format!("\"{decoded}\""))
        .unwrap_or_else(|_| decoded.into_owned());
    let key: String = decoded
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let key = key.as_str();
    key.contains("apikey")
        || key.contains("accesstoken")
        || key.contains("refreshtoken")
        || key == "token"
        || key.contains("token")
        || key.contains("password")
        || key.contains("secret")
        || key.contains("authorization")
        || key.contains("credential")
        || key.contains("privatekey")
}

fn redact_sensitive_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            // Object keys are display data too. Rebuild the map so opaque
            // credentials embedded in a key cannot bypass the structured
            // value redaction path.
            let original = std::mem::take(fields);
            let mut redacted_fields = serde_json::Map::with_capacity(original.len());
            for (key, mut value) in original {
                let mut redacted_key = redact_opaque_sensitive_tokens(&key);
                if redacted_fields.contains_key(&redacted_key) {
                    // Two credential-like keys can redact to the same marker.
                    // Keep both display fields without exposing their source.
                    let base_key = redacted_key.clone();
                    let mut suffix = 2usize;
                    loop {
                        let candidate = format!("{base_key}_{suffix}");
                        if !redacted_fields.contains_key(&candidate) {
                            redacted_key = candidate;
                            break;
                        }
                        suffix = suffix.saturating_add(1);
                    }
                }
                if is_sensitive_alert_display_key(&key) {
                    value = serde_json::Value::String("***redacted***".to_string());
                } else {
                    redact_sensitive_json(&mut value);
                }
                redacted_fields.insert(redacted_key, value);
            }
            *fields = redacted_fields;
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_sensitive_json(value);
            }
        }
        serde_json::Value::String(text) => {
            if let Some(redacted) = redact_embedded_json_text(text) {
                *text = redacted;
            } else {
                *text = redact_sensitive_labeled_values(text);
            }
        }
        _ => {}
    }
}

fn quoted_alert_suffix_is_structural(value: &str) -> bool {
    let mut remainder = value.trim_start();
    while !remainder.is_empty() {
        let Some((_, separator)) = remainder.char_indices().next() else {
            return true;
        };
        if !matches!(
            separator,
            ',' | ';' | '|' | '&' | '\n' | '\r' | '}' | ']' | ')' | ':' | '='
        ) {
            return false;
        }
        let after_separator = &remainder[separator.len_utf8()..];
        remainder = after_separator.trim_start();
        if matches!(separator, '}' | ']' | ')') {
            if remainder.is_empty() {
                return true;
            }
            return false;
        }
        if remainder.is_empty() {
            return true;
        }

        let segment_end = remainder
            .char_indices()
            .find(|(_, character)| {
                matches!(
                    character,
                    ',' | ';' | '|' | '&' | '\n' | '\r' | '}' | ']' | ')'
                )
            })
            .map(|(offset, _)| offset)
            .unwrap_or(remainder.len());
        let segment = remainder[..segment_end].trim();
        let Some((delimiter_offset, delimiter)) = segment
            .char_indices()
            .find(|(_, character)| matches!(character, ':' | '='))
        else {
            return false;
        };
        let raw_key = segment[..delimiter_offset].trim();
        let raw_value = segment[delimiter_offset + delimiter.len_utf8()..].trim();
        if raw_key.is_empty()
            || (is_sensitive_alert_display_key(raw_key)
                && !matches!(
                    raw_value,
                    "***redacted***" | "\"***redacted***\"" | "'***redacted***'"
                ))
        {
            return false;
        }
        debug_assert!(matches!(delimiter, ':' | '='));
        remainder = remainder[segment_end..].trim_start();
    }
    true
}

fn redact_sensitive_labeled_values(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    let mut scan_offset = 0;
    let mut changed = false;

    while scan_offset < value.len() {
        let Some((relative_index, character)) = value[scan_offset..]
            .char_indices()
            .find(|(_, character)| matches!(character, ':' | '='))
        else {
            break;
        };
        let delimiter = scan_offset + relative_index;
        let bytes = value.as_bytes();
        let mut key_start = delimiter;
        while key_start > 0
            && !matches!(
                bytes[key_start - 1],
                b'{' | b'}'
                    | b'['
                    | b']'
                    | b'('
                    | b')'
                    | b','
                    | b';'
                    | b'|'
                    | b'&'
                    | b'?'
                    | b'\n'
                    | b'\r'
                    | b':'
                    | b'='
            )
        {
            key_start -= 1;
        }
        let raw_key = value[key_start..delimiter].trim();
        if !is_sensitive_alert_display_key(raw_key) {
            scan_offset = delimiter + character.len_utf8();
            continue;
        }

        let mut value_start = delimiter + character.len_utf8();
        while value_start < value.len() && value.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        let quoted = value
            .as_bytes()
            .get(value_start)
            .copied()
            .filter(|character| matches!(character, b'"' | b'\''));
        if let Some(quote) = quoted {
            let content_start = value_start + 1;
            let mut escaped = false;
            let closing_quote =
                value[content_start..]
                    .char_indices()
                    .find_map(|(offset, character)| {
                        if escaped {
                            escaped = false;
                            return None;
                        }
                        if character == '\\' {
                            escaped = true;
                            return None;
                        }
                        (character == quote as char).then_some(content_start + offset)
                    });
            output.push_str(&value[cursor..value_start + 1]);
            output.push_str("***redacted***");
            if let Some(closing_quote) = closing_quote {
                let after_quote = &value[closing_quote + 1..];
                if quoted_alert_suffix_is_structural(after_quote) {
                    output.push(quote as char);
                    cursor = closing_quote + 1;
                } else {
                    // A non-delimited suffix means the quote was not a
                    // trustworthy structural boundary. Keep the display
                    // projection fail-closed instead of copying an opaque
                    // credential-like suffix into the output.
                    cursor = value.len();
                }
            } else {
                cursor = value.len();
            }
            scan_offset = cursor;
        } else {
            let value_end = value[value_start..]
                .char_indices()
                .find(|(_, character)| {
                    matches!(character, ',' | ';' | '|' | '&' | '\n' | '\r' | '}' | ']')
                })
                .map(|(offset, _)| value_start + offset)
                .unwrap_or(value.len());

            output.push_str(&value[cursor..value_start]);
            output.push_str("***redacted***");
            cursor = value_end;
            scan_offset = value_end;
        }
        changed = true;
    }

    let redacted = if !changed {
        value.to_string()
    } else {
        output.push_str(&value[cursor..]);
        output
    };
    redact_opaque_sensitive_tokens(&redacted)
}

fn redact_opaque_sensitive_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    let mut scan_offset = 0;
    while let Some(relative_index) = [
        value[scan_offset..].find("sk_"),
        value[scan_offset..].find("tvly-"),
    ]
    .into_iter()
    .flatten()
    .min()
    {
        let start = scan_offset + relative_index;
        let is_boundary = start == 0
            || !value[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
        if !is_boundary {
            scan_offset = start + 3;
            continue;
        }
        let end = value[start..]
            .char_indices()
            .find(|(_, character)| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            })
            .map(|(offset, _)| start + offset)
            .unwrap_or(value.len());
        output.push_str(&value[cursor..start]);
        output.push_str("***redacted***");
        cursor = end;
        scan_offset = end;
    }
    if cursor == 0 {
        value.to_string()
    } else {
        output.push_str(&value[cursor..]);
        output
    }
}

fn redact_embedded_json_text(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    let mut scan_offset = 0;
    let mut changed = false;

    while scan_offset < value.len() {
        let Some((relative_index, character)) = value[scan_offset..]
            .char_indices()
            .find(|(_, character)| matches!(character, '{' | '['))
        else {
            break;
        };
        let index = scan_offset + relative_index;
        let suffix = &value[index..];
        let mut stream =
            serde_json::Deserializer::from_str(suffix).into_iter::<serde_json::Value>();
        let Some(Ok(mut json)) = stream.next() else {
            scan_offset = index + character.len_utf8();
            continue;
        };
        let consumed = stream.byte_offset();
        let Ok(serialized) = serde_json::to_string(&json) else {
            scan_offset = index + character.len_utf8();
            continue;
        };
        redact_sensitive_json(&mut json);
        let Ok(redacted) = serde_json::to_string(&json) else {
            scan_offset = index + character.len_utf8();
            continue;
        };
        output.push_str(&redact_sensitive_labeled_values(&value[cursor..index]));
        if serialized == redacted {
            output.push_str(&suffix[..consumed]);
        } else {
            output.push_str(&redacted);
        }
        cursor = index + consumed;
        scan_offset = cursor;
        changed |= serialized != redacted;
    }

    if !changed {
        return None;
    }
    output.push_str(&redact_sensitive_labeled_values(&value[cursor..]));
    Some(output)
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
    let (normalized, structured) =
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(value) {
            redact_sensitive_json(&mut json);
            (
                serde_json::to_string(&json).unwrap_or_else(|_| value.to_string()),
                true,
            )
        } else if let Some(redacted) = redact_embedded_json_text(value) {
            (redacted, true)
        } else {
            (value.to_string(), false)
        };
    let normalized = redact_sensitive_query_parameters(&normalized);
    if structured {
        normalized
    } else {
        redact_sensitive_labeled_values(&normalized)
    }
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

pub(crate) fn normalize_alert_group_record_for_projection(
    mut group: AlertGroupRecord,
) -> AlertGroupRecord {
    group.subject_label =
        bounded_alert_event_display_text(Some(group.subject_label)).unwrap_or_default();
    for entity in [&mut group.token, &mut group.key] {
        if let Some(entity) = entity.as_mut() {
            entity.label =
                bounded_alert_event_display_text(Some(std::mem::take(&mut entity.label)))
                    .unwrap_or_default();
        }
    }
    if let Some(user) = group.user.as_mut() {
        user.display_name = bounded_alert_event_display_text(user.display_name.take());
        user.username = bounded_alert_event_display_text(user.username.take());
    }
    if let Some(job) = group.job.as_mut() {
        job.job_type = bounded_alert_event_display_text(Some(std::mem::take(&mut job.job_type)))
            .unwrap_or_default();
        job.trigger_source =
            bounded_alert_event_display_text(Some(std::mem::take(&mut job.trigger_source)))
                .unwrap_or_default();
        job.status = bounded_alert_event_display_text(Some(std::mem::take(&mut job.status)))
            .unwrap_or_default();
        job.message = bounded_alert_event_display_text(job.message.take());
    }
    if let Some(request_kind) = group.request_kind.as_mut() {
        request_kind.key = bounded_alert_event_identifier_value(&request_kind.key);
        request_kind.label =
            bounded_alert_event_display_text(Some(std::mem::take(&mut request_kind.label)))
                .unwrap_or_default();
        request_kind.detail = bounded_alert_event_display_text(request_kind.detail.take());
    }
    group.latest_event = bound_alert_event_record_for_projection(group.latest_event);
    group.children = group
        .children
        .into_iter()
        .map(normalize_alert_group_record_for_projection)
        .collect();
    group.child_events = group
        .child_events
        .into_iter()
        .map(bound_alert_event_record_for_projection)
        .collect();
    group
}

pub(crate) fn normalize_recent_alerts_summary_for_projection(
    mut summary: RecentAlertsSummary,
) -> RecentAlertsSummary {
    summary.top_groups = summary
        .top_groups
        .into_iter()
        .map(normalize_alert_group_record_for_projection)
        .collect();
    summary
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
    use super::{
        normalize_recent_alerts_summary_for_projection, redact_sensitive_alert_display_text,
    };
    use crate::{
        AlertEventRecord, AlertGroupRecord, AlertJobRef, AlertSourceRef, RecentAlertsSummary,
        TokenRequestKind,
    };

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
    fn alert_projection_redacts_common_sensitive_key_spellings() {
        let redacted = redact_sensitive_alert_display_text(
            "https://example.test/mcp?access-token=one&%61pi%5Fkey=two&safe=value",
        );
        assert!(redacted.contains("access-token=***redacted***"));
        assert!(redacted.contains("%61pi%5Fkey=***redacted***"));
        assert!(redacted.contains("safe=value"));
        assert!(!redacted.contains("one"));
        assert!(!redacted.contains("two"));
    }

    #[test]
    fn alert_projection_redacts_sensitive_json_fields() {
        let redacted = redact_sensitive_alert_display_text(
            r#"{"error":"failed","accessToken":"secret","private-key":"key","nested":{"refreshToken":"refresh"}}"#,
        );
        assert!(redacted.contains("\"accessToken\":\"***redacted***\""));
        assert!(redacted.contains("\"private-key\":\"***redacted***\""));
        assert!(redacted.contains("\"refreshToken\":\"***redacted***\""));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("\"key\""));
        assert!(!redacted.contains("\"refreshToken\":\"refresh\""));
    }

    #[test]
    fn alert_projection_redacts_prefixed_json_error_payloads() {
        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"accessToken":"secret","nested":{"apiKey":"key"}}"#,
        );
        assert!(redacted.contains("usage_http 429"));
        assert!(redacted.contains("\"accessToken\":\"***redacted***\""));
        assert!(redacted.contains("\"apiKey\":\"***redacted***\""));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("key"));
    }

    #[test]
    fn alert_projection_redacts_json_nested_inside_string_fields() {
        let redacted = redact_sensitive_alert_display_text(
            r#"{"message":"{\"accessToken\":\"secret\",\"nested\":{\"apiKey\":\"key\"}}"}"#,
        );
        assert!(redacted.contains("***redacted***"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("key"));
    }

    #[test]
    fn alert_projection_redacts_all_embedded_json_error_payloads() {
        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"message":"first","accessToken":"first-secret"} then [{"apiKey":"second-secret"}]"#,
        );
        assert!(redacted.contains("usage_http 429"));
        assert!(redacted.contains("then"));
        assert!(!redacted.contains("first-secret"));
        assert!(!redacted.contains("second-secret"));
        assert_eq!(redacted.matches("***redacted***").count(), 2);
    }

    #[test]
    fn alert_projection_redacts_colon_delimited_sensitive_values() {
        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: authorization: Bearer secret-value; safe: visible"#,
        );
        assert!(redacted.contains("authorization: ***redacted***"));
        assert!(redacted.contains("safe: visible"));
        assert!(!redacted.contains("secret-value"));
    }

    #[test]
    fn alert_projection_redacts_quoted_colon_delimited_sensitive_values() {
        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: authorization: "secret-value"; safe: visible"#,
        );
        assert!(redacted.contains("authorization: \"***redacted***\""));
        assert!(redacted.contains("safe: visible"));
        assert!(!redacted.contains("secret-value"));
    }

    #[test]
    fn alert_projection_redacts_unicode_sensitive_values() {
        let redacted = redact_sensitive_alert_display_text(
            "usage_http 429: authorization: \"Ģsecret-value\"; safe: visible",
        );
        assert!(redacted.contains("authorization: \"***redacted***\""));
        assert!(redacted.contains("safe: visible"));
        assert!(!redacted.contains("Ģsecret-value"));
    }

    #[test]
    fn alert_projection_redacts_quoted_sensitive_labels() {
        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: "authorization": "secret-value"; safe: visible"#,
        );
        assert!(redacted.contains("\"authorization\": \"***redacted***\""));
        assert!(redacted.contains("safe: visible"));
        assert!(!redacted.contains("secret-value"));
    }

    #[test]
    fn alert_projection_redacts_colon_delimited_values_inside_json_strings() {
        let redacted = redact_sensitive_alert_display_text(
            r#"{"error":"authorization: Bearer secret-value"}"#,
        );
        assert!(redacted.contains("***redacted***"));
        assert!(!redacted.contains("secret-value"));
    }

    #[test]
    fn alert_projection_redacts_unicode_escaped_sensitive_labels() {
        let redacted =
            redact_sensitive_alert_display_text(r#"usage_http 429: {"\u0061piKey": "secret""#);
        assert!(!redacted.contains("secret"));
        assert!(redacted.contains("***redacted***"));
    }

    #[test]
    fn alert_projection_redacts_opaque_credentials_in_json_object_keys() {
        let redacted = redact_sensitive_alert_display_text(
            r#"{"tvly-dev-opaque-value":"visible","sk_opaque_value":"visible"}"#,
        );
        assert!(!redacted.contains("tvly-dev-opaque-value"));
        assert!(!redacted.contains("sk_opaque_value"));
        assert_eq!(redacted.matches("***redacted***").count(), 2);

        let redacted = redact_sensitive_alert_display_text(
            r#"{"\u0074vly-dev-unicode-value":"visible","\u0073k_unicode_value":"visible"}"#,
        );
        assert!(!redacted.contains("tvly-dev-unicode-value"));
        assert!(!redacted.contains("sk_unicode_value"));
        assert_eq!(redacted.matches("***redacted***").count(), 2);
    }

    #[test]
    fn alert_projection_redacts_malformed_quoted_sensitive_value_suffix() {
        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"authorization":"prefix"sk_live_secret"}"#,
        );
        assert!(!redacted.contains("sk_live_secret"));
        assert!(redacted.contains("***redacted***"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: authorization: "prefix" sk_live_secret"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: authorization: "prefix" sk_live_secret: ignored"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: authorization: "prefix";sk_live_secret"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: authorization: "prefix";sk_live_secret: ignored"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"authorization":"prefix"}sk_live_secret"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"authorization":"prefix"} sk_live_secret"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"authorization":"prefix"} tvly-dev-secret"#,
        );
        assert!(!redacted.contains("tvly-dev-secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"authorization":"prefix"} note sk_live_secret"#,
        );
        assert!(!redacted.contains("sk_live_secret"));

        let redacted = redact_sensitive_alert_display_text(
            r#"usage_http 429: {"authorization":"prefix"} note tvly-dev-secret"#,
        );
        assert!(!redacted.contains("tvly-dev-secret"));

        let redacted =
            redact_sensitive_alert_display_text("usage_http 429: note tvly-dev-opaque-value");
        assert!(!redacted.contains("tvly-dev-opaque-value"));

        let redacted = redact_sensitive_alert_display_text("usage_http 429: note sk_opaque_value");
        assert!(!redacted.contains("sk_opaque_value"));
    }

    #[test]
    fn alert_projection_redacts_legacy_materialized_summary_job_messages() {
        let event = AlertEventRecord {
            id: "event-1".to_string(),
            alert_type: "job_failed".to_string(),
            title: "Job failed".to_string(),
            summary: "summary".to_string(),
            occurred_at: 1,
            subject_kind: "job".to_string(),
            subject_id: "job-1".to_string(),
            subject_label: "job".to_string(),
            user: None,
            token: None,
            key: None,
            job: Some(AlertJobRef {
                id: 1,
                job_type: "maintenance".to_string(),
                trigger_source: "scheduler".to_string(),
                status: "failed".to_string(),
                attempt: 1,
                message: Some("authorization: sk_live_legacy".to_string()),
                queued_at: 1,
                started_at: None,
                finished_at: None,
            }),
            request: None,
            request_kind: None,
            failure_kind: None,
            result_status: None,
            error_message: None,
            reason_code: None,
            reason_summary: None,
            reason_detail: None,
            source: AlertSourceRef {
                kind: "scheduled_job".to_string(),
                id: "job-1".to_string(),
            },
            semantic_window: None,
        };
        let group = AlertGroupRecord {
            id: "group-1".to_string(),
            alert_type: "job_failed".to_string(),
            subject_kind: "job".to_string(),
            subject_id: "job-1".to_string(),
            subject_label: "job".to_string(),
            user: None,
            token: None,
            key: None,
            job: None,
            request_kind: Some(TokenRequestKind {
                key: "research".to_string(),
                label: "Research".to_string(),
                detail: Some("authorization: legacy-secret".to_string()),
            }),
            count: 1,
            first_seen: 1,
            last_seen: 1,
            latest_event: event.clone(),
            grouping_kind: "job".to_string(),
            semantic_window_kind: None,
            semantic_window_minutes: None,
            semantic_window_start: None,
            semantic_window_end: None,
            semantic_window_key: None,
            child_count: 1,
            event_count: 1,
            children: Vec::new(),
            child_events: vec![event],
        };
        let normalized = normalize_recent_alerts_summary_for_projection(RecentAlertsSummary {
            top_groups: vec![group],
            ..Default::default()
        });
        let normalized_group = &normalized.top_groups[0];
        assert_eq!(
            normalized_group
                .latest_event
                .job
                .as_ref()
                .unwrap()
                .message
                .as_deref(),
            Some("authorization: ***redacted***")
        );
        assert_eq!(
            normalized_group.child_events[0]
                .job
                .as_ref()
                .unwrap()
                .message
                .as_deref(),
            Some("authorization: ***redacted***")
        );
        assert_eq!(
            normalized_group
                .request_kind
                .as_ref()
                .unwrap()
                .detail
                .as_deref(),
            Some("authorization: ***redacted***")
        );
    }
}
