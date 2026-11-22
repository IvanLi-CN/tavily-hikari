use std::collections::HashSet as MarkerHashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex as MarkerMutex, OnceLock as MarkerOnceLock};

static ABANDONED_UPSTREAM_USAGE_ATTEMPTS: MarkerOnceLock<MarkerMutex<MarkerHashSet<(String, String)>>> =
    MarkerOnceLock::new();

fn abandoned_upstream_usage_marker_path(database_path: &str) -> PathBuf {
    PathBuf::from(format!("{database_path}.abandoned-upstream-usage"))
}

fn load_persisted_abandoned_upstream_usage_attempts(
    database_path: &str,
    abandoned: &mut MarkerHashSet<(String, String)>,
) {
    let path = abandoned_upstream_usage_marker_path(database_path);
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    for reservation_id in contents.lines().map(str::trim).filter(|id| !id.is_empty()) {
        abandoned.insert((database_path.to_string(), reservation_id.to_string()));
    }
}

fn persist_abandoned_upstream_usage_attempts(
    database_path: &str,
    abandoned: &MarkerHashSet<(String, String)>,
) -> std::io::Result<()> {
    let path = abandoned_upstream_usage_marker_path(database_path);
    let ids = abandoned
        .iter()
        .filter(|(path, _)| path == database_path)
        .map(|(_, reservation_id)| reservation_id.as_str())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        let path = abandoned_upstream_usage_marker_path(database_path);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        for reservation_id in ids {
            writeln!(temporary, "{reservation_id}")?;
        }
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)
    }
}

pub(crate) fn remember_abandoned_upstream_usage_attempt(
    database_path: impl Into<String>,
    reservation_id: String,
) {
    let database_path = database_path.into();
    ABANDONED_UPSTREAM_USAGE_ATTEMPTS
        .get_or_init(|| MarkerMutex::new(MarkerHashSet::new()))
        .lock()
        .map(|mut abandoned| {
            load_persisted_abandoned_upstream_usage_attempts(&database_path, &mut abandoned);
            if abandoned.insert((database_path.clone(), reservation_id))
                && let Err(error) =
                    persist_abandoned_upstream_usage_attempts(&database_path, &abandoned)
            {
                tracing::warn!(
                    component = "reconciliation",
                    event = "rate_attempt_reservation_marker_persist_failed",
                    error_kind = "marker_io",
                    error = %error,
                );
            }
        })
        .expect("abandoned upstream usage attempt lock is not poisoned");
}

pub(crate) fn peek_abandoned_upstream_usage_attempts(database_path: &str) -> Vec<String> {
    let mut abandoned = ABANDONED_UPSTREAM_USAGE_ATTEMPTS
        .get_or_init(|| MarkerMutex::new(MarkerHashSet::new()))
        .lock()
        .expect("abandoned upstream usage attempt lock is not poisoned");
    load_persisted_abandoned_upstream_usage_attempts(database_path, &mut abandoned);
    abandoned
        .iter()
        .filter(|(path, _)| path == database_path)
        .map(|(_, reservation_id)| reservation_id.clone())
        .collect()
}

pub(crate) fn forget_abandoned_upstream_usage_attempts(
    database_path: &str,
    reservation_ids: &[String],
) {
    if reservation_ids.is_empty() {
        return;
    }
    let mut abandoned = ABANDONED_UPSTREAM_USAGE_ATTEMPTS
        .get_or_init(|| MarkerMutex::new(MarkerHashSet::new()))
        .lock()
        .expect("abandoned upstream usage attempt lock is not poisoned");
    load_persisted_abandoned_upstream_usage_attempts(database_path, &mut abandoned);
    let ids = reservation_ids
        .iter()
        .map(String::as_str)
        .collect::<MarkerHashSet<_>>();
    abandoned.retain(|(path, reservation_id)| {
        path != database_path || !ids.contains(reservation_id.as_str())
    });
    if let Err(error) = persist_abandoned_upstream_usage_attempts(database_path, &abandoned) {
        tracing::warn!(
            component = "reconciliation",
            event = "rate_attempt_reservation_marker_clear_failed",
            error_kind = "marker_io",
            error = %error,
        );
    }
}

#[cfg(test)]
pub(crate) fn take_abandoned_upstream_usage_attempts(database_path: &str) -> Vec<String> {
    let ids = peek_abandoned_upstream_usage_attempts(database_path);
    forget_abandoned_upstream_usage_attempts(database_path, &ids);
    ids
}

#[cfg(test)]
fn clear_abandoned_upstream_usage_attempt_memory_for_test(database_path: &str) {
    ABANDONED_UPSTREAM_USAGE_ATTEMPTS
        .get_or_init(|| MarkerMutex::new(MarkerHashSet::new()))
        .lock()
        .expect("abandoned upstream usage attempt lock is not poisoned")
        .retain(|(path, _)| path != database_path);
}
