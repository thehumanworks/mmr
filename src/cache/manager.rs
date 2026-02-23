use anyhow::{Context, Result};
use duckdb::Connection;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::db::{cache_schema_version, init_db};
use crate::ingest::{now_unix_millis, now_unix_secs, refresh_incremental_cache};

const BG_REFRESH_LOCK_FILE: &str = ".mmr-refresh.lock";
const BG_REFRESH_COOLDOWN_FILE: &str = ".mmr-refresh.cooldown";
const BG_REFRESH_LOCK_ENV: &str = "MMR_BG_REFRESH_LOCK_PATH";
const BG_REFRESH_COOLDOWN_MILLIS: i64 = 2_000;
const BG_REFRESH_STALE_LOCK_SECS: i64 = 15 * 60;

pub fn cache_db_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("MMR_DB_PATH") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("MEMORY_DB_PATH") {
        return Ok(PathBuf::from(p));
    }

    let base = dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .context("Could not determine cache directory")?;

    let new_path = base.join("mmr").join("mmr.duckdb");
    let legacy_path = base.join("memory").join("memory.duckdb");
    if new_path.exists() {
        return Ok(new_path);
    }
    if legacy_path.exists() {
        return Ok(legacy_path);
    }
    Ok(new_path)
}

struct LockFileGuard {
    path: PathBuf,
    enabled: bool,
}

impl LockFileGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            enabled: true,
        }
    }

    fn disarm(&mut self) {
        self.enabled = false;
    }
}

impl Drop for LockFileGuard {
    fn drop(&mut self) {
        if self.enabled {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn maybe_remove_stale_refresh_lock(lock_path: &Path) {
    let Ok(meta) = std::fs::metadata(lock_path) else {
        return;
    };

    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if mtime <= 0 {
        return;
    }

    let age = now_unix_secs().saturating_sub(mtime);
    if age > BG_REFRESH_STALE_LOCK_SECS {
        let _ = std::fs::remove_file(lock_path);
    }
}

fn try_acquire_refresh_lock(lock_path: &Path) -> Result<Option<LockFileGuard>> {
    maybe_remove_stale_refresh_lock(lock_path);

    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(lock_path)
    {
        Ok(mut file) => {
            writeln!(file, "{} {}", std::process::id(), now_unix_secs())?;
            Ok(Some(LockFileGuard::new(lock_path.to_path_buf())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn read_cooldown_timestamp(path: &Path) -> Option<i64> {
    let raw = std::fs::read_to_string(path).ok()?;
    raw.trim().parse::<i64>().ok()
}

pub fn maybe_spawn_background_refresh() -> Result<()> {
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    std::fs::create_dir_all(cache_dir)?;

    let lock_path = cache_dir.join(BG_REFRESH_LOCK_FILE);
    let cooldown_path = cache_dir.join(BG_REFRESH_COOLDOWN_FILE);

    let mut lock = match try_acquire_refresh_lock(&lock_path)? {
        Some(lock) => lock,
        None => return Ok(()),
    };

    let now_ms = now_unix_millis();
    if let Some(last) = read_cooldown_timestamp(&cooldown_path) {
        if now_ms.saturating_sub(last) < BG_REFRESH_COOLDOWN_MILLIS {
            return Ok(());
        }
    }

    let _ = std::fs::write(&cooldown_path, format!("{now_ms}\n"));

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return Ok(()),
    };

    let spawned = std::process::Command::new(exe)
        .arg("--quiet")
        .arg("__background-refresh")
        .env(BG_REFRESH_LOCK_ENV, &lock_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    if spawned.is_err() {
        let _ = std::fs::remove_file(&cooldown_path);
        return Ok(());
    }

    lock.disarm();
    Ok(())
}

fn background_refresh_lock_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(BG_REFRESH_LOCK_ENV) {
        return Ok(PathBuf::from(path));
    }
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    Ok(cache_dir.join(BG_REFRESH_LOCK_FILE))
}

fn cache_tmp_path(cache_dir: &Path, prefix: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    cache_dir.join(format!("{prefix}-{}-{}.duckdb", std::process::id(), ts))
}

fn swap_cache_into_place(tmp_path: &Path, cache_path: &Path) -> Result<()> {
    if let Err(e) = std::fs::rename(tmp_path, cache_path) {
        if cache_path.exists() {
            std::fs::remove_file(cache_path)?;
            std::fs::rename(tmp_path, cache_path)?;
        } else {
            return Err(e.into());
        }
    }
    Ok(())
}

fn refresh_cli_cache_snapshot(quiet: bool) -> Result<()> {
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    std::fs::create_dir_all(cache_dir)?;

    let tmp_path = cache_tmp_path(cache_dir, ".mmr-cache-swr");

    if cache_path.exists() && std::fs::copy(&cache_path, &tmp_path).is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }

    let refresh_result: Result<()> = (|| {
        let mut conn = Connection::open(&tmp_path)?;
        init_db(&conn)?;

        let schema_version: Option<String> = conn
            .query_row(
                "SELECT value FROM cache_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .ok();
        if let Some(version) = schema_version {
            if version != cache_schema_version() {
                drop(conn);
                let _ = std::fs::remove_file(&tmp_path);
                conn = Connection::open(&tmp_path)?;
                init_db(&conn)?;
            }
        }

        refresh_incremental_cache(&conn, quiet)?;
        Ok(())
    })();

    if let Err(e) = refresh_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    swap_cache_into_place(&tmp_path, &cache_path)?;
    Ok(())
}

pub fn run_background_refresh_worker() -> Result<()> {
    let _lock_guard = LockFileGuard::new(background_refresh_lock_path()?);
    let _ = refresh_cli_cache_snapshot(true);
    Ok(())
}

pub fn open_cache_db_for_cli(quiet: bool, refresh: bool) -> Result<Connection> {
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    std::fs::create_dir_all(cache_dir)?;

    let mut conn = Connection::open(&cache_path)?;
    init_db(&conn)?;

    let schema_version: Option<String> = conn
        .query_row(
            "SELECT value FROM cache_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    if let Some(version) = schema_version {
        if version != cache_schema_version() {
            if !quiet {
                eprintln!(
                    "Cache schema changed ({} -> {}). Rebuilding cache at {}.",
                    version,
                    cache_schema_version(),
                    cache_path.display()
                );
            }
            drop(conn);
            let _ = std::fs::remove_file(&cache_path);
            conn = Connection::open(&cache_path)?;
            init_db(&conn)?;
        }
    }

    if refresh {
        refresh_incremental_cache(&conn, quiet)?;
    }

    Ok(conn)
}

pub fn rebuild_cli_cache(quiet: bool) -> Result<()> {
    let cache_path = cache_db_path()?;
    let cache_dir = cache_path
        .parent()
        .context("Cache path has no parent directory")?;
    std::fs::create_dir_all(cache_dir)?;

    let tmp_path = cache_tmp_path(cache_dir, ".mmr-cache");

    if !quiet {
        eprintln!("Building CLI cache at {}", cache_path.display());
        eprintln!("Ingesting conversation history...");
    }

    let ingest_result = (|| {
        let conn = Connection::open(&tmp_path)?;
        init_db(&conn)?;
        refresh_incremental_cache(&conn, quiet)
    })();

    let stats = match ingest_result {
        Ok(stats) => stats,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    };

    swap_cache_into_place(&tmp_path, &cache_path)?;

    if !quiet {
        eprintln!(
            "  Claude: {} messages from {} sessions across {} projects",
            stats.claude_messages, stats.claude_sessions, stats.claude_projects
        );
        eprintln!(
            "  Codex:  {} messages from {} sessions across {} projects",
            stats.codex_messages, stats.codex_sessions, stats.codex_projects
        );
        let total_messages = stats.claude_messages + stats.codex_messages;
        let total_sessions = stats.claude_sessions + stats.codex_sessions;
        eprintln!(
            "  Total:  {} messages, {} sessions",
            total_messages, total_sessions
        );
        eprintln!("Cache ready.");
    }

    Ok(())
}
