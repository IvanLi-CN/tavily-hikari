//! Snapshot manifest. The snapshot identifier is the sha256 of the snapshot file; the
//! manifest records what the copy contains so an operator can identify the artifact without
//! opening it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const MANIFEST_SUFFIX: &str = ".manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub path: String,
    pub bytes: u64,
    pub created_at: String,
    pub tool_version: String,
    pub source: SnapshotSource,
    pub tables: Vec<ManifestTable>,
    pub total_rows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSource {
    pub sqlite_path: String,
    pub observability_path: Option<String>,
    pub user_version: i64,
    pub observability_user_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestTable {
    pub name: String,
    pub rows: u64,
    /// `core` or `observability`, i.e. the SQLite file the table was copied from.
    pub source: String,
}

impl SnapshotManifest {
    pub fn manifest_path(snapshot: &Path) -> std::path::PathBuf {
        let mut value = snapshot.as_os_str().to_os_string();
        value.push(MANIFEST_SUFFIX);
        std::path::PathBuf::from(value)
    }

    pub fn write(&self, snapshot: &Path) -> Result<std::path::PathBuf> {
        let path = Self::manifest_path(snapshot);
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|err| Error::msg(format!("write manifest {}: {err}", path.display())))?;
        Ok(path)
    }
}
