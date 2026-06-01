use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::TeleportOutputFormat;
use super::TeleportStatus;
use super::bundle::{
    TeleportBundleFile, bundle_sha256, cache_dir, hash_text, load_bundle_from_locator,
    messages_from_bundle, verify_artifact_hashes, write_bundle,
};
use super::error::TeleportFailure;
use super::file::{
    BUNDLE_FILENAME, InboxEntryState, ReceiveLocatorKind, SHA256_FILENAME,
    classify_receive_locator, inbox_entry_state, verify_ready_inbox_bundle,
};
use super::http::{fetch_and_cache_http_bundle, is_http_locator, parse_http_locator};
use super::receive::{ReceiveStagedSummary, resolve_teleport_locator};
use crate::types::api::ApiMessage;

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub locator: String,
    pub dry_run: bool,
    pub output_format: TeleportOutputFormat,
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
    pub messages: Vec<ApiMessage>,
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub fn read_bundle(options: ReadOptions) -> Result<ReadResponse, TeleportFailure> {
    if is_http_locator(&options.locator) {
        return read_http(&options);
    }

    let path = PathBuf::from(&options.locator);
    let kind = classify_receive_locator(&path)?;

    match kind {
        ReceiveLocatorKind::DirectBundle(bundle_path) => {
            read_direct_bundle(&options.locator, bundle_path, &options)
        }
        ReceiveLocatorKind::InboxEntry(entry_dir) => {
            read_inbox_entry(&options.locator, &entry_dir, &options)
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
    if !options.dry_run
        && let Some(cached) = cached_http_locator_entry(&options.locator)?
    {
        return build_read_response(
            options.locator.clone(),
            "http",
            vec![cached],
            options.dry_run,
            Some(TeleportStatus::Skipped),
            None,
            options.output_format,
        );
    }

    let cached = fetch_and_cache_http_bundle(&target, options.dry_run, "teleport/read")?;
    if !options.dry_run {
        record_http_locator_entry(&options.locator, &cached)?;
    }
    build_read_response(
        options.locator.clone(),
        "http",
        cached,
        options.dry_run,
        None,
        None,
        options.output_format,
    )
}

#[derive(Debug, Deserialize, Serialize)]
struct HttpReadLocatorCacheEntry {
    locator: String,
    bundle_id: String,
    inbox_path: String,
    bundle_path: String,
    sha256: String,
}

fn cached_http_locator_entry(
    locator: &str,
) -> Result<Option<ReceiveStagedSummary>, TeleportFailure> {
    let path = http_locator_cache_path(locator)?;
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    let Ok(entry) = serde_json::from_str::<HttpReadLocatorCacheEntry>(&content) else {
        return Ok(None);
    };
    if entry.locator != locator {
        return Ok(None);
    }

    let bundle_path = PathBuf::from(&entry.bundle_path);
    if !bundle_path.exists() {
        return Ok(None);
    }
    let sha256 = bundle_sha256(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    if sha256 != entry.sha256 {
        return Ok(None);
    }
    let bundle = load_bundle_from_locator(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    if bundle.manifest.bundle_id != entry.bundle_id {
        return Ok(None);
    }

    Ok(Some(ReceiveStagedSummary {
        bundle_id: entry.bundle_id,
        inbox_path: entry.inbox_path,
        bundle_path: entry.bundle_path,
        sha256: entry.sha256,
    }))
}

fn record_http_locator_entry(
    locator: &str,
    cached: &[ReceiveStagedSummary],
) -> Result<(), TeleportFailure> {
    let Some(entry) = cached.first() else {
        return Ok(());
    };
    if entry.bundle_id.is_empty() {
        return Ok(());
    }

    let path = http_locator_cache_path(locator)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    }
    let entry = HttpReadLocatorCacheEntry {
        locator: locator.to_string(),
        bundle_id: entry.bundle_id.clone(),
        inbox_path: entry.inbox_path.clone(),
        bundle_path: entry.bundle_path.clone(),
        sha256: entry.sha256.clone(),
    };
    let content = serde_json::to_string(&entry)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    fs::write(&path, content)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    Ok(())
}

fn http_locator_cache_path(locator: &str) -> Result<PathBuf, TeleportFailure> {
    let locator_hash = hash_text(locator).replace(':', "-");
    Ok(cache_dir("http-locators")
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?
        .join(format!("{locator_hash}.json")))
}

fn read_direct_bundle(
    locator: &str,
    bundle_path: PathBuf,
    options: &ReadOptions,
) -> Result<ReadResponse, TeleportFailure> {
    let bundle = load_bundle_from_locator(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    verify_artifact_hashes(&bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    let sha256 = bundle_sha256(&bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error.to_string()))?;
    let (cached, status) = stage_bundle_in_cache(&bundle, &bundle_path, sha256, options.dry_run)?;
    build_read_response(
        locator.to_string(),
        "file",
        vec![cached],
        options.dry_run,
        Some(status),
        Some(&bundle),
        options.output_format,
    )
}

fn read_inbox_entry(
    locator: &str,
    entry_dir: &Path,
    options: &ReadOptions,
) -> Result<ReadResponse, TeleportFailure> {
    match inbox_entry_state(entry_dir) {
        InboxEntryState::Waiting => Ok(empty_read_response(
            locator,
            "file",
            options.dry_run,
            options.output_format,
        )),
        InboxEntryState::Ready => {
            let bundle_path = entry_dir.join(BUNDLE_FILENAME);
            let sha256_path = entry_dir.join(SHA256_FILENAME);
            verify_ready_inbox_bundle(&bundle_path, &sha256_path)?;
            read_direct_bundle(locator, bundle_path, options)
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
    output_format: TeleportOutputFormat,
) -> Result<ReadResponse, TeleportFailure> {
    let entry = cached.first().ok_or_else(|| {
        TeleportFailure::runtime("teleport/read", "read produced no cached bundle entry")
    })?;
    if entry.bundle_id.is_empty() && dry_run {
        return Ok(empty_read_response(
            &locator,
            transport,
            dry_run,
            output_format,
        ));
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
    let messages = messages_from_bundle(bundle)
        .map_err(|error| TeleportFailure::runtime("teleport/read", error))?;
    let text = match output_format {
        TeleportOutputFormat::Md => Some(read_markdown(bundle, &messages)),
        TeleportOutputFormat::Json => None,
    };
    let bundle_path = entry.bundle_path.clone();
    Ok(ReadResponse {
        command: "teleport/read",
        status,
        transport,
        locator,
        cached,
        bundle_id: bundle.manifest.bundle_id.clone(),
        bundle_path,
        session: ReadSessionSummary {
            source: bundle.manifest.source.clone(),
            source_session_id: bundle.manifest.session.source_session_id.clone(),
            project_name: Some(bundle.metadata.project_name.clone()),
        },
        message_count: messages.len() as u64,
        messages,
        dry_run,
        text,
    })
}

fn empty_read_response(
    locator: &str,
    transport: &'static str,
    dry_run: bool,
    output_format: TeleportOutputFormat,
) -> ReadResponse {
    ReadResponse {
        command: "teleport/read",
        status: TeleportStatus::Ok,
        transport,
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
        messages: Vec::new(),
        dry_run,
        text: match output_format {
            TeleportOutputFormat::Md => {
                Some("# Import bundle\n\n(no session content)\n".to_string())
            }
            TeleportOutputFormat::Json => None,
        },
    }
}

fn read_markdown(bundle: &TeleportBundleFile, messages: &[ApiMessage]) -> String {
    let mut parts = vec![format!(
        "# Import bundle\n\n- source: {}\n- session_id: {}\n- project: {}\n- messages: {}\n",
        bundle.manifest.source,
        bundle.manifest.session.source_session_id,
        bundle.metadata.project_name,
        messages.len()
    )];
    for message in messages {
        parts.push(format!(
            "\n## {} ({}) — {}\n\n{}\n",
            message.role, message.timestamp, message.msg_type, message.content
        ));
    }
    parts.join("")
}
