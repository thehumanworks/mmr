use mmr::capture::{EventBoundary, event_hash_set, parse_fixture_jsonl};
use mmr::store::{LATEST_SCHEMA_VERSION, Store};
use std::io::Write;
use std::process::{Command, Stdio};

const FIXTURES: &[(&str, &str)] = &[
    (
        "codex_session",
        include_str!("fixtures/memory_fabric/codex_session.jsonl"),
    ),
    (
        "claude_like_session",
        include_str!("fixtures/memory_fabric/claude_like_session.jsonl"),
    ),
    (
        "human_note",
        include_str!("fixtures/memory_fabric/human_note.jsonl"),
    ),
    (
        "tool_output_fake_secret",
        include_str!("fixtures/memory_fabric/tool_output_fake_secret.jsonl"),
    ),
    (
        "pii_heavy_sample",
        include_str!("fixtures/memory_fabric/pii_heavy_sample.jsonl"),
    ),
];

const NHL_269_REQUIRED_TABLES: &[&str] = &[
    "blobs",
    "dream_candidates",
    "dream_runs",
    "events",
    "learned_memory",
    "project_aliases",
    "project_links",
    "projects",
    "redaction_policies",
    "redaction_runs",
    "redaction_spans",
    "schema_migrations",
    "search_documents",
    "sessions",
    "source_cursors",
    "sources",
    "summaries",
    "sync_manifest_entries",
    "sync_manifests",
];

const MALFORMED_MIXED_FIXTURE: &str =
    include_str!("fixtures/memory_fabric/malformed_mixed_session.jsonl");

const MVP_NON_GOAL_COMMANDS: &[&str] = &[
    "init",
    "store",
    "learn",
    "context",
    "candidates",
    "knowledge",
    "promote",
    "reject",
];

#[test]
fn memory_fabric_golden_fixtures_are_valid_jsonl() {
    for (name, contents) in FIXTURES {
        let mut parsed_records = 0;

        for line in contents.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|err| panic!("{name}: {err}"));
            assert!(
                value.is_object(),
                "{name}: fixture rows must be JSON objects"
            );
            parsed_records += 1;
        }

        assert!(parsed_records > 0, "{name}: fixture must not be empty");
    }
}

#[test]
fn memory_fabric_malformed_fixture_contains_valid_and_invalid_lines() {
    let mut valid_records = 0;
    let mut invalid_records = 0;

    for line in MALFORMED_MIXED_FIXTURE
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                assert!(
                    value.is_object(),
                    "valid malformed-fixture rows must be objects"
                );
                valid_records += 1;
            }
            Err(_) => invalid_records += 1,
        }
    }

    assert!(
        valid_records > 0,
        "malformed fixture must include valid rows to preserve defensive parsing"
    );
    assert!(
        invalid_records > 0,
        "malformed fixture must include invalid rows to exercise skip-and-continue parsing"
    );
}

#[test]
fn memory_fabric_non_goal_commands_are_not_public() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).expect("create HOME");

    for command_name in MVP_NON_GOAL_COMMANDS {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg(command_name)
            .env("HOME", &home)
            .output()
            .expect("run mmr");

        assert!(
            !output.status.success(),
            "{command_name} must remain outside the public MVP command surface"
        );
        assert!(
            output.stdout.is_empty(),
            "{command_name} should not write machine output when rejected"
        );
    }
}

#[test]
fn db_info_smoke_links_non_git_project_and_round_trips_synthetic_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "__db-info",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--smoke-event",
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("run mmr");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout JSON");
    assert_eq!(json["schema_version"].as_i64().unwrap(), 1);
    assert_eq!(json["event_count"].as_i64().unwrap(), 1);
    assert!(json["project_id"].as_str().unwrap().starts_with("proj:v1:"));
    assert_eq!(
        json["db_path"].as_str().unwrap(),
        data_home.join("mmr").join("mmr.db").to_str().unwrap()
    );
}

#[test]
fn source_adapter_normalization_contract_is_implemented() {
    let path = std::path::Path::new("tests/fixtures/memory_fabric/malformed_mixed_session.jsonl");
    let batch = parse_fixture_jsonl(
        "fixture",
        "fixture-jsonl-v1",
        "malformed-mixed-session",
        path,
        include_str!("fixtures/memory_fabric/malformed_mixed_session.jsonl"),
    )
    .expect("parse fixture");

    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.warnings.len(), 1);
    assert_eq!(event_hash_set(&batch.events).len(), 2);
    assert!(batch.events.iter().all(|event| {
        event.parser_version == "fixture-jsonl-v1"
            && event
                .raw_local_ref
                .contains("malformed_mixed_session.jsonl")
    }));

    let codex = parse_fixture_jsonl(
        "fixture",
        "fixture-jsonl-v1",
        "codex_session",
        std::path::Path::new("tests/fixtures/memory_fabric/codex_session.jsonl"),
        include_str!("fixtures/memory_fabric/codex_session.jsonl"),
    )
    .expect("parse codex fixture");
    assert_eq!(codex.events.len(), 3);
    assert!(
        codex
            .events
            .iter()
            .all(|event| event.source_session_id == "codex-mvp-1")
    );
    assert_eq!(codex.events[1].boundary, EventBoundary::UserTurn);
    assert_eq!(codex.events[2].boundary, EventBoundary::AssistantTurn);
    assert!(codex.events[2].content_text.contains("store contract"));

    let claude = parse_fixture_jsonl(
        "fixture",
        "fixture-jsonl-v1",
        "claude_like_session",
        std::path::Path::new("tests/fixtures/memory_fabric/claude_like_session.jsonl"),
        include_str!("fixtures/memory_fabric/claude_like_session.jsonl"),
    )
    .expect("parse claude-like fixture");
    assert_eq!(claude.events[0].boundary, EventBoundary::UserTurn);
    assert_eq!(claude.events[1].boundary, EventBoundary::AssistantTurn);

    let tool = parse_fixture_jsonl(
        "fixture",
        "fixture-jsonl-v1",
        "tool_output_fake_secret",
        std::path::Path::new("tests/fixtures/memory_fabric/tool_output_fake_secret.jsonl"),
        include_str!("fixtures/memory_fabric/tool_output_fake_secret.jsonl"),
    )
    .expect("parse tool fixture");
    assert_eq!(tool.events[0].boundary, EventBoundary::ToolResult);
}

#[test]
#[ignore = "pending NHL-277: implement link command"]
fn link_cli_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "mmr link sets up local state from a non-Git cwd, performs safe reconciliation, and prints status JSON",
    );
}

#[test]
#[ignore = "pending NHL-277: implement sync command"]
fn sync_cli_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "mmr sync is idempotent, redaction-gated, non-destructive, and prints sync/status JSON",
    );
}

#[test]
#[ignore = "pending NHL-277: implement status command"]
fn status_cli_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "mmr status reports store, project, source, redaction, and sync state as JSON",
    );
}

#[test]
fn note_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let link = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "__db-info",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("link project");
    assert!(
        link.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&link.stderr)
    );

    let inline = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["note", "decision:", "notes", "are", "memory"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("note inline");
    assert!(
        inline.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&inline.stderr)
    );
    let inline_json: serde_json::Value =
        serde_json::from_slice(&inline.stdout).expect("inline note JSON");
    assert_eq!(inline_json["source"].as_str().unwrap(), "note");
    assert!(
        inline_json["citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://event/")
    );

    let mut stdin_note = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("note")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stdin note");
    stdin_note
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"first line\nsecond line\n")
        .expect("write stdin note");
    let stdin_note = stdin_note.wait_with_output().expect("stdin note output");
    assert!(
        stdin_note.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&stdin_note.stderr)
    );

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let notes = store
        .events_for_project(&project_record.id, Some("note"), Some("notes"))
        .expect("note events");
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|event| event.source == "note"));
    assert!(notes.iter().all(|event| event.sync_status == "local_only"));
    assert!(notes.iter().all(|event| {
        !event
            .source_event_id
            .as_deref()
            .unwrap_or_default()
            .contains(&project_record.id)
    }));
    assert!(notes.iter().all(|event| {
        !event
            .raw_local_ref
            .as_deref()
            .unwrap_or_default()
            .contains(&project_record.id)
    }));
    let search_doc = store
        .search_document_by_event(&notes[0].id)
        .expect("search doc");
    assert_eq!(search_doc.source, "note");
    assert!(search_doc.document_text.contains("decision: notes"));
}

#[test]
fn note_requires_linked_project() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["note", "unlinked"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("note unlinked");
    assert!(!output.status.success(), "unlinked note should fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not linked"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rg_cli_contract_is_implemented() {
    let (_tmp, home, data_home, project, codex_event_id, _) = seed_search_fixture();

    let exact = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["rg", "panic at src/main.rs:42"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("rg exact");
    assert!(
        exact.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&exact.stderr)
    );
    let exact_json: serde_json::Value = serde_json::from_slice(&exact.stdout).expect("rg JSON");
    assert_eq!(exact_json["total_results"].as_u64().unwrap(), 1);
    let result = &exact_json["results"][0];
    assert_eq!(result["event_id"].as_str().unwrap(), codex_event_id);
    assert!(
        result["citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://event/")
    );

    let special = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["rg", "ERROR[abc]*"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("rg special");
    assert!(
        special.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&special.stderr)
    );
    let special_json: serde_json::Value =
        serde_json::from_slice(&special.stdout).expect("rg special JSON");
    assert_eq!(
        special_json["total_results"].as_u64().unwrap(),
        1,
        "special characters are literal, not regex"
    );

    let scoped = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["--source", "codex", "rg", "PANIC", "--ignore-case"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("rg scoped");
    assert!(
        scoped.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&scoped.stderr)
    );
    let scoped_json: serde_json::Value =
        serde_json::from_slice(&scoped.stdout).expect("rg scoped JSON");
    assert_eq!(scoped_json["total_results"].as_u64().unwrap(), 1);
    assert!(
        scoped_json["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|result| result["source"] == "codex")
    );

    let line = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["rg", "panic at src/main.rs:42", "--line"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("rg line");
    assert!(
        line.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&line.stderr)
    );
    let line_stdout = String::from_utf8(line.stdout).expect("line stdout utf8");
    assert!(line_stdout.contains("mmr://event/"));
    assert!(line_stdout.contains("panic at src/main.rs:42"));
    let columns = line_stdout
        .lines()
        .next()
        .expect("line result")
        .split('\t')
        .collect::<Vec<_>>();
    assert_eq!(columns.len(), 4);
    assert!(columns[0].starts_with("mmr://event/"));
    assert_eq!(columns[1], "1");

    let search_line = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["search", "decision", "--line"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("search line rejection");
    assert!(!search_line.status.success(), "search --line should fail");
    assert!(
        String::from_utf8_lossy(&search_line.stderr).contains("--line is only supported"),
        "stderr={}",
        String::from_utf8_lossy(&search_line.stderr)
    );
}

#[test]
fn search_cli_contract_is_implemented() {
    let (_tmp, home, data_home, project, _, note_event_id) = seed_search_fixture();

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["search", "decision", "--role", "user", "--session", "notes"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("search decision");
    assert!(
        search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&search.stdout).expect("search JSON");
    assert_eq!(json["query"], "decision");
    assert_eq!(json["total_results"].as_u64().unwrap(), 1);
    let result = &json["results"][0];
    assert_eq!(result["event_id"].as_str().unwrap(), note_event_id);
    assert_eq!(result["role"], "user");
    assert_eq!(result["event_type"], "note");
    assert!(result["snippet"].as_str().unwrap().contains("decision"));
    assert!(result.get("raw_local_ref").is_none());

    let project_scoped = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "search",
            "decision",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("project search");
    assert!(
        project_scoped.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&project_scoped.stderr)
    );
    let project_json: serde_json::Value =
        serde_json::from_slice(&project_scoped.stdout).expect("project search JSON");
    assert_eq!(project_json["total_results"].as_u64().unwrap(), 1);
}

#[test]
#[ignore = "pending NHL-282: implement summary command"]
fn summary_cli_contract_is_implemented() {
    pending_contract(
        "NHL-282",
        "mmr summary mirrors remember selection/provider semantics and keeps remember as a compatibility alias",
    );
}

#[test]
#[ignore = "pending NHL-279: implement dream command"]
fn dream_cli_contract_is_implemented() {
    pending_contract(
        "NHL-279",
        "mmr dream analyzes the current project and writes learned memory only with valid evidence refs",
    );
}

#[test]
fn schema_validation_contract_is_implemented() {
    let store = Store::open_in_memory().expect("store");
    assert_eq!(
        store.schema_version().expect("schema"),
        LATEST_SCHEMA_VERSION
    );

    let tables = store.schema_table_names().expect("tables");
    for table in NHL_269_REQUIRED_TABLES {
        assert!(
            tables.iter().any(|name| name == table),
            "missing NHL-269 table {table}"
        );
    }
}

#[test]
fn migration_replay_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db_path = tmp.path().join("mmr.db");

    let first = Store::open(&db_path).expect("first migration replay");
    assert_eq!(
        first.schema_version().expect("schema"),
        LATEST_SCHEMA_VERSION
    );
    drop(first);

    let second = Store::open(&db_path).expect("idempotent migration replay");
    assert_eq!(
        second.schema_version().expect("schema"),
        LATEST_SCHEMA_VERSION
    );
}

#[test]
fn redaction_policy_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let link = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "__db-info",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("link project");
    assert!(
        link.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&link.stderr)
    );

    let mut store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let mut adversarial_secret_ids = Vec::new();
    for (name, path, contents) in [
        (
            "tool_output_fake_secret",
            std::path::Path::new("tests/fixtures/memory_fabric/tool_output_fake_secret.jsonl"),
            include_str!("fixtures/memory_fabric/tool_output_fake_secret.jsonl"),
        ),
        (
            "pii_heavy_sample",
            std::path::Path::new("tests/fixtures/memory_fabric/pii_heavy_sample.jsonl"),
            include_str!("fixtures/memory_fabric/pii_heavy_sample.jsonl"),
        ),
    ] {
        let batch = parse_fixture_jsonl("fixture", "fixture-jsonl-v1", name, path, contents)
            .expect("parse redaction fixture");
        for event in batch.events {
            store
                .insert_event_with_search_document(&project_record.id, &event.into_store_event())
                .expect("insert fixture event");
        }
    }

    for (idx, content) in [
        "password=hunter2",
        r#"{"db_password":"hunter2"}"#,
        "api_key: short-secret",
    ]
    .into_iter()
    .enumerate()
    {
        let event = mmr::store::NewEvent::new(
            "codex",
            "redaction-adversarial",
            "tool_output",
            "tool",
            format!("2026-05-24T10:00:0{idx}Z"),
            content,
            "test-v1",
        )
        .with_source_event_id(format!("adversarial-secret-{idx}"));
        let (inserted, _) = store
            .insert_event_with_search_document(&project_record.id, &event)
            .expect("insert adversarial secret event");
        adversarial_secret_ids.push(inserted.id);
    }

    drop(store);

    let scan = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["redact", "scan"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("redact scan");
    assert!(
        scan.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let scan_json: serde_json::Value =
        serde_json::from_slice(&scan.stdout).expect("scan stdout JSON");
    assert_eq!(scan_json["events_scanned"].as_u64().unwrap(), 5);
    assert_eq!(scan_json["blocked"].as_u64().unwrap(), 4);
    assert_eq!(scan_json["passed"].as_u64().unwrap(), 1);
    assert_eq!(scan_json["pii_coverage"]["status"], "degraded");

    let blocked_event = scan_json["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["status"] == "blocked")
        .expect("blocked event");
    let blocked_event_id = blocked_event["event_id"].as_str().unwrap();
    assert!(
        blocked_event["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind == "secret")
    );

    let explain = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["redact", "explain", blocked_event_id])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("redact explain");
    assert!(
        explain.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let explain_json: serde_json::Value =
        serde_json::from_slice(&explain.stdout).expect("explain stdout JSON");
    assert_eq!(explain_json["status"], "blocked");
    assert!(
        explain_json["blocking_findings"].as_i64().unwrap() >= 1,
        "blocked event should have a blocking finding"
    );
    assert!(
        !explain_json["redacted_text"]
            .as_str()
            .unwrap()
            .contains("sk-test"),
        "explain output must not reveal raw fake secret"
    );
    assert!(explain.stderr.is_empty());

    let dry_run = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["sync", "--dry-run"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("sync dry-run");
    assert!(
        dry_run.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let dry_run_stdout = String::from_utf8(dry_run.stdout).expect("utf8 dry-run stdout");
    assert!(!dry_run_stdout.contains("sk-test"));
    assert!(!dry_run_stdout.contains("hunter2"));
    assert!(!dry_run_stdout.contains("short-secret"));
    assert!(dry_run.stderr.is_empty());
    let dry_run_json: serde_json::Value =
        serde_json::from_str(&dry_run_stdout).expect("dry-run stdout JSON");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["blocked_events"].as_u64().unwrap(), 5);
    assert_eq!(dry_run_json["syncable_events"].as_u64().unwrap(), 0);
    assert!(
        dry_run_json["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["payload_preview"].is_null()),
        "degraded policy should not print payload previews"
    );
    assert!(
        dry_run_json["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["blocked_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason.as_str().unwrap().contains("deterministic secret")))
    );

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    for event_id in adversarial_secret_ids {
        let run = store
            .latest_redaction_run_for_event(&event_id)
            .expect("read adversarial run")
            .expect("adversarial run");
        assert_eq!(run.status, "blocked");
    }
    drop(store);

    let mut store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let cursor_event = mmr::store::NewEvent::new(
        "cursor",
        "cursor-redaction",
        "message",
        "user",
        "2026-05-24T10:01:00Z",
        "cursor source should be filtered away from codex dry-run",
        "test-v1",
    )
    .with_source_event_id("cursor-filter-event");
    store
        .insert_event_with_search_document(&project_record.id, &cursor_event)
        .expect("insert cursor event after persisted scan");
    let before_dry_run = store
        .events_for_project(&project_record.id, Some("cursor"), Some("cursor-redaction"))
        .expect("cursor events before source-filtered dry-run");
    assert_eq!(before_dry_run[0].sync_status, "local_only");
    drop(store);

    let codex_dry_run = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["--source", "codex", "sync", "--dry-run"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("codex sync dry-run");
    assert!(
        codex_dry_run.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&codex_dry_run.stderr)
    );
    let codex_dry_run_json: serde_json::Value =
        serde_json::from_slice(&codex_dry_run.stdout).expect("codex dry-run JSON");
    assert!(
        codex_dry_run_json["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["source"] == "codex")
    );

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let after_dry_run = store
        .events_for_project(&project_record.id, Some("cursor"), Some("cursor-redaction"))
        .expect("cursor events after source-filtered dry-run");
    assert_eq!(
        after_dry_run[0].sync_status, "local_only",
        "sync --dry-run must not mutate persistent redaction state"
    );
}

#[test]
fn search_document_contract_is_implemented() {
    let (_tmp, home, data_home, project, codex_event_id, _) = seed_search_fixture();

    let output_dir = tempfile::tempdir().expect("export output dir");
    let export = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "export",
            "--format",
            "tree",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--output-dir",
            output_dir.path().to_str().expect("output path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("export tree");
    assert!(
        export.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&export.stderr)
    );
    let export_json: serde_json::Value =
        serde_json::from_slice(&export.stdout).expect("export tree JSON");
    assert!(export_json["total_files"].as_u64().unwrap() >= 3);
    let files = export_json["files"].as_array().unwrap();
    assert!(files.iter().any(|file| file["event_id"] == codex_event_id));
    let run_dir = std::path::PathBuf::from(export_json["output_dir"].as_str().unwrap());
    assert_eq!(run_dir.parent().unwrap(), output_dir.path());
    assert!(
        run_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("mmr-tree-")
    );
    let searchable_text = files
        .iter()
        .map(|file| {
            std::fs::read_to_string(file["path"].as_str().unwrap()).expect("read exported file")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(searchable_text.contains("panic at src/main.rs:42"));
    assert!(!searchable_text.contains("raw_local_ref"));
    assert!(!searchable_text.contains("tests/fixtures/search/codex.jsonl"));

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let search_doc = store
        .search_document_by_event(&codex_event_id)
        .expect("generated search document");
    assert!(search_doc.document_text.contains("panic at src/main.rs:42"));
}

#[test]
#[ignore = "pending NHL-282: implement summary command contract"]
fn summary_generation_contract_is_implemented() {
    pending_contract(
        "NHL-282",
        "route summary through the existing remember runner semantics while keeping remember as a compatibility alias",
    );
}

#[test]
#[ignore = "pending NHL-278/NHL-279: implement dream assimilation validation"]
fn dream_assimilation_contract_is_implemented() {
    pending_contract(
        "NHL-278/NHL-279",
        "validate structured dream output and write learned memory only when every evidence ref resolves",
    );
}

#[test]
#[ignore = "pending NHL-277: implement sync manifest generation"]
fn sync_manifest_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "generate replayable redacted sync manifests for github:<user>/mmr-store hydration",
    );
}

fn pending_contract(ticket: &str, behavior: &str) -> ! {
    panic!("{ticket} pending contract: {behavior}. Remove #[ignore] when implemented.");
}

fn seed_search_fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    String,
    String,
) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().to_path_buf();
    let home = root.join("home");
    let data_home = root.join("data");
    let project = root.join("plain-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let link = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "__db-info",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("link project");
    assert!(
        link.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&link.stderr)
    );

    let mut store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let codex = mmr::store::NewEvent::new(
        "codex",
        "search-codex",
        "tool_output",
        "tool",
        "2026-05-24T11:00:00Z",
        "panic at src/main.rs:42\nERROR[abc]* literal marker",
        "search-test-v1",
    )
    .with_source_event_id("search-codex-1")
    .with_raw_local_ref("tests/fixtures/search/codex.jsonl:1");
    let codex_event = store
        .insert_event(&project_record.id, &codex)
        .expect("insert codex event without search doc");

    let note = mmr::store::NewEvent::new(
        "note",
        "notes",
        "note",
        "user",
        "2026-05-24T11:01:00Z",
        "decision: exact search should stay lexical",
        "note-v1",
    )
    .with_source_event_id("search-note-1");
    let (note_event, _) = store
        .insert_event_with_search_document(&project_record.id, &note)
        .expect("insert note event");

    let cursor = mmr::store::NewEvent::new(
        "cursor",
        "search-cursor",
        "message",
        "assistant",
        "2026-05-24T11:02:00Z",
        "panic at cursor should be filtered by source",
        "search-test-v1",
    )
    .with_source_event_id("search-cursor-1");
    store
        .insert_event_with_search_document(&project_record.id, &cursor)
        .expect("insert cursor event");

    (tmp, home, data_home, project, codex_event.id, note_event.id)
}
