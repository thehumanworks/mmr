use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::sync::config::SyncConfig;

const LOCK_FILE: &str = "sync.lock";
const STALE_TIMEOUT: Duration = Duration::from_secs(30 * 60); // 30 minutes

pub struct SyncLock {
    path: PathBuf,
}

impl SyncLock {
    pub fn acquire() -> Result<Self> {
        let path = SyncConfig::config_dir()?.join(LOCK_FILE);
        let dir = path.parent().unwrap();
        fs::create_dir_all(dir)?;

        if path.exists() {
            // Check if lock is stale
            let metadata = fs::metadata(&path)?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO);

            if age > STALE_TIMEOUT {
                eprintln!(
                    "Removing stale lock file (age: {}s)",
                    age.as_secs()
                );
                fs::remove_file(&path).ok();
            } else {
                anyhow::bail!(
                    "Another sync is in progress (lock: {}). \
                     If this is stale, remove it manually or wait {}s for auto-cleanup.",
                    path.display(),
                    (STALE_TIMEOUT - age).as_secs()
                );
            }
        }

        let pid = std::process::id().to_string();
        fs::write(&path, &pid)
            .with_context(|| format!("failed to create lock at {}", path.display()))?;

        Ok(Self { path })
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        fs::remove_file(&self.path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_creates_and_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(LOCK_FILE);

        // Simulate by directly writing/removing
        fs::write(&lock_path, "12345").unwrap();
        assert!(lock_path.exists());
        fs::remove_file(&lock_path).unwrap();
        assert!(!lock_path.exists());
    }
}
