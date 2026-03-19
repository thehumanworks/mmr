use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::sync::config::SyncConfig;

const MANIFEST_FILE: &str = "sync-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    pub version: u32,
    #[serde(default)]
    pub last_push: Option<String>,
    #[serde(default)]
    pub last_pull: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub sha256: String,
    pub size: u64,
    pub last_modified: String,
    #[serde(default)]
    pub remote_etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDiff {
    New,
    Modified,
    Unchanged,
}

impl SyncManifest {
    pub fn new() -> Self {
        Self {
            version: 1,
            last_push: None,
            last_pull: None,
            files: BTreeMap::new(),
        }
    }

    pub fn manifest_path() -> Result<PathBuf> {
        Ok(SyncConfig::config_dir()?.join(MANIFEST_FILE))
    }

    pub fn load() -> Result<Self> {
        let path = Self::manifest_path()?;
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("invalid manifest in {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::manifest_path()?;
        let dir = path.parent().unwrap();
        fs::create_dir_all(dir)?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn diff_file(&self, relative_path: &str, sha256: &str) -> FileDiff {
        match self.files.get(relative_path) {
            None => FileDiff::New,
            Some(entry) if entry.sha256 != sha256 => FileDiff::Modified,
            Some(_) => FileDiff::Unchanged,
        }
    }

    pub fn update_entry(
        &mut self,
        relative_path: String,
        sha256: String,
        size: u64,
        last_modified: String,
        remote_etag: Option<String>,
    ) {
        self.files.insert(
            relative_path,
            FileEntry {
                sha256,
                size,
                last_modified,
                remote_etag,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manifest_is_empty() {
        let m = SyncManifest::new();
        assert_eq!(m.version, 1);
        assert!(m.files.is_empty());
        assert!(m.last_push.is_none());
    }

    #[test]
    fn diff_detects_new_file() {
        let m = SyncManifest::new();
        assert_eq!(m.diff_file("test.jsonl", "abc123"), FileDiff::New);
    }

    #[test]
    fn diff_detects_modified_file() {
        let mut m = SyncManifest::new();
        m.update_entry(
            "test.jsonl".to_string(),
            "old_hash".to_string(),
            100,
            "2025-01-01T00:00:00Z".to_string(),
            None,
        );
        assert_eq!(m.diff_file("test.jsonl", "new_hash"), FileDiff::Modified);
    }

    #[test]
    fn diff_detects_unchanged_file() {
        let mut m = SyncManifest::new();
        m.update_entry(
            "test.jsonl".to_string(),
            "same_hash".to_string(),
            100,
            "2025-01-01T00:00:00Z".to_string(),
            None,
        );
        assert_eq!(m.diff_file("test.jsonl", "same_hash"), FileDiff::Unchanged);
    }

    #[test]
    fn manifest_roundtrip() {
        let mut m = SyncManifest::new();
        m.last_push = Some("2025-06-15T10:00:00Z".to_string());
        m.update_entry(
            "claude/proj/sess.jsonl".to_string(),
            "abc".to_string(),
            512,
            "2025-06-15T09:00:00Z".to_string(),
            Some("\"etag123\"".to_string()),
        );

        let json = serde_json::to_string(&m).unwrap();
        let parsed: SyncManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(
            parsed.files["claude/proj/sess.jsonl"].sha256,
            "abc"
        );
        assert_eq!(
            parsed.files["claude/proj/sess.jsonl"].remote_etag,
            Some("\"etag123\"".to_string())
        );
    }
}
