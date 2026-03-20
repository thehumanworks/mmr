use std::fs;

use anyhow::{Context, Result};

use crate::source::resolve_home_dir;
use crate::sync::SyncResponse;
use crate::sync::config::SyncConfig;
use crate::sync::manifest::SyncManifest;
use crate::sync::push::compute_sha256;
use crate::sync::storage::StorageBackend;

pub async fn execute(
    backend: &dyn StorageBackend,
    mut manifest: SyncManifest,
    _config: &SyncConfig,
    dry_run: bool,
) -> Result<SyncResponse> {
    let home = resolve_home_dir()?;

    // Download remote manifest
    let remote_manifest = fetch_remote_manifest(backend).await?;

    let mut files_processed = 0usize;
    let mut files_skipped = 0usize;
    let mut bytes_transferred = 0u64;
    let mut details = Vec::new();
    let mut conflicts = Vec::new();

    for (relative_path, remote_entry) in &remote_manifest.files {
        let local_path = home.join(relative_path);

        if local_path.exists() {
            // Non-destructive: never overwrite existing files
            let local_content = fs::read(&local_path)
                .with_context(|| format!("failed to read {}", local_path.display()))?;
            let local_hash = compute_sha256(&local_content);

            if local_hash == remote_entry.sha256 {
                files_skipped += 1;
            } else {
                // Diverged: local differs from remote
                conflicts.push(format!(
                    "diverged (local kept): {} (local: {}..., remote: {}...)",
                    relative_path,
                    &local_hash[..8],
                    &remote_entry.sha256[..8.min(remote_entry.sha256.len())]
                ));
                files_skipped += 1;
            }
        } else {
            // File missing locally — download it
            let remote_key = format!("mmr/{}", relative_path);

            if dry_run {
                details.push(format!("would download: {}", relative_path));
                files_processed += 1;
            } else {
                let content = backend.get_object(&remote_key).await?;
                let size = content.len() as u64;

                // Create parent directories
                if let Some(parent) = local_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create directory {}", parent.display())
                    })?;
                }

                fs::write(&local_path, &content)
                    .with_context(|| format!("failed to write {}", local_path.display()))?;

                // Update local manifest
                let sha256 = compute_sha256(&content);
                manifest.update_entry(
                    relative_path.clone(),
                    sha256,
                    size,
                    remote_entry.last_modified.clone(),
                    remote_entry.remote_etag.clone(),
                );

                bytes_transferred += size;
                files_processed += 1;
                details.push(format!("downloaded: {}", relative_path));
            }
        }
    }

    if !dry_run && files_processed > 0 {
        manifest.last_pull = Some(now_iso8601());
        manifest.save()?;
    }

    Ok(SyncResponse {
        action: if dry_run {
            "pull (dry-run)".to_string()
        } else {
            "pull".to_string()
        },
        files_processed,
        files_skipped,
        bytes_transferred,
        details,
        conflicts,
    })
}

async fn fetch_remote_manifest(backend: &dyn StorageBackend) -> Result<SyncManifest> {
    match backend.get_object("mmr/manifest.json").await {
        Ok(data) => {
            let manifest: SyncManifest =
                serde_json::from_slice(&data).context("failed to parse remote manifest")?;
            Ok(manifest)
        }
        Err(_) => {
            // No remote manifest yet — nothing to pull
            Ok(SyncManifest::new())
        }
    }
}

fn now_iso8601() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::storage::MemoryStorage;

    #[tokio::test]
    async fn pull_from_empty_remote_does_nothing() {
        let backend = MemoryStorage::new();
        let manifest = SyncManifest::new();
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

        let result = execute(&backend, manifest, &config, true).await.unwrap();
        assert_eq!(result.files_processed, 0);
        assert_eq!(result.files_skipped, 0);
    }
}
