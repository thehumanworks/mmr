#[allow(dead_code)]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{TestFixture, parse_stdout_json};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Handoff {
    _source: TestFixture,
    _destination: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    entry: PathBuf,
    messages: Value,
}

impl Handoff {
    fn new(source: &str) -> Self {
        let fixture = TestFixture::seeded();
        let source_project = fixture.home.join("source project");
        fs::create_dir_all(&source_project).unwrap();
        let source_project = fs::canonicalize(source_project).unwrap();
        let (session, old_project, original) = match source {
            "codex" => (
                "sess-codex-1",
                "/Users/test/codex-proj",
                ".codex/sessions/sess-codex-1.jsonl",
            ),
            "claude" => (
                "sess-claude-1",
                "/Users/test/proj",
                ".claude/projects/-Users-test-proj/sess-claude-1.jsonl",
            ),
            "pi" => (
                "sess-pi-1",
                "/Users/test/pi-proj",
                ".pi/agent/sessions/--Users-test-pi-proj--/2025-01-04T00-00-00-000Z_sess-pi-1.jsonl",
            ),
            _ => unreachable!(),
        };
        let original = fixture.home.join(original);
        let text = fs::read_to_string(&original)
            .unwrap()
            .replace(old_project, source_project.to_str().unwrap());
        let native = if source == "claude" {
            fixture
                .home
                .join(".claude/projects")
                .join(source_project.to_str().unwrap().replace('/', "-"))
                .join(format!("{session}.jsonl"))
        } else {
            original.clone()
        };
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        if native != original {
            fs::remove_file(&original).unwrap();
        }
        fs::write(&native, &text).unwrap();
        let read = fixture.run_cli_in_dir(
            &["--source", source, "read", "session", session],
            &source_project,
        );
        let messages = success(&read)["messages"].clone();
        assert!(!messages.as_array().unwrap().is_empty());
        let inbox = fixture.home.join("handoff inbox");
        let target = format!("file://{}", inbox.display());
        let share = fixture.run_cli_in_dir(
            &[
                "--source", source, "share", "session", session, "--to", &target,
            ],
            &source_project,
        );
        let response = success(&share);
        assert_eq!(response["transport"], "file");
        let source_entry = PathBuf::from(response["inbox_path"].as_str().unwrap());
        assert!(!source_entry.join("bundle.mmr.partial").exists());
        assert_eq!(fs::read_to_string(&native).unwrap(), text);

        let destination = tempfile::tempdir().unwrap();
        let home = destination.path().join("home");
        let project = home.join("different project");
        let entry = home.join("copied handoff");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&entry).unwrap();
        for name in ["bundle.mmr", "bundle.sha256", "ready"] {
            fs::copy(source_entry.join(name), entry.join(name)).unwrap();
        }
        Self {
            _source: fixture,
            _destination: destination,
            home,
            project,
            entry,
            messages,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(&self.project)
            .args(args)
            .output()
            .unwrap()
    }

    fn import(&self, flags: &[&str]) -> Output {
        let mut args = vec!["import", "bundle", self.entry.to_str().unwrap()];
        args.extend_from_slice(flags);
        self.run(&args)
    }

    fn assert_no_native_files(&self) {
        for name in [".codex", ".claude", ".pi", ".cursor", ".grok"] {
            assert!(
                !self.home.join(name).exists(),
                "unexpected provider write: {name}"
            );
        }
    }
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_stdout_json(output)
}

fn failure(output: &Output, message: &str) {
    assert!(!output.status.success());
    let response = parse_stdout_json(output);
    assert_eq!(response["command"], "import/bundle");
    assert!(
        response["message"].as_str().unwrap().contains(message),
        "{response}"
    );
}

#[test]
fn portable_file_roundtrip_defaults_to_read_only_across_homes() {
    for source in ["codex", "claude", "pi"] {
        let handoff = Handoff::new(source);
        let dry = success(&handoff.import(&["--dry-run"]));
        assert_eq!(dry["messages"], handoff.messages);
        assert!(!handoff.home.join(".mmr").exists());
        let read = success(&handoff.import(&[]));
        assert_eq!(read["messages"], handoff.messages);
        assert_eq!(read["session"]["source"], source);
        assert!(Path::new(read["bundle_path"].as_str().unwrap()).is_file());
        assert!(read.get("apply").is_none());
        handoff.assert_no_native_files();
        let repeat = success(&handoff.import(&["--read-only"]));
        assert_eq!(repeat["messages"], handoff.messages);
        assert_eq!(repeat["status"], "skipped");
        let markdown = handoff.import(&["-O", "md"]);
        assert!(markdown.status.success());
        assert!(
            success(&markdown)["text"]
                .as_str()
                .unwrap()
                .starts_with("# Import bundle")
        );
        handoff.assert_no_native_files();

        // Native restoration stays explicit and remaps to the destination project.
        let project = handoff.project.to_str().unwrap();
        let apply = success(&handoff.import(&["--apply", "--project", project]));
        assert_eq!(apply["apply"]["target_project"], project);
        assert_eq!(apply["apply"]["native"]["written"], true);
        let paths = apply["apply"]["native"]["paths"].as_array().unwrap();
        assert!(!paths.is_empty());
        // A newer colliding native file must survive subsequent default imports.
        for path in paths {
            fs::write(
                path.as_str().unwrap(),
                json!({"timestamp":"2099-01-01T00:00:00Z", "message":"destination-only content"})
                    .to_string(),
            )
            .unwrap();
        }
        let preserved = paths
            .iter()
            .map(|p| fs::read(p.as_str().unwrap()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(success(&handoff.import(&[]))["messages"], handoff.messages);
        for (path, bytes) in paths.iter().zip(preserved) {
            assert_eq!(fs::read(path.as_str().unwrap()).unwrap(), bytes);
        }
    }
}

#[test]
fn portable_file_rejects_hash_damage_truncation_and_bundle_id_traversal_before_caching() {
    let handoff = Handoff::new("codex");
    let original = fs::read(handoff.entry.join("bundle.mmr")).unwrap();
    let mut damaged: Value = serde_json::from_slice(&original).unwrap();
    let path = damaged["files"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    damaged["files"][&path] = json!("tampered");
    let bad = handoff.home.join("bad.mmr");
    fs::write(&bad, damaged.to_string()).unwrap();
    failure(
        &handoff.run(&["import", "bundle", bad.to_str().unwrap()]),
        "artifact hash mismatch",
    );
    fs::write(&bad, &original[..original.len() / 2]).unwrap();
    failure(
        &handoff.run(&["import", "bundle", bad.to_str().unwrap()]),
        "parse import/bundle bundle JSON",
    );
    let mut traversal: Value = serde_json::from_slice(&original).unwrap();
    traversal["manifest"]["bundle_id"] = json!("../../outside");
    fs::write(&bad, traversal.to_string()).unwrap();
    failure(
        &handoff.run(&["import", "bundle", bad.to_str().unwrap()]),
        "bundle_id does not match",
    );
    fs::write(handoff.entry.join("bundle.sha256"), "sha256:wrong").unwrap();
    failure(&handoff.import(&[]), "bundle hash mismatch");
    handoff.assert_no_native_files();
    assert!(!handoff.home.join(".mmr").exists());
    assert!(!handoff.home.join("outside").exists());
}

#[test]
fn portable_file_waits_for_ready_marker_without_importing_partial_writes() {
    let handoff = Handoff::new("pi");
    fs::remove_file(handoff.entry.join("ready")).unwrap();
    fs::rename(
        handoff.entry.join("bundle.mmr"),
        handoff.entry.join("bundle.mmr.partial"),
    )
    .unwrap();
    let partial = fs::read(handoff.entry.join("bundle.mmr.partial")).unwrap();
    let read = success(&handoff.import(&[]));
    assert_eq!(read["message_count"], 0);
    assert_eq!(read["cached"], json!([]));
    assert!(!handoff.home.join(".mmr").exists());
    assert_eq!(
        fs::read(handoff.entry.join("bundle.mmr.partial")).unwrap(),
        partial
    );
    handoff.assert_no_native_files();
}

#[test]
fn portable_file_native_traversal_is_rejected_on_explicit_apply() {
    let handoff = Handoff::new("codex");
    let mut bundle: Value =
        serde_json::from_slice(&fs::read(handoff.entry.join("bundle.mmr")).unwrap()).unwrap();
    bundle["metadata"]["native_source_file"] = json!("/source/.codex/../../escape.jsonl");
    // Metadata is hashed separately from the bundle's artifact identity.
    let metadata: mmr::teleport::BundleMetadata =
        serde_json::from_value(bundle["metadata"].clone()).unwrap();
    let hash = format!(
        "sha256:{}",
        Sha256::digest(serde_json::to_string(&metadata).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    for artifact in bundle["manifest"]["artifacts"].as_array_mut().unwrap() {
        if artifact["path"] == "metadata.json" {
            artifact["sha256"] = json!(hash);
        }
    }
    let bad = handoff.home.join("traversal.mmr");
    fs::write(&bad, bundle.to_string()).unwrap();
    failure(
        &handoff.run(&["import", "bundle", bad.to_str().unwrap(), "--apply"]),
        "path traversal",
    );
    handoff.assert_no_native_files();
    assert!(!handoff.home.join("escape.jsonl").exists());
}

#[test]
fn import_force_requires_explicit_apply_and_stdin_does_not_implicitly_apply() {
    let handoff = Handoff::new("codex");
    failure(&handoff.import(&["--force"]), "--force requires --apply");
    failure(
        &handoff.import(&["--read-only", "--apply"]),
        "either --read-only or --apply",
    );
    failure(
        &handoff.run(&["import", "bundle", "--to", "-"]),
        "stdin bundle import requires --apply",
    );
    handoff.assert_no_native_files();
    assert!(!handoff.home.join(".mmr").exists());
}
