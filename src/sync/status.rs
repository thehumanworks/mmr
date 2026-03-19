use anyhow::Result;

use crate::source::resolve_home_dir;
use crate::sync::config::SyncConfig;
use crate::sync::manifest::{FileDiff, SyncManifest};
use crate::sync::push::{collect_local_files, compute_sha256};
use crate::sync::storage::StorageBackend;
use crate::sync::SyncResponse;

pub async fn execute(
    backend: &dyn StorageBackend,
    manifest: &SyncManifest,
    config: &SyncConfig,
) -> Result<SyncResponse> {
    let home = resolve_home_dir()?;
    let local_files = collect_local_files(&home, config)?;

    let mut details = Vec::new();
    let mut conflicts = Vec::new();

    // Check local files against manifest (would-push)
    let mut local_new = 0usize;
    let mut local_modified = 0usize;
    let mut local_unchanged = 0usize;

    for (relative_path, absolute_path) in &local_files {
        let content = std::fs::read(absolute_path)?;
        let sha256 = compute_sha256(&content);
        match manifest.diff_file(relative_path, &sha256) {
            FileDiff::New => {
                details.push(format!("local new: {}", relative_path));
                local_new += 1;
            }
            FileDiff::Modified => {
                details.push(format!("local modified: {}", relative_path));
                local_modified += 1;
            }
            FileDiff::Unchanged => {
                local_unchanged += 1;
            }
        }
    }

    // Check remote manifest for files we don't have locally
    let remote_manifest = match backend.get_object("mmr/manifest.json").await {
        Ok(data) => serde_json::from_slice::<SyncManifest>(&data).ok(),
        Err(_) => None,
    };

    let mut remote_only = 0usize;

    if let Some(rm) = &remote_manifest {
        for (relative_path, remote_entry) in &rm.files {
            let local_path = home.join(relative_path);
            if !local_path.exists() {
                details.push(format!("remote only: {}", relative_path));
                remote_only += 1;
            } else {
                let content = std::fs::read(&local_path)?;
                let local_hash = compute_sha256(&content);
                if local_hash != remote_entry.sha256 {
                    conflicts.push(format!(
                        "diverged: {} (local: {}..., remote: {}...)",
                        relative_path,
                        &local_hash[..8],
                        &remote_entry.sha256[..8.min(remote_entry.sha256.len())]
                    ));
                }
            }
        }
    }

    details.insert(
        0,
        format!(
            "local: {} new, {} modified, {} synced | remote-only: {}",
            local_new, local_modified, local_unchanged, remote_only
        ),
    );

    if let Some(push) = &manifest.last_push {
        details.push(format!("last push: {}", push));
    }
    if let Some(pull) = &manifest.last_pull {
        details.push(format!("last pull: {}", pull));
    }

    Ok(SyncResponse {
        action: "status".to_string(),
        files_processed: local_new + local_modified + remote_only,
        files_skipped: local_unchanged,
        bytes_transferred: 0,
        details,
        conflicts,
    })
}
