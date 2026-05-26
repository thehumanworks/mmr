use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::bundle::{bundle_sha256, load_bundle, verify_artifact_hashes};
use super::error::TeleportFailure;

pub const BUNDLE_FILENAME: &str = "bundle.mmr";
pub const BUNDLE_PARTIAL_FILENAME: &str = "bundle.mmr.partial";
pub const SHA256_FILENAME: &str = "bundle.sha256";
pub const READY_FILENAME: &str = "ready";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxBundlePaths {
    pub inbox_dir: PathBuf,
    pub bundle_path: PathBuf,
    pub partial_path: PathBuf,
    pub sha256_path: PathBuf,
    pub ready_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveLocatorKind {
    DirectBundle(PathBuf),
    InboxEntry(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxEntryState {
    Waiting,
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSendPlan {
    pub inbox_path: String,
    pub bundle_path: String,
    pub sha256_path: String,
    pub ready_path: String,
}

pub fn is_file_url(value: &str) -> bool {
    value.trim().starts_with("file://")
}

pub fn parse_locator(value: &str) -> Result<PathBuf, TeleportFailure> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(TeleportFailure::usage(
            "teleport/receive",
            "locator path is required",
        ));
    }
    if is_file_url(trimmed) {
        parse_file_url_for_command(trimmed, "teleport/receive")
    } else {
        expand_path(PathBuf::from(trimmed))
    }
}

pub fn parse_file_url(value: &str) -> Result<PathBuf, TeleportFailure> {
    parse_file_url_for_command(value, "teleport/send")
}

fn parse_file_url_for_command(
    value: &str,
    command: &'static str,
) -> Result<PathBuf, TeleportFailure> {
    let trimmed = value.trim();
    if !trimmed.starts_with("file://") {
        return Err(TeleportFailure::usage(
            command,
            format!("file transport requires file:// target; got {trimmed:?}"),
        ));
    }
    let path_part = trimmed.trim_start_matches("file://");
    if path_part.is_empty() {
        return Err(TeleportFailure::usage(
            command,
            "file:// target must include a path",
        ));
    }
    expand_path(PathBuf::from(path_part))
}

pub fn expand_path(path: PathBuf) -> Result<PathBuf, TeleportFailure> {
    if path.as_os_str().is_empty() {
        return Ok(path);
    }
    let path_str = path.to_string_lossy();
    if path_str == "~" {
        let home = dirs::home_dir().ok_or_else(|| {
            TeleportFailure::runtime("teleport/file", "resolve HOME for path expansion")
        })?;
        return Ok(home);
    }
    if let Some(rest) = path_str.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| {
            TeleportFailure::runtime("teleport/file", "resolve HOME for path expansion")
        })?;
        return Ok(home.join(rest));
    }
    Ok(path)
}

pub fn inbox_bundle_paths(inbox_root: &Path, bundle_id: &str) -> InboxBundlePaths {
    let inbox_dir = inbox_root.join(bundle_id);
    InboxBundlePaths {
        bundle_path: inbox_dir.join(BUNDLE_FILENAME),
        partial_path: inbox_dir.join(BUNDLE_PARTIAL_FILENAME),
        sha256_path: inbox_dir.join(SHA256_FILENAME),
        ready_path: inbox_dir.join(READY_FILENAME),
        inbox_dir,
    }
}

pub fn classify_receive_locator(path: &Path) -> Result<ReceiveLocatorKind, TeleportFailure> {
    if path.is_file() || path.extension().is_some_and(|ext| ext == "mmr") {
        return Ok(ReceiveLocatorKind::DirectBundle(path.to_path_buf()));
    }
    if !path.is_dir() {
        return Err(TeleportFailure::runtime(
            "teleport/receive",
            format!(
                "locator {} is not a bundle file or inbox directory",
                path.display()
            ),
        ));
    }

    if path.join(BUNDLE_FILENAME).is_file()
        || path.join(BUNDLE_PARTIAL_FILENAME).is_file()
        || path.join(READY_FILENAME).is_file()
        || path.join(SHA256_FILENAME).is_file()
    {
        return Ok(ReceiveLocatorKind::InboxEntry(path.to_path_buf()));
    }

    Ok(ReceiveLocatorKind::InboxEntry(path.to_path_buf()))
}

pub fn inbox_entry_state(entry_dir: &Path) -> InboxEntryState {
    if entry_dir.join(READY_FILENAME).is_file() {
        InboxEntryState::Ready
    } else {
        InboxEntryState::Waiting
    }
}

pub fn verify_inbox_sidecar_hash(
    bundle_path: &Path,
    sha256_path: &Path,
) -> Result<(), TeleportFailure> {
    let expected = fs::read_to_string(sha256_path).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/receive",
            format!("read bundle sidecar {}: {error}", sha256_path.display()),
        )
    })?;
    let actual = bundle_sha256(bundle_path)
        .map_err(|error| TeleportFailure::runtime("teleport/receive", error.to_string()))?;
    if expected.trim() != actual {
        return Err(TeleportFailure::runtime(
            "teleport/receive",
            format!(
                "bundle hash mismatch: sidecar expected {}, computed {}",
                expected.trim(),
                actual
            ),
        )
        .with_error_kind("bundle_hash_mismatch"));
    }
    Ok(())
}

pub fn write_bundle_to_inbox(
    inbox_root: &Path,
    bundle_id: &str,
    source_bundle: &Path,
    sha256: &str,
    dry_run: bool,
) -> Result<FileSendPlan, TeleportFailure> {
    let paths = inbox_bundle_paths(inbox_root, bundle_id);
    let plan = FileSendPlan {
        inbox_path: paths.inbox_dir.display().to_string(),
        bundle_path: paths.bundle_path.display().to_string(),
        sha256_path: paths.sha256_path.display().to_string(),
        ready_path: paths.ready_path.display().to_string(),
    };

    if dry_run {
        return Ok(plan);
    }

    fs::create_dir_all(&paths.inbox_dir).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!(
                "create inbox directory {}: {error}",
                paths.inbox_dir.display()
            ),
        )
    })?;

    if paths.ready_path.exists() {
        fs::remove_file(&paths.ready_path).map_err(|error| {
            TeleportFailure::runtime(
                "teleport/send",
                format!(
                    "remove stale ready marker {}: {error}",
                    paths.ready_path.display()
                ),
            )
        })?;
    }

    let bundle_bytes = fs::read(source_bundle).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!("read local bundle {}: {error}", source_bundle.display()),
        )
    })?;

    fs::write(&paths.partial_path, &bundle_bytes).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!(
                "write partial bundle {}: {error}",
                paths.partial_path.display()
            ),
        )
    })?;
    fs::rename(&paths.partial_path, &paths.bundle_path).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!(
                "rename partial bundle to {}: {error}",
                paths.bundle_path.display()
            ),
        )
    })?;
    fs::write(&paths.sha256_path, format!("{sha256}\n")).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!(
                "write bundle sidecar {}: {error}",
                paths.sha256_path.display()
            ),
        )
    })?;
    fs::File::create(&paths.ready_path)
        .and_then(|mut file| file.write_all(&[]))
        .map_err(|error| {
            TeleportFailure::runtime(
                "teleport/send",
                format!("write ready marker {}: {error}", paths.ready_path.display()),
            )
        })?;

    Ok(plan)
}

pub fn verify_ready_inbox_bundle(
    bundle_path: &Path,
    sha256_path: &Path,
) -> Result<(), TeleportFailure> {
    verify_inbox_sidecar_hash(bundle_path, sha256_path)?;
    let bundle = load_bundle(bundle_path).map_err(|error| {
        TeleportFailure::runtime("teleport/receive", error.to_string())
            .with_error_kind("bundle_corrupt")
    })?;
    verify_artifact_hashes(&bundle).map_err(|error| {
        TeleportFailure::runtime("teleport/receive", error.to_string())
            .with_error_kind("bundle_hash_mismatch")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_file_url_expands_home_prefix() {
        let home = dirs::home_dir().expect("home");
        let parsed = parse_file_url("file://~/teleport-inbox").expect("file url");
        assert_eq!(parsed, home.join("teleport-inbox"));
    }

    #[test]
    fn write_bundle_to_inbox_uses_atomic_partial_then_ready_marker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let inbox_root = tmp.path().join("inbox");
        let source = tmp.path().join("source.mmr");
        fs::write(&source, r#"{"mmr_teleport_bundle_version":1}"#).expect("source bundle");

        let plan = write_bundle_to_inbox(&inbox_root, "tp:v1:test", &source, "sha256:abc", false)
            .expect("write inbox bundle");

        assert!(
            !PathBuf::from(&plan.bundle_path)
                .with_file_name(BUNDLE_PARTIAL_FILENAME)
                .exists()
        );
        assert!(Path::new(&plan.bundle_path).is_file());
        assert!(Path::new(&plan.sha256_path).is_file());
        assert!(Path::new(&plan.ready_path).is_file());
        assert_eq!(
            fs::read_to_string(&plan.sha256_path).expect("sha256"),
            "sha256:abc\n"
        );
    }

    #[test]
    fn write_bundle_to_inbox_dry_run_does_not_create_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let inbox_root = tmp.path().join("inbox");
        let source = tmp.path().join("source.mmr");
        fs::write(&source, "bundle").expect("source bundle");

        let plan = write_bundle_to_inbox(&inbox_root, "tp:v1:test", &source, "sha256:abc", true)
            .expect("dry run plan");
        assert!(plan.inbox_path.contains("tp:v1:test"));
        assert!(!inbox_root.exists());
    }

    #[test]
    fn inbox_entry_state_reports_waiting_without_ready() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let entry = tmp.path().join("tp:v1:test");
        fs::create_dir_all(&entry).expect("entry dir");
        fs::write(entry.join(BUNDLE_PARTIAL_FILENAME), "partial").expect("partial");
        assert_eq!(inbox_entry_state(&entry), InboxEntryState::Waiting);
    }
}
