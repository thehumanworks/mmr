use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::source::resolve_home_dir;
use crate::sync::config::SyncConfig;
use crate::sync::manifest::{FileDiff, SyncManifest};
use crate::sync::storage::StorageBackend;
use crate::sync::SyncResponse;

pub async fn execute(
    backend: &dyn StorageBackend,
    mut manifest: SyncManifest,
    config: &SyncConfig,
    dry_run: bool,
) -> Result<SyncResponse> {
    let home = resolve_home_dir()?;
    let local_files = collect_local_files(&home, config)?;

    let mut files_processed = 0usize;
    let mut files_skipped = 0usize;
    let mut bytes_transferred = 0u64;
    let mut details = Vec::new();

    for (relative_path, absolute_path) in &local_files {
        let content = fs::read(absolute_path)
            .with_context(|| format!("failed to read {}", absolute_path.display()))?;

        let sha256 = compute_sha256(&content);
        let diff = manifest.diff_file(relative_path, &sha256);

        match diff {
            FileDiff::Unchanged => {
                files_skipped += 1;
            }
            FileDiff::New | FileDiff::Modified => {
                let remote_key = format!("mmr/{}", relative_path);
                let size = content.len() as u64;

                if dry_run {
                    let action = if diff == FileDiff::New {
                        "would upload (new)"
                    } else {
                        "would upload (modified)"
                    };
                    details.push(format!("{}: {}", action, relative_path));
                    files_processed += 1;
                } else {
                    let etag = backend.put_object(&remote_key, &content).await?;
                    let now = now_iso8601();
                    manifest.update_entry(
                        relative_path.clone(),
                        sha256,
                        size,
                        now,
                        Some(etag),
                    );
                    bytes_transferred += size;
                    files_processed += 1;
                    let action = if diff == FileDiff::New { "new" } else { "modified" };
                    details.push(format!("uploaded ({}): {}", action, relative_path));
                }
            }
        }
    }

    if !dry_run && files_processed > 0 {
        // Upload updated manifest to remote
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        backend
            .put_object("mmr/manifest.json", manifest_json.as_bytes())
            .await?;
        manifest.last_push = Some(now_iso8601());
        manifest.save()?;
    }

    Ok(SyncResponse {
        action: if dry_run {
            "push (dry-run)".to_string()
        } else {
            "push".to_string()
        },
        files_processed,
        files_skipped,
        bytes_transferred,
        details,
        conflicts: Vec::new(),
    })
}

pub fn collect_local_files(
    home: &Path,
    config: &SyncConfig,
) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();

    if config.sources.claude {
        let claude_dir = home.join(".claude").join("projects");
        if claude_dir.exists() {
            collect_jsonl_files(&claude_dir, home, &mut files)?;
        }
    }

    if config.sources.codex {
        for subdir in &["sessions", "archived_sessions"] {
            let codex_dir = home.join(".codex").join(subdir);
            if codex_dir.exists() {
                collect_jsonl_files(&codex_dir, home, &mut files)?;
            }
        }
    }

    Ok(files)
}

fn collect_jsonl_files(
    dir: &Path,
    home: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    for entry in WalkDir::new(dir).follow_links(false).into_iter().flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let relative = path
                .strip_prefix(home)
                .with_context(|| format!("path {} is not under home", path.display()))?
                .to_string_lossy()
                .to_string();
            files.push((relative, path.to_path_buf()));
        }
    }
    Ok(())
}

pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn now_iso8601() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_deterministic() {
        let hash1 = compute_sha256(b"hello world");
        let hash2 = compute_sha256(b"hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // 256 bits = 64 hex chars
    }

    #[test]
    fn sha256_differs_for_different_input() {
        let hash1 = compute_sha256(b"hello");
        let hash2 = compute_sha256(b"world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn collect_local_files_empty_home() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            storage: crate::sync::config::StorageConfig {
                provider: "r2".to_string(),
                endpoint: "https://test.r2.cloudflarestorage.com".to_string(),
                bucket: "test".to_string(),
                access_key_id: "k".to_string(),
                secret_access_key: "s".to_string(),
                region: "auto".to_string(),
            },
            sync: Default::default(),
            sources: Default::default(),
        };
        let files = collect_local_files(dir.path(), &config).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn collect_local_files_finds_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let claude_proj = dir.path().join(".claude/projects/test-proj");
        fs::create_dir_all(&claude_proj).unwrap();
        fs::write(claude_proj.join("sess1.jsonl"), "{}").unwrap();
        fs::write(claude_proj.join("readme.md"), "hi").unwrap(); // should be ignored

        let config = SyncConfig {
            storage: crate::sync::config::StorageConfig {
                provider: "r2".to_string(),
                endpoint: "https://test.r2.cloudflarestorage.com".to_string(),
                bucket: "test".to_string(),
                access_key_id: "k".to_string(),
                secret_access_key: "s".to_string(),
                region: "auto".to_string(),
            },
            sync: Default::default(),
            sources: Default::default(),
        };
        let files = collect_local_files(dir.path(), &config).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].0.ends_with("sess1.jsonl"));
    }
}
