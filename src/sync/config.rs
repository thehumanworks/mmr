use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::source::resolve_home_dir;

const CONFIG_DIR: &str = ".config/mmr";
const CONFIG_FILE: &str = "sync.toml";

const ENV_SYNC_ENDPOINT: &str = "MMR_SYNC_ENDPOINT";
const ENV_SYNC_BUCKET: &str = "MMR_SYNC_BUCKET";
const ENV_SYNC_ACCESS_KEY_ID: &str = "MMR_SYNC_ACCESS_KEY_ID";
const ENV_SYNC_SECRET_ACCESS_KEY: &str = "MMR_SYNC_SECRET_ACCESS_KEY";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub storage: StorageConfig,
    #[serde(default)]
    pub sync: ScheduleConfig,
    #[serde(default)]
    pub sources: SourcesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default = "default_region")]
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    #[serde(default = "default_quiet_start")]
    pub quiet_start: String,
    #[serde(default = "default_quiet_end")]
    pub quiet_end: String,
    #[serde(default = "default_quiet_timezone")]
    pub quiet_timezone: String,
    #[serde(default = "default_interval")]
    pub interval_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesConfig {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
}

fn default_provider() -> String {
    "r2".to_string()
}
fn default_region() -> String {
    "auto".to_string()
}
fn default_quiet_start() -> String {
    "03:00".to_string()
}
fn default_quiet_end() -> String {
    "09:00".to_string()
}
fn default_quiet_timezone() -> String {
    "Europe/London".to_string()
}
fn default_interval() -> u32 {
    15
}
fn default_true() -> bool {
    true
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            quiet_start: default_quiet_start(),
            quiet_end: default_quiet_end(),
            quiet_timezone: default_quiet_timezone(),
            interval_minutes: default_interval(),
        }
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            claude: true,
            codex: true,
        }
    }
}

impl SyncConfig {
    pub fn config_dir() -> Result<PathBuf> {
        let home = resolve_home_dir()?;
        Ok(home.join(CONFIG_DIR))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join(CONFIG_FILE))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            anyhow::bail!(
                "sync not configured. Run `mmr sync init` to set up cloud sync.\n\
                 Expected config at: {}",
                path.display()
            );
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut config: SyncConfig =
            toml::from_str(&content).with_context(|| format!("invalid config in {}", path.display()))?;

        // Environment variable overrides
        if let Ok(v) = std::env::var(ENV_SYNC_ENDPOINT) {
            config.storage.endpoint = v;
        }
        if let Ok(v) = std::env::var(ENV_SYNC_BUCKET) {
            config.storage.bucket = v;
        }
        if let Ok(v) = std::env::var(ENV_SYNC_ACCESS_KEY_ID) {
            config.storage.access_key_id = v;
        }
        if let Ok(v) = std::env::var(ENV_SYNC_SECRET_ACCESS_KEY) {
            config.storage.secret_access_key = v;
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let dir = path.parent().unwrap();
        fs::create_dir_all(dir)?;

        let content = toml::to_string_pretty(self)
            .context("failed to serialize config")?;
        fs::write(&path, &content)
            .with_context(|| format!("failed to write {}", path.display()))?;

        // Set file permissions to 0600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }
}

pub fn interactive_init() -> Result<String> {
    let path = SyncConfig::config_path()?;
    if path.exists() {
        eprintln!("Config already exists at: {}", path.display());
        eprint!("Overwrite? [y/N] ");
        io::stderr().flush()?;
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            return Ok("init cancelled".to_string());
        }
    }

    let endpoint = prompt_input("Cloudflare R2 endpoint (https://<account_id>.r2.cloudflarestorage.com)")?;
    let bucket = prompt_input("Bucket name")?;
    let access_key_id = prompt_input("Access Key ID")?;
    let secret_access_key = prompt_input("Secret Access Key")?;

    let config = SyncConfig {
        storage: StorageConfig {
            provider: "r2".to_string(),
            endpoint,
            bucket,
            access_key_id,
            secret_access_key,
            region: "auto".to_string(),
        },
        sync: ScheduleConfig::default(),
        sources: SourcesConfig::default(),
    };

    config.save()?;

    Ok(format!(
        "Sync configured. Config saved to: {}\nRun `mmr sync push` to start syncing or `mmr sync install` to enable the background daemon.",
        path.display()
    ))
}

fn prompt_input(label: &str) -> Result<String> {
    eprint!("{}: ", label);
    io::stderr().flush()?;
    let mut value = String::new();
    io::stdin().lock().read_line(&mut value)?;
    let value = value.trim().to_string();
    if value.is_empty() {
        anyhow::bail!("{} cannot be empty", label);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let config = SyncConfig {
            storage: StorageConfig {
                provider: "r2".to_string(),
                endpoint: "https://test.r2.cloudflarestorage.com".to_string(),
                bucket: "test-bucket".to_string(),
                access_key_id: "AKID".to_string(),
                secret_access_key: "SECRET".to_string(),
                region: "auto".to_string(),
            },
            sync: ScheduleConfig::default(),
            sources: SourcesConfig::default(),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: SyncConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.storage.bucket, "test-bucket");
        assert_eq!(parsed.storage.access_key_id, "AKID");
        assert!(parsed.sources.claude);
        assert!(parsed.sources.codex);
        assert_eq!(parsed.sync.interval_minutes, 15);
    }

    #[test]
    fn config_defaults() {
        let minimal = r#"
[storage]
endpoint = "https://test.r2.cloudflarestorage.com"
bucket = "b"
access_key_id = "k"
secret_access_key = "s"
"#;
        let config: SyncConfig = toml::from_str(minimal).unwrap();
        assert_eq!(config.storage.provider, "r2");
        assert_eq!(config.storage.region, "auto");
        assert_eq!(config.sync.quiet_start, "03:00");
        assert_eq!(config.sync.quiet_end, "09:00");
        assert_eq!(config.sync.quiet_timezone, "Europe/London");
        assert_eq!(config.sync.interval_minutes, 15);
        assert!(config.sources.claude);
        assert!(config.sources.codex);
    }
}
