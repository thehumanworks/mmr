pub mod config;
pub mod daemon;
pub mod lock;
pub mod manifest;
pub mod pull;
pub mod push;
pub mod status;
pub mod storage;

use anyhow::Result;
use serde::Serialize;

use crate::sync::config::SyncConfig;
use crate::sync::lock::SyncLock;
use crate::sync::manifest::SyncManifest;
use crate::sync::storage::{R2Storage, StorageBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Linux,
    Unsupported,
}

pub fn detect_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOS
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else {
        Platform::Unsupported
    }
}

#[derive(Serialize, Debug)]
pub struct SyncResponse {
    pub action: String,
    pub files_processed: usize,
    pub files_skipped: usize,
    pub bytes_transferred: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

pub async fn run_push(dry_run: bool) -> Result<SyncResponse> {
    let config = SyncConfig::load()?;
    let _lock = SyncLock::acquire()?;
    let manifest = SyncManifest::load()?;
    let backend = create_backend(&config)?;
    push::execute(backend.as_ref(), manifest, &config, dry_run).await
}

pub async fn run_pull(dry_run: bool) -> Result<SyncResponse> {
    let config = SyncConfig::load()?;
    let _lock = SyncLock::acquire()?;
    let manifest = SyncManifest::load()?;
    let backend = create_backend(&config)?;
    pull::execute(backend.as_ref(), manifest, &config, dry_run).await
}

pub async fn run_status() -> Result<SyncResponse> {
    let config = SyncConfig::load()?;
    let manifest = SyncManifest::load()?;
    let backend = create_backend(&config)?;
    status::execute(backend.as_ref(), &manifest, &config).await
}

pub fn run_init() -> Result<String> {
    config::interactive_init()
}

pub fn run_install(interval: u32) -> Result<String> {
    daemon::install(interval)
}

pub fn run_uninstall() -> Result<String> {
    daemon::uninstall()
}

fn create_backend(config: &SyncConfig) -> Result<Box<dyn StorageBackend>> {
    Ok(Box::new(R2Storage::new(config)?))
}

/// Create a memory-backed sync runner for testing.
#[cfg(test)]
pub fn create_test_backend() -> Box<dyn StorageBackend> {
    Box::new(storage::MemoryStorage::new())
}
