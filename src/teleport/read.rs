use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::TeleportStatus;
use super::bundle::{
    TeleportBundleFile, bundle_sha256, cache_dir, load_bundle_from_locator, verify_artifact_hashes,
    write_bundle,
};
use super::error::TeleportFailure;
use super::file::{
    BUNDLE_FILENAME, InboxEntryState, ReceiveLocatorKind, SHA256_FILENAME,
    classify_receive_locator, inbox_entry_state, verify_ready_inbox_bundle,
};
use super::http::{fetch_and_cache_http_bundle, is_http_locator, parse_http_locator};
use super::receive::{ReceiveStagedSummary, resolve_teleport_locator};

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub locator: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadSessionSummary {
    pub source: String,
    pub source_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReadResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub transport: &'static str,
    pub locator: String,
    pub cached: Vec<ReceiveStagedSummary>,
    pub bundle_id: String,
    pub bundle_path: String,
    pub session: ReadSessionSummary,
    pub message_count: u64,
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
}

pub fn read_bundle(options: ReadOptions) -> Result<ReadResponse, TeleportFailure> {
    if is_http_locator(&options.locator) {
        return read_http(&options);
    }

    let path = PathBuf::from(&options.locator);
    let kind = classify_receive_locator(&path)?;

    match kind {
        ReceiveLocatorKind::DirectBundle(bundle_path) => {
            read_direct_bundle(&options.locator, bundle_path, options.dry_run)
        }
        ReceiveLocatorKind::InboxEntry(entry_dir) => {
            read_inbox_entry(&options.locator, &entry_dir, options.dry_run)
        }
    }
}

pub fn resolve_read_locator(
    positional: Option<String>,
    to: Option<String>,
) -> Result<String, TeleportFailure> {
    resolve_teleport_locator("teleport/read", "read", positional, to)
}

fn read_http(options: &ReadOptions) -> Result<ReadResponse, TeleportFailure> {
    let target = parse_http_locator(&options.locator)?;
    let cached = fetch_and_cache_http_bundle(&target, options.dry_run, "teleport/read")?;
    build_read_response(
        options.locator.clone(),
        "http",
        cached,
        options.dry_run,
        None,
        None,
    )
}

fn read_direct_bundle(
    locator: &str,
    bundle_path: PathBuf,
    dry_run: bool,
) -> Result<ReadResponse, TeleportFailure> {
    let bundle = load_bundle_from_locator(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    let sha256 = bundle_sha256(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    let (cached, status) = stage_bundle_in_cache(&bundle, &bundle_path, sha256, dry_run)?;
    build_read_response(
        locator.to_string(),
        "file",
        vec![cached],
        dry_run,
        Some(status),
        Some(&bundle),
    )
}

fn read_inbox_entry(
    locator: &str,
    entry_dir: &Path,
    dry_run: bool,
) -> Result<ReadResponse, TeleportFailure> {
    match inbox_entry_state(entry_dir) {
        InboxEntryState::Waiting => Ok(ReadResponse {
            command: "teleport/read",
            status: TeleportStatus::Ok,
            transport: "file",
            locator: locator.to_string(),
            cached: Vec::new(),
            bundle_id: String::new(),
            bundle_path: String::new(),
            session: ReadSessionSummary {
                source: String::new(),
                source_session_id: String::new(),
                project_name: None,
            },
            message_count: 0,
            dry_run,
            next_command: None,
        }),
        InboxEntryState::Ready => {
            let bundle_path = entry_dir.join(BUNDLE_FILENAME);
            let sha256_path = entry_dir.join(SHA256_FILENAME);
            verify_ready_inbox_bundle(&bundle_path, &sha256_path)?;
            read_direct_bundle(locator, bundle_path, dry_run)
        }
    }
}

fn stage_bundle_in_cache(
    bundle: &TeleportBundleFile,
    source_path: &Path,
    sha256: String,
    dry_run: bool,
) -> Result<(ReceiveStagedSummary, TeleportStatus), TeleportFailure> {
    let bundle_id = bundle.manifest.bundle_id.clone();
    let cache_root = cache_dir(&bundle_id)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    let cache_bundle = cache_root.join(BUNDLE_FILENAME);

    if dry_run {
        return Ok((
            ReceiveStagedSummary {
                bundle_id,
                inbox_path: cache_root.display().to_string(),
                bundle_path: cache_bundle.display().to_string(),
                sha256,
            },
            TeleportStatus::Ok,
        ));
    }

    fs::create_dir_all(&cache_root)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;

    let status = if cache_bundle.exists() {
        let existing_sha256 = bundle_sha256(&cache_bundle)
            .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
        if existing_sha256 == sha256 {
            TeleportStatus::Skipped
        } else {
            write_bundle(&cache_bundle, bundle)
                .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
            TeleportStatus::Ok
        }
    } else if source_path == cache_bundle {
        TeleportStatus::Skipped
    } else {
        write_bundle(&cache_bundle, bundle)
            .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
        TeleportStatus::Ok
    };

    Ok((
        ReceiveStagedSummary {
            bundle_id,
            inbox_path: cache_root.display().to_string(),
            bundle_path: cache_bundle.display().to_string(),
            sha256,
        },
        status,
    ))
}

fn build_read_response(
    locator: String,
    transport: &'static str,
    cached: Vec<ReceiveStagedSummary>,
    dry_run: bool,
    explicit_status: Option<TeleportStatus>,
    preloaded: Option<&TeleportBundleFile>,
) -> Result<ReadResponse, TeleportFailure> {
    let entry = cached.first().ok_or_else(|| {
        TeleportFailure::runtime("teleport/read", "read produced no cached bundle entry")
    })?;
    if entry.bundle_id.is_empty() && dry_run {
        return Ok(ReadResponse {
            command: "teleport/read",
            status: TeleportStatus::Ok,
            transport,
            locator,
            cached,
            bundle_id: String::new(),
            bundle_path: String::new(),
            session: ReadSessionSummary {
                source: String::new(),
                source_session_id: String::new(),
                project_name: None,
            },
            message_count: 0,
            dry_run,
            next_command: None,
        });
    }

    let loaded;
    let bundle = match preloaded {
        Some(b) => b,
        None => {
            let bundle_path = PathBuf::from(&entry.bundle_path);
            loaded = load_bundle_from_locator(&bundle_path)
                .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
            &loaded
        }
    };
    let status = explicit_status.unwrap_or(TeleportStatus::Ok);
    let bundle_path_display = entry.bundle_path.clone();
    Ok(ReadResponse {
        command: "teleport/read",
        status,
        transport,
        locator,
        cached,
        bundle_id: bundle.manifest.bundle_id.clone(),
        bundle_path: bundle_path_display.clone(),
        session: ReadSessionSummary {
            source: bundle.manifest.source.clone(),
            source_session_id: bundle.manifest.session.source_session_id.clone(),
            project_name: Some(bundle.metadata.project_name.clone()),
        },
        message_count: bundle.manifest.session.message_count as u64,
        dry_run,
        next_command: Some(format!(
            "mmr teleport export {bundle_path_display} --to - --as same"
        )),
    })
}
