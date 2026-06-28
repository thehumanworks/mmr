use mmr::capture::{
    ClaudeAdapter, CodexAdapter, CursorAdapter, EventBoundary, FileWatcher, WatchState,
    event_hash_set, parse_claude_jsonl, parse_codex_jsonl, parse_cursor_jsonl, parse_fixture_jsonl,
};
use mmr::dream::{
    DreamEvidenceMode, DreamObservationStatus, DreamRunner, DreamRunnerConfig, EvidenceAccess,
    MockDreamRunner, build_evidence_bundle, build_evidence_request,
};
use mmr::store::{LATEST_SCHEMA_VERSION, NewDreamCandidate, NewLearnedMemory, Store};
#[allow(dead_code)]
mod common;
use common::RetrieveContractFixture;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn loopback_bind_available() -> bool {
    std::net::TcpListener::bind("127.0.0.1:0").is_ok()
}

const FIXTURES: &[(&str, &str)] = &[
    (
        "codex_session",
        include_str!("fixtures/memory_fabric/codex_session.jsonl"),
    ),
    (
        "codex_rollout_session",
        include_str!("fixtures/memory_fabric/codex_rollout_session.jsonl"),
    ),
    (
        "claude_like_session",
        include_str!("fixtures/memory_fabric/claude_like_session.jsonl"),
    ),
    (
        "claude_code_session",
        include_str!("fixtures/memory_fabric/claude_code_session.jsonl"),
    ),
    (
        "cursor_agent_session",
        include_str!("fixtures/memory_fabric/cursor_agent_session.jsonl"),
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
const MALFORMED_CODEX_ROLLOUT: &str =
    include_str!("fixtures/memory_fabric/codex_rollout_malformed_tail.jsonl");
const MALFORMED_CLAUDE_CODE: &str =
    include_str!("fixtures/memory_fabric/claude_code_malformed_tail.jsonl");
const MALFORMED_CURSOR_AGENT: &str =
    include_str!("fixtures/memory_fabric/cursor_agent_malformed_tail.jsonl");

const MVP_NON_GOAL_COMMANDS: &[&str] = &[
    "store",
    "learn",
    "candidates",
    "knowledge",
    "promote",
    "reject",
];

const RELEASE_NOTE_SECRET: &str = "sk-test-000000000000000000000000000000000000000000000000";
const RELEASE_NOTE_EMAIL: &str = "person@example.com";

fn encode_claude_project_name(path: &std::path::Path) -> String {
    let path = path.to_str().expect("project path UTF-8");
    if path == "/" {
        "-".to_string()
    } else {
        format!("-{}", path.trim_start_matches('/').replace('/', "-"))
    }
}

fn encode_cursor_project_name(path: &std::path::Path) -> String {
    let path = path.to_str().expect("project path UTF-8");
    if path == "/" {
        String::new()
    } else {
        path.trim_start_matches('/').replace('/', "-")
    }
}

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
    assert_eq!(
        json["schema_version"].as_i64().unwrap(),
        LATEST_SCHEMA_VERSION
    );
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
fn codex_importer_contract_is_implemented() {
    let path = std::path::Path::new("tests/fixtures/memory_fabric/codex_rollout_session.jsonl");
    let batch = parse_codex_jsonl(
        "fallback-codex",
        path,
        include_str!("fixtures/memory_fabric/codex_rollout_session.jsonl"),
    )
    .expect("parse codex rollout");

    assert_eq!(batch.source, "codex");
    assert_eq!(batch.parser_version, CodexAdapter::PARSER_VERSION);
    assert_eq!(batch.events.len(), 6);
    assert_eq!(event_hash_set(&batch.events).len(), 6);
    assert!(batch.warnings.is_empty());
    assert!(batch.events.iter().all(|event| {
        event.source_session_id == "codex-rollout-1"
            && event.parser_version == CodexAdapter::PARSER_VERSION
            && event.raw_local_ref.contains("codex_rollout_session.jsonl")
    }));
    assert_eq!(batch.events[0].boundary, EventBoundary::SessionStart);
    assert_eq!(batch.events[1].boundary, EventBoundary::UserTurn);
    assert_eq!(batch.events[2].boundary, EventBoundary::AssistantTurn);
    assert_eq!(batch.events[3].boundary, EventBoundary::ToolCall);
    assert_eq!(batch.events[4].boundary, EventBoundary::ToolResult);
    assert_eq!(batch.events[5].boundary, EventBoundary::Compaction);
    assert!(batch.events[4].content_text.contains("CodexAdapter"));
    assert!(
        !batch.events[0]
            .content_text
            .contains("/Users/test/memory-fabric")
    );
    assert!(
        batch.cursor_updates[0]
            .cursor_value
            .starts_with("line:6;bytes:")
    );

    let malformed = parse_codex_jsonl(
        "fallback-codex",
        std::path::Path::new("tests/fixtures/memory_fabric/codex_rollout_malformed_tail.jsonl"),
        MALFORMED_CODEX_ROLLOUT,
    )
    .expect("parse malformed codex rollout");
    assert_eq!(malformed.events.len(), 3);
    assert_eq!(malformed.warnings.len(), 1);
    assert!(
        malformed.warnings[0]
            .message
            .contains("skipped malformed Codex JSONL row")
    );

    let partial_tail = "{\"type\":\"session_meta\",\"timestamp\":\"2026-05-24T12:00:00Z\",\"payload\":{\"id\":\"codex-partial\",\"cwd\":\"/Users/test/memory-fabric\",\"model_provider\":\"openai\"}}\n{\"type\":\"event_msg\",\"timestamp\":\"2026-05-24T12:00:05Z\",\"payload\":{\"type\":\"user_message\",\"message\":\"Keep complete rows only.\"}}\n{\"type\":\"response_item\",\"timestamp\":\"2026-05-24T12:00:10Z\",\"payload\":";
    let partial = parse_codex_jsonl(
        "fallback-codex",
        std::path::Path::new("tests/fixtures/memory_fabric/codex_rollout_partial_tail.jsonl"),
        partial_tail,
    )
    .expect("parse partial codex rollout");
    assert_eq!(partial.events.len(), 2);
    assert!(partial.warnings.is_empty());
    let consumed_bytes = partial_tail.rfind('\n').expect("complete row newline") + 1;
    assert_eq!(
        partial.cursor_updates[0].cursor_value,
        format!("line:2;bytes:{consumed_bytes}")
    );
}

#[test]
fn codex_active_session_watcher_uses_complete_rows_only() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("active-codex.jsonl");
    std::fs::write(
        &path,
        r#"{"type":"session_meta","timestamp":"2026-05-24T12:00:00Z","payload":{"id":"active-codex","cwd":"/Users/test/memory-fabric","model_provider":"openai"}}
{"type":"event_msg","timestamp":"2026-05-24T12:00:05Z","payload":{"type":"user_message","message":"Watch complete rows."}}
{"type":"response_item","timestamp":"2026-05-24T12:00:10Z","payload":{"role":"assistant","content":[{"type":"output_text","text":"#,
    )
    .expect("write partial active codex");

    let delta = FileWatcher::read_delta(&WatchState {
        path: path.clone(),
        offset: 0,
        fingerprint: None,
    })
    .expect("partial delta");
    assert!(delta.partial_tail);
    let batch = parse_codex_jsonl(
        "active-codex",
        &path,
        &String::from_utf8(delta.bytes).expect("delta UTF-8"),
    )
    .expect("parse complete codex rows");
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.events[0].boundary, EventBoundary::SessionStart);
    assert_eq!(batch.events[1].boundary, EventBoundary::UserTurn);

    std::fs::write(
        &path,
        r#"{"type":"session_meta","timestamp":"2026-05-24T12:00:00Z","payload":{"id":"active-codex","cwd":"/Users/test/memory-fabric","model_provider":"openai"}}
{"type":"event_msg","timestamp":"2026-05-24T12:00:05Z","payload":{"type":"user_message","message":"Watch complete rows."}}
{"type":"response_item","timestamp":"2026-05-24T12:00:10Z","payload":{"role":"assistant","content":[{"type":"output_text","text":"Assistant row is complete."}]}}
"#,
    )
    .expect("complete active codex");
    let next = FileWatcher::read_delta(&WatchState {
        path: path.clone(),
        offset: delta.new_offset,
        fingerprint: Some(delta.new_fingerprint),
    })
    .expect("completed delta");
    assert!(!next.partial_tail);
    let next_batch = parse_codex_jsonl(
        "active-codex",
        &path,
        &String::from_utf8(next.bytes).expect("next delta UTF-8"),
    )
    .expect("parse completed assistant row");
    assert_eq!(next_batch.events.len(), 1);
    assert_eq!(next_batch.events[0].boundary, EventBoundary::AssistantTurn);
}

#[test]
fn claude_importer_contract_is_implemented() {
    let path = std::path::Path::new("tests/fixtures/memory_fabric/claude_code_session.jsonl");
    let batch = parse_claude_jsonl(
        "fallback-claude",
        path,
        include_str!("fixtures/memory_fabric/claude_code_session.jsonl"),
    )
    .expect("parse claude code rollout");

    assert_eq!(batch.source, "claude");
    assert_eq!(batch.parser_version, ClaudeAdapter::PARSER_VERSION);
    assert_eq!(batch.events.len(), 7);
    assert_eq!(event_hash_set(&batch.events).len(), 7);
    assert!(batch.warnings.is_empty());
    assert!(batch.events.iter().all(|event| {
        event.source_session_id == "claude-code-rollout-1"
            && event.parser_version == ClaudeAdapter::PARSER_VERSION
            && event.raw_local_ref.contains("claude_code_session.jsonl")
            && !event.content_text.contains("/Users/test/memory-fabric")
    }));
    assert_eq!(batch.events[0].boundary, EventBoundary::UserTurn);
    assert_eq!(batch.events[1].boundary, EventBoundary::AssistantTurn);
    assert_eq!(batch.events[2].boundary, EventBoundary::Compaction);
    assert_eq!(batch.events[3].boundary, EventBoundary::ToolCall);
    assert_eq!(batch.events[4].boundary, EventBoundary::ToolResult);
    assert_eq!(batch.events[5].boundary, EventBoundary::AssistantTurn);
    assert_eq!(batch.events[6].boundary, EventBoundary::SessionEnd);
    assert!(batch.events[3].content_text.contains("TodoWrite"));
    assert!(
        batch.events[4]
            .content_text
            .contains("Todos updated successfully.")
    );
    assert!(
        batch.cursor_updates[0]
            .cursor_value
            .starts_with("line:5;bytes:")
    );

    let malformed = parse_claude_jsonl(
        "fallback-claude",
        std::path::Path::new("tests/fixtures/memory_fabric/claude_code_malformed_tail.jsonl"),
        MALFORMED_CLAUDE_CODE,
    )
    .expect("parse malformed claude code rollout");
    assert_eq!(malformed.events.len(), 2);
    assert_eq!(malformed.warnings.len(), 1);
    assert!(
        malformed.warnings[0]
            .message
            .contains("skipped malformed Claude JSONL row")
    );

    let drift_rows = r#"{"type":"file-history-snapshot","sessionId":"claude-drift","cwd":"/Users/test/memory-fabric","gitBranch":"main","files":[{"path":"/Users/test/memory-fabric/src/main.rs"}],"timestamp":"2026-05-24T13:25:00Z","uuid":"snapshot-1"}
{"type":"queue-operation","operation":"enqueue","sessionId":"claude-drift","content":"Queued Claude prompt.","timestamp":"2026-05-24T13:25:01Z","uuid":"queue-1","cwd":"/Users/test/memory-fabric"}
{"type":"attachment","sessionId":"claude-drift","payload":{"path":"/Users/test/memory-fabric/private.txt"},"timestamp":"2026-05-24T13:25:02Z","uuid":"attachment-1","cwd":"/Users/test/memory-fabric"}
{"type":"weird-row","sessionId":"claude-drift","payload":{"path":"/Users/test/memory-fabric/raw.json"},"timestamp":"2026-05-24T13:25:03Z","uuid":"weird-1","cwd":"/Users/test/memory-fabric"}
{"type":"user","sessionId":"claude-drift","message":{"role":"user"},"timestamp":"2026-05-24T13:25:04Z","uuid":"missing-content-1","cwd":"/Users/test/memory-fabric"}
"#;
    let drift = parse_claude_jsonl(
        "fallback-claude",
        std::path::Path::new("tests/fixtures/memory_fabric/claude_code_drift.jsonl"),
        drift_rows,
    )
    .expect("parse claude drift rows");
    assert_eq!(drift.events.len(), 5);
    assert_eq!(drift.events[0].boundary, EventBoundary::UnknownRawEvent);
    assert_eq!(drift.events[1].boundary, EventBoundary::UserTurn);
    assert_eq!(drift.events[2].boundary, EventBoundary::UnknownRawEvent);
    assert_eq!(drift.events[3].boundary, EventBoundary::UnknownRawEvent);
    assert_eq!(drift.events[4].boundary, EventBoundary::UnknownRawEvent);
    assert!(drift.events.iter().all(|event| {
        !event.content_text.contains("/Users/test/memory-fabric")
            && !event.content_text.contains("gitBranch")
    }));
    assert_eq!(drift.events[1].content_text, "Queued Claude prompt.");
    assert!(drift.events[4].content_text.contains("missing content"));

    let partial_tail = "{\"type\":\"user\",\"sessionId\":\"claude-partial\",\"message\":{\"role\":\"user\",\"content\":\"Keep complete Claude rows.\"},\"timestamp\":\"2026-05-24T13:30:00Z\",\"uuid\":\"u-claude-partial\",\"cwd\":\"/Users/test/memory-fabric\"}\n{\"type\":\"assistant\",\"sessionId\":\"claude-partial\",\"message\":";
    let partial = parse_claude_jsonl(
        "fallback-claude",
        std::path::Path::new("tests/fixtures/memory_fabric/claude_code_partial_tail.jsonl"),
        partial_tail,
    )
    .expect("parse partial claude code rollout");
    assert_eq!(partial.events.len(), 1);
    assert!(partial.warnings.is_empty());
    let consumed_bytes = partial_tail.rfind('\n').expect("complete row newline") + 1;
    assert_eq!(
        partial.cursor_updates[0].cursor_value,
        format!("line:1;bytes:{consumed_bytes}")
    );

    let long_output = "x".repeat(ClaudeAdapter::TOOL_RESULT_MAX_CHARS + 500);
    let large_line = format!(
        "{{\"type\":\"user\",\"sessionId\":\"claude-large\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_large\",\"content\":\"{long_output}\"}}]}},\"timestamp\":\"2026-05-24T13:40:00Z\",\"uuid\":\"tool-result-large\",\"cwd\":\"/Users/test/memory-fabric\"}}\n"
    );
    let large = parse_claude_jsonl(
        "fallback-claude",
        std::path::Path::new("tests/fixtures/memory_fabric/claude_code_large.jsonl"),
        &large_line,
    )
    .expect("parse large claude tool result");
    assert_eq!(large.events.len(), 1);
    assert_eq!(large.events[0].boundary, EventBoundary::ToolResult);
    assert!(large.events[0].content_text.contains("omitted_chars=500"));
    assert!(
        large.events[0]
            .content_text
            .contains("full_content_hash=sha256:")
    );
    assert!(large.events[0].content_text.len() < long_output.len());
}

#[test]
fn claude_active_session_watcher_uses_complete_rows_only() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("active-claude.jsonl");
    std::fs::write(
        &path,
        r#"{"type":"user","sessionId":"active-claude","message":{"role":"user","content":"Watch Claude rows."},"timestamp":"2026-05-24T13:50:00Z","uuid":"u-active-claude","cwd":"/Users/test/memory-fabric"}
{"type":"assistant","sessionId":"active-claude","message":{"role":"assistant","content":"#,
    )
    .expect("write partial active claude");

    let delta = FileWatcher::read_delta(&WatchState {
        path: path.clone(),
        offset: 0,
        fingerprint: None,
    })
    .expect("partial delta");
    assert!(delta.partial_tail);
    let batch = parse_claude_jsonl(
        "active-claude",
        &path,
        &String::from_utf8(delta.bytes).expect("delta UTF-8"),
    )
    .expect("parse complete claude rows");
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].boundary, EventBoundary::UserTurn);

    std::fs::write(
        &path,
        r#"{"type":"user","sessionId":"active-claude","message":{"role":"user","content":"Watch Claude rows."},"timestamp":"2026-05-24T13:50:00Z","uuid":"u-active-claude","cwd":"/Users/test/memory-fabric"}
{"type":"assistant","sessionId":"active-claude","message":{"role":"assistant","content":"Assistant row is complete."},"timestamp":"2026-05-24T13:50:05Z","uuid":"a-active-claude","cwd":"/Users/test/memory-fabric"}
"#,
    )
    .expect("complete active claude");
    let next = FileWatcher::read_delta(&WatchState {
        path: path.clone(),
        offset: delta.new_offset,
        fingerprint: Some(delta.new_fingerprint),
    })
    .expect("completed delta");
    assert!(!next.partial_tail);
    let next_batch = parse_claude_jsonl(
        "active-claude",
        &path,
        &String::from_utf8(next.bytes).expect("next delta UTF-8"),
    )
    .expect("parse completed claude row");
    assert_eq!(next_batch.events.len(), 1);
    assert_eq!(next_batch.events[0].boundary, EventBoundary::AssistantTurn);
}

#[test]
fn cursor_importer_contract_is_implemented() {
    let path = std::path::Path::new("tests/fixtures/memory_fabric/cursor_agent_session.jsonl");
    let batch = parse_cursor_jsonl(
        "cursor-agent-rollout-1",
        path,
        include_str!("fixtures/memory_fabric/cursor_agent_session.jsonl"),
    )
    .expect("parse cursor agent rollout");

    assert_eq!(batch.source, "cursor");
    assert_eq!(batch.parser_version, CursorAdapter::PARSER_VERSION);
    assert_eq!(batch.events.len(), 6);
    assert_eq!(event_hash_set(&batch.events).len(), 6);
    assert!(batch.warnings.is_empty());
    assert!(batch.events.iter().all(|event| {
        event.source_session_id == "cursor-agent-rollout-1"
            && event.parser_version == CursorAdapter::PARSER_VERSION
            && event.raw_local_ref.contains("cursor_agent_session.jsonl")
    }));
    assert_eq!(batch.events[0].boundary, EventBoundary::UserTurn);
    assert_eq!(batch.events[1].boundary, EventBoundary::AssistantTurn);
    assert_eq!(batch.events[2].boundary, EventBoundary::Compaction);
    assert_eq!(batch.events[3].boundary, EventBoundary::ToolCall);
    assert_eq!(batch.events[4].boundary, EventBoundary::ToolResult);
    assert_eq!(batch.events[5].boundary, EventBoundary::SessionEnd);
    assert!(batch.events[3].content_text.contains("shell"));
    assert!(
        !batch.events[3]
            .content_text
            .contains("/Users/test/memory-fabric")
    );
    assert!(batch.events[3].content_text.contains("[LOCAL_PATH]"));
    assert!(batch.events[4].content_text.contains("CursorAdapter"));
    assert!(
        batch.cursor_updates[0]
            .cursor_value
            .starts_with("line:3;bytes:")
    );

    let malformed = parse_cursor_jsonl(
        "cursor-agent-bad",
        std::path::Path::new("tests/fixtures/memory_fabric/cursor_agent_malformed_tail.jsonl"),
        MALFORMED_CURSOR_AGENT,
    )
    .expect("parse malformed cursor rollout");
    assert_eq!(malformed.events.len(), 2);
    assert_eq!(malformed.warnings.len(), 1);
    assert!(
        malformed.warnings[0]
            .message
            .contains("skipped malformed Cursor JSONL row")
    );

    let partial_tail = "{\"role\":\"user\",\"timestamp\":\"2026-05-24T14:20:00Z\",\"id\":\"u-cursor-partial\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Keep complete Cursor rows.\"}]}}\n{\"role\":\"assistant\",\"timestamp\":\"2026-05-24T14:20:05Z\",\"message\":";
    let partial = parse_cursor_jsonl(
        "cursor-agent-partial",
        std::path::Path::new("tests/fixtures/memory_fabric/cursor_agent_partial_tail.jsonl"),
        partial_tail,
    )
    .expect("parse partial cursor rollout");
    assert_eq!(partial.events.len(), 1);
    assert!(partial.warnings.is_empty());
    let consumed_bytes = partial_tail.rfind('\n').expect("complete row newline") + 1;
    assert_eq!(
        partial.cursor_updates[0].cursor_value,
        format!("line:1;bytes:{consumed_bytes}")
    );

    let flat = parse_cursor_jsonl(
        "cursor-flat",
        std::path::Path::new("tests/fixtures/memory_fabric/cursor_agent_flat.jsonl"),
        r#"{"role":"user","content":"Flat Cursor prompt.","timestamp":"2026-05-24T14:25:00Z","id":"flat-u"}
{"role":"assistant","content":{"text":"Flat Cursor answer."},"timestamp":"2026-05-24T14:25:05Z","id":"flat-a"}
{"role":"mystery","payload":{"cwd":"/Users/test/memory-fabric","path":"/Users/test/memory-fabric/file.txt"},"timestamp":"2026-05-24T14:25:10Z","id":"flat-unknown"}
"#,
    )
    .expect("parse flat cursor rollout");
    assert_eq!(flat.events.len(), 3);
    assert_eq!(flat.events[0].boundary, EventBoundary::UserTurn);
    assert_eq!(flat.events[1].boundary, EventBoundary::AssistantTurn);
    assert_eq!(flat.events[2].boundary, EventBoundary::UnknownRawEvent);
    assert!(
        flat.events
            .iter()
            .all(|event| !event.content_text.contains("/Users/test/memory-fabric"))
    );
}

#[test]
fn cursor_active_session_watcher_uses_complete_rows_only() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("active-cursor.jsonl");
    std::fs::write(
        &path,
        r#"{"role":"user","timestamp":"2026-05-24T14:30:00Z","id":"u-active-cursor","message":{"content":[{"type":"text","text":"Watch Cursor rows."}]}}
{"role":"assistant","timestamp":"2026-05-24T14:30:05Z","message":{"content":[{"type":"text","text":"#,
    )
    .expect("write partial active cursor");

    let delta = FileWatcher::read_delta(&WatchState {
        path: path.clone(),
        offset: 0,
        fingerprint: None,
    })
    .expect("partial delta");
    assert!(delta.partial_tail);
    let batch = parse_cursor_jsonl(
        "active-cursor",
        &path,
        &String::from_utf8(delta.bytes).expect("delta UTF-8"),
    )
    .expect("parse complete cursor rows");
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].boundary, EventBoundary::UserTurn);

    std::fs::write(
        &path,
        r#"{"role":"user","timestamp":"2026-05-24T14:30:00Z","id":"u-active-cursor","message":{"content":[{"type":"text","text":"Watch Cursor rows."}]}}
{"role":"assistant","timestamp":"2026-05-24T14:30:05Z","id":"a-active-cursor","message":{"content":[{"type":"text","text":"Assistant row is complete."}]}}
"#,
    )
    .expect("complete active cursor");
    let next = FileWatcher::read_delta(&WatchState {
        path: path.clone(),
        offset: delta.new_offset,
        fingerprint: Some(delta.new_fingerprint),
    })
    .expect("completed delta");
    assert!(!next.partial_tail);
    let next_batch = parse_cursor_jsonl(
        "active-cursor",
        &path,
        &String::from_utf8(next.bytes).expect("next delta UTF-8"),
    )
    .expect("parse completed cursor row");
    assert_eq!(next_batch.events.len(), 1);
    assert_eq!(next_batch.events[0].boundary, EventBoundary::AssistantTurn);
}

#[test]
fn link_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let link = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("link project");
    assert!(
        link.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&link.stderr)
    );
    let link_json: serde_json::Value =
        serde_json::from_slice(&link.stdout).expect("link stdout JSON");
    let link_stdout = String::from_utf8_lossy(&link.stdout);
    for local_path in [&home, &data_home, &project, &remote] {
        assert!(
            !link_stdout.contains(&local_path.to_string_lossy().to_string()),
            "link stdout should not expose local path {}",
            local_path.display()
        );
    }
    assert_eq!(link_json["command"], "init");
    assert_eq!(link_json["already_linked"], false);
    assert_eq!(link_json["status"]["linked"], true);
    assert_eq!(link_json["status"]["sync_status"], "synced");
    assert_eq!(
        link_json["remote"]["descriptor"],
        "github:fixture-user/mmr-store"
    );
    assert_eq!(link_json["sync"]["status"], "synced");
    assert!(remote.join("remote.json").exists());

    let relink = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("relink project");
    assert!(
        relink.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&relink.stderr)
    );
    let relink_json: serde_json::Value =
        serde_json::from_slice(&relink.stdout).expect("relink stdout JSON");
    assert_eq!(relink_json["already_linked"], true);
    assert_eq!(relink_json["sync"]["uploaded_events"].as_u64().unwrap(), 0);

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let manifests = store
        .sync_manifests_for_project(&project_record.id)
        .expect("manifests");
    assert_eq!(manifests.len(), 1);

    let conflict_remote = tmp.path().join("conflict-remote");
    std::fs::create_dir_all(&conflict_remote).expect("create conflict remote");
    std::fs::write(
        conflict_remote.join("remote.json"),
        r#"{"backend":"file-github","descriptor":"github:someone-else/mmr-store","repo":"mmr-store"}"#,
    )
    .expect("write conflict remote metadata");
    let conflict = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", tmp.path().join("conflict-data"))
        .env("MMR_FAKE_REMOTE_DIR", &conflict_remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("link conflict remote");
    assert!(!conflict.status.success());
    assert!(conflict.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&conflict.stderr).contains("remote payload conflict"),
        "stderr={}",
        String::from_utf8_lossy(&conflict.stderr)
    );
}

#[test]
fn sync_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg("init")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&project)
            .output()
            .expect("link before sync"),
    );
    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args([
                "note",
                "Email",
                "person@example.com",
                "about",
                "portable",
                "sync",
            ])
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .current_dir(&project)
            .output()
            .expect("add syncable note"),
    );
    let original_event_id = {
        let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
        let project_record = store
            .project_by_path(&project)
            .expect("project lookup")
            .expect("project");
        let events = store
            .events_for_project(&project_record.id, Some("note"), None)
            .expect("events before sync");
        assert_eq!(events.len(), 1);
        events[0].id.clone()
    };

    let sync = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("sync")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("sync project");
    assert!(
        sync.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let sync_stdout = String::from_utf8(sync.stdout).expect("sync stdout UTF-8");
    assert!(!sync_stdout.contains("person@example.com"));
    let sync_json: serde_json::Value =
        serde_json::from_str(&sync_stdout).expect("sync stdout JSON");
    assert_eq!(sync_json["status"], "synced");
    assert_eq!(sync_json["synced_events"].as_u64().unwrap(), 1);
    assert_eq!(sync_json["uploaded_events"].as_u64().unwrap(), 1);
    assert_eq!(sync_json["blocked_events"].as_u64().unwrap(), 0);
    assert_eq!(sync_json["pii_coverage"]["status"], "available");

    let remote_text = remote_file_text(&remote);
    assert!(!remote_text.contains("person@example.com"));
    assert!(!remote_text.contains(&original_event_id));
    assert!(remote_text.contains("[REDACTED:private_email]"));
    let remote_event_count = remote_event_file_count(&remote);
    assert_eq!(remote_event_count, 1);

    let resync = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("sync")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("sync project again");
    assert!(
        resync.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&resync.stderr)
    );
    let resync_json: serde_json::Value =
        serde_json::from_slice(&resync.stdout).expect("resync stdout JSON");
    assert_eq!(resync_json["status"], "synced");
    assert_eq!(resync_json["uploaded_events"].as_u64().unwrap(), 0);
    assert_eq!(remote_event_file_count(&remote), remote_event_count);

    let relink = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("relink after sync");
    assert!(
        relink.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&relink.stderr)
    );
    let relink_json: serde_json::Value =
        serde_json::from_slice(&relink.stdout).expect("relink stdout JSON");
    assert_eq!(relink_json["hydration"]["inserted_events"], 0);
    assert_eq!(relink_json["hydration"]["existing_events"], 1);
    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store after relink");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup after relink")
        .expect("project after relink");
    let events = store
        .events_for_project(&project_record.id, Some("note"), None)
        .expect("events after relink");
    assert_eq!(events.len(), 1);

    let remote_event_path = first_remote_event_file(&remote);
    let mut remote_event: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&remote_event_path).expect("read remote event JSON"),
    )
    .expect("remote event JSON");
    remote_event["content_text"] = serde_json::Value::String("tampered remote content".to_string());
    std::fs::write(
        &remote_event_path,
        serde_json::to_vec_pretty(&remote_event).expect("tampered remote event JSON"),
    )
    .expect("write tampered remote event");
    let tampered_sync = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("sync")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("sync tampered remote");
    assert!(!tampered_sync.status.success());
    assert!(tampered_sync.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&tampered_sync.stderr)
            .contains("remote event content_hash mismatch"),
        "stderr={}",
        String::from_utf8_lossy(&tampered_sync.stderr)
    );

    std::fs::remove_dir_all(&remote).expect("remove remote");
    let missing_remote_status = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("status")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("status with missing remote");
    assert!(
        missing_remote_status.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&missing_remote_status.stderr)
    );
    let missing_remote_status_json: serde_json::Value =
        serde_json::from_slice(&missing_remote_status.stdout).expect("missing remote status JSON");
    assert_eq!(
        missing_remote_status_json["status"]["sync_status"],
        "remote_unavailable"
    );
    assert_eq!(missing_remote_status_json["remote"]["available"], false);
}

#[test]
fn status_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg("init")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&project)
            .output()
            .expect("link before status"),
    );
    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(["note", "password=hunter2"])
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .current_dir(&project)
            .output()
            .expect("add blocked note"),
    );

    let sync = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("sync")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("sync blocked note");
    assert!(
        sync.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let sync_stdout = String::from_utf8(sync.stdout).expect("sync stdout UTF-8");
    assert!(!sync_stdout.contains("hunter2"));
    let sync_json: serde_json::Value =
        serde_json::from_str(&sync_stdout).expect("sync stdout JSON");
    assert_eq!(sync_json["status"], "blocked");
    assert_eq!(sync_json["blocked_events"].as_u64().unwrap(), 1);

    let status = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("status")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("status");
    assert!(
        status.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("status stdout JSON");
    assert_eq!(status_json["command"], "status");
    assert_eq!(status_json["status"]["linked"], true);
    assert_eq!(status_json["status"]["sync_status"], "blocked");
    assert_eq!(status_json["status"]["redaction"]["blocked"], 1);
    assert_eq!(status_json["status"]["source_counts"]["note"], 1);
    assert!(!remote_file_text(&remote).contains("hunter2"));

    let auth_failure = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", tmp.path().join("auth-data"))
        .env("MMR_FAKE_REMOTE_DIR", tmp.path().join("auth-remote"))
        .env("MMR_FAKE_REMOTE_AUTH", "fail")
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("link auth failure");
    assert!(
        auth_failure.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&auth_failure.stderr)
    );
    let auth_failure_json: serde_json::Value =
        serde_json::from_slice(&auth_failure.stdout).expect("auth failure link JSON");
    assert_eq!(auth_failure_json["sync"]["status"], "remote_pending");
    assert_eq!(auth_failure_json["remote"]["auth_status"], "failed");
    assert_eq!(
        auth_failure_json["status"]["sync_status"],
        "remote_unavailable"
    );
}

#[test]
fn status_diagnostics_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(home.join(".codex")).expect("create codex source root");
    std::fs::create_dir_all(&project).expect("create project");

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["--pretty", "status"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("MMR_CONFIG_FILE")
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_FAKE_REMOTE_AUTH", "fail")
        .env("MMR_DEFAULT_DREAM_RUNNER", "command")
        .env_remove("OPENAI_API_KEY")
        .current_dir(&project)
        .output()
        .expect("status diagnostics");
    assert_success_ref(&output);

    let stdout = String::from_utf8(output.stdout).expect("status stdout UTF-8");
    assert!(stdout.contains('\n'), "--pretty should emit readable JSON");
    let status_json: serde_json::Value =
        serde_json::from_str(&stdout).expect("status diagnostics JSON");
    assert_eq!(status_json["command"], "status");
    assert_eq!(status_json["project"], serde_json::Value::Null);
    assert_eq!(status_json["status"]["linked"], false);
    assert_eq!(
        status_json["store"]["db_path"].as_str().unwrap(),
        data_home.join("mmr").join("mmr.db").to_str().unwrap()
    );
    assert_eq!(
        status_json["store"]["schema_version"].as_i64().unwrap(),
        LATEST_SCHEMA_VERSION
    );
    assert_eq!(
        status_json["store"]["expected_schema_version"]
            .as_i64()
            .unwrap(),
        LATEST_SCHEMA_VERSION
    );
    assert_eq!(status_json["store"]["schema_status"], "ok");
    assert_eq!(status_json["store"]["existed_before_command"], false);
    assert_eq!(status_json["diagnostics"]["schema"]["status"], "ok");
    assert_eq!(
        status_json["diagnostics"]["remote"]["status"],
        "auth_failed"
    );
    assert_eq!(
        status_json["diagnostics"]["privacy_filter"]["status"],
        "degraded"
    );
    assert_eq!(
        status_json["diagnostics"]["dream_runner"]["status"],
        "not_required"
    );
    assert_eq!(
        status_json["diagnostics"]["summary_runner"]["status"],
        "missing_api_key"
    );
    assert_eq!(
        status_json["diagnostics"]["summary_runner"]["backend"],
        "openai-compatible"
    );
    assert!(
        status_json["diagnostics"]["summary_runner"]["config_file"]
            .as_str()
            .is_some_and(|path| path.contains("config.json"))
    );
    assert_eq!(
        status_json["diagnostics"]["dream_runner"]["command_env"],
        ""
    );

    let actions = status_json["diagnostics"]["actions"]
        .as_array()
        .expect("actions array");
    assert!(
        actions
            .iter()
            .any(|action| action.as_str().unwrap_or_default().contains("mmr init")),
        "unlinked status should tell the user how to link"
    );
    assert!(
        actions.iter().all(|action| !action
            .as_str()
            .unwrap_or_default()
            .contains("MMR_DREAM_COMMAND")),
        "dream no longer requires a command runner"
    );
    assert!(
        actions.iter().all(|action| !action
            .as_str()
            .unwrap_or_default()
            .contains("MMR_FAKE_REMOTE_AUTH")),
        "product diagnostics should not expose fixture-only auth controls"
    );
    assert!(
        actions.iter().all(|action| !action
            .as_str()
            .unwrap_or_default()
            .contains("unsupported importer")),
        "top-level actions should stay focused on user-recoverable items"
    );

    let sources = status_json["diagnostics"]["sources"]
        .as_array()
        .expect("sources array");
    let codex = sources
        .iter()
        .find(|source| source["source"] == "codex")
        .expect("codex source diagnostic");
    let claude = sources
        .iter()
        .find(|source| source["source"] == "claude")
        .expect("claude source diagnostic");
    assert_eq!(codex["status"], "available");
    assert_eq!(codex["event_count"].as_u64().unwrap(), 0);
    assert_eq!(claude["status"], "missing_source_root");
    assert!(
        claude["action"]
            .as_str()
            .unwrap_or_default()
            .contains("mmr --source claude ingest events --project"),
        "source diagnostics should include copy/paste ingest recovery"
    );
}

#[test]
fn cli_help_contract_is_lean_and_actionable() {
    let help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("--help")
        .output()
        .expect("top-level help");
    assert_success_ref(&help);
    let help_text = String::from_utf8(help.stdout).expect("help UTF-8");
    for command in [
        "init",
        "sync",
        "status",
        "note",
        "find",
        "summarize",
        "assimilate",
    ] {
        assert!(
            help_text.contains(command),
            "top-level help should include public command {command}"
        );
    }
    for command in MVP_NON_GOAL_COMMANDS {
        assert!(
            !help_text.contains(&format!("\n  {command} ")),
            "top-level help should not advertise non-goal command {command}"
        );
    }
    assert!(help_text.contains("mmr init"));
    assert!(help_text.contains("mmr status --pretty"));
    assert!(help_text.contains("mmr assimilate project --pretty"));

    let status_help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["status", "--help"])
        .output()
        .expect("status help");
    assert_success_ref(&status_help);
    let status_help_text = String::from_utf8(status_help.stdout).expect("status help UTF-8");
    assert!(status_help_text.contains("db_path"));
    assert!(status_help_text.contains("schema_version"));
    assert!(status_help_text.contains("diagnostics"));

    let dream_help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["assimilate", "project", "--help"])
        .output()
        .expect("dream help");
    assert_success_ref(&dream_help);
    let dream_help_text = String::from_utf8(dream_help.stdout).expect("dream help UTF-8");
    assert!(dream_help_text.contains("prompt"));
    assert!(dream_help_text.contains("runbook"));
    assert!(!dream_help_text.contains("--dry-run"));
    assert!(!dream_help_text.contains("--review"));
    assert!(!dream_help_text.contains("MMR_DREAM_COMMAND"));
}

#[test]
fn mvp_quickstart_flow_smoke_test() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let base_command = |name: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(name.split_whitespace())
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&project);
        command
    };

    let link = base_command("init").output().expect("quickstart link");
    assert_success_ref(&link);
    let link_json: serde_json::Value =
        serde_json::from_slice(&link.stdout).expect("quickstart link JSON");
    assert_eq!(link_json["status"]["linked"], true);

    let note = base_command("note")
        .args(["Decision:", "document", "the", "NHL-280", "CLI", "flow"])
        .output()
        .expect("quickstart note");
    assert_success_ref(&note);

    let search = base_command("find")
        .arg("NHL-280 CLI flow")
        .output()
        .expect("quickstart search");
    assert_success_ref(&search);
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("quickstart search JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 1);

    let dream = base_command("assimilate project")
        .args(["--pretty"])
        .output()
        .expect("quickstart dream");
    assert_success_ref(&dream);
    let dream_json: serde_json::Value =
        serde_json::from_slice(&dream.stdout).expect("quickstart dream JSON");
    assert_eq!(dream_json["command"], "assimilate/project");
    assert_eq!(dream_json["mode"], "prompt_runbook");
    assert!(
        dream_json["system_prompt"]
            .as_str()
            .unwrap()
            .contains("Memory Assimilation Agent")
    );
    assert!(
        dream_json["runbook"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["step"] == "deduplicate")
    );

    let sync = base_command("sync")
        .args(["--pretty"])
        .output()
        .expect("quickstart sync");
    assert_success_ref(&sync);
    let sync_json: serde_json::Value =
        serde_json::from_slice(&sync.stdout).expect("quickstart sync JSON");
    assert_eq!(sync_json["status"], "synced");

    let status = base_command("status")
        .args(["--pretty"])
        .output()
        .expect("quickstart status");
    assert_success_ref(&status);
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("quickstart status JSON");
    assert_eq!(status_json["status"]["sync_status"], "synced");
    assert_eq!(
        status_json["diagnostics"]["dream_runner"]["status"],
        "not_required"
    );

    for command in ["list projects", "list sessions", "read project"] {
        let output = base_command(command)
            .output()
            .unwrap_or_else(|err| panic!("quickstart raw retrieval {command}: {err}"));
        assert_success_ref(&output);
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap_or_else(|err| panic!("{command} stdout JSON: {err}"));
    }
}

#[test]
fn mvp_release_gate_e2e_fixture_scenario() {
    if !loopback_bind_available() {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let fresh_home = tmp.path().join("fresh-home");
    let data_home = tmp.path().join("data");
    let fresh_data_home = tmp.path().join("fresh-data");
    let project = tmp.path().join("release-project");
    let fresh_project = tmp.path().join("fresh-release-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&fresh_home).expect("create fresh HOME");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&fresh_project).expect("create fresh project");
    let project = std::fs::canonicalize(&project).expect("canonical project");
    let fresh_project = std::fs::canonicalize(&fresh_project).expect("canonical fresh project");
    seed_release_gate_sources(&home, &project);

    let command = |name: &str, data_home: &Path, cwd: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(name.split_whitespace())
            .env("HOME", &home)
            .env("SIMPLEMMR_HOME", &home)
            .env("XDG_DATA_HOME", data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(cwd);
        command
    };
    let fresh_command = |name: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(name.split_whitespace())
            .env("HOME", &fresh_home)
            .env("XDG_DATA_HOME", &fresh_data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&fresh_project);
        command
    };

    let link = command("init", &data_home, &project)
        .output()
        .expect("release link");
    assert_success_ref(&link);
    let link_json: serde_json::Value =
        serde_json::from_slice(&link.stdout).expect("release link JSON");
    assert_eq!(link_json["status"]["linked"], true);
    assert_eq!(
        link_json["remote"]["descriptor"],
        "github:fixture-user/mmr-store"
    );
    assert_eq!(link_json["sync"]["status"], "synced");
    assert_eq!(link_json["status"]["sync_status"], "synced");
    assert!(
        link_json["rebuilt_search_documents"].as_u64().unwrap() >= 6,
        "link should rebuild search documents after import"
    );
    let link_stdout = String::from_utf8_lossy(&link.stdout);
    let link_stderr = String::from_utf8_lossy(&link.stderr);
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!link_stdout.contains(sensitive));
        assert!(!link_stderr.contains(sensitive));
    }
    for source in ["codex", "claude", "cursor"] {
        let import = link_json["imports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|import| import["source"] == source)
            .unwrap_or_else(|| panic!("release link import for {source}"));
        assert_eq!(import["status"], "imported");
        assert!(import["discovered_sessions"].as_u64().unwrap() >= 1);
        assert!(import["imported_events"].as_u64().unwrap() >= 2);
    }
    let link_remote_text = remote_file_text(&remote);
    assert!(link_remote_text.contains("Release Codex fixture records adapter setup."));
    assert!(link_remote_text.contains("Release Claude fixture is normalized safely."));
    assert!(link_remote_text.contains("Release Cursor fixture is normalized safely."));
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!link_remote_text.contains(sensitive));
    }
    assert!(remote_event_file_count(&remote) > 0);
    let link_store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store after link");
    let link_project = link_store
        .project_by_path(&project)
        .expect("project after link lookup")
        .expect("project after link");
    assert!(
        link_store
            .sync_manifests_for_project(&link_project.id)
            .expect("link sync manifests")
            .iter()
            .any(|manifest| {
                !link_store
                    .sync_manifest_entries(&manifest.id)
                    .expect("link sync manifest entries")
                    .is_empty()
            }),
        "link should write at least one sync manifest with entries"
    );
    drop(link_store);

    let relink = command("init", &data_home, &project)
        .output()
        .expect("release relink");
    assert_success_ref(&relink);
    let relink_json: serde_json::Value =
        serde_json::from_slice(&relink.stdout).expect("release relink JSON");
    assert_eq!(relink_json["already_linked"], true);
    assert_eq!(relink_json["sync"]["uploaded_events"].as_u64().unwrap(), 0);

    let note = command("note", &data_home, &project)
        .args([
            "Release",
            "gate",
            "safe",
            "note",
            "prefers",
            "evidence-linked",
            "checks.",
        ])
        .output()
        .expect("release safe note");
    assert_success_ref(&note);
    let unsafe_note_text = format!("OPENAI_API_KEY={RELEASE_NOTE_SECRET}");
    let unsafe_note = command("note", &data_home, &project)
        .args(["Unsafe", "fixture", "secret", unsafe_note_text.as_str()])
        .output()
        .expect("release unsafe note");
    assert_success_ref(&unsafe_note);
    let pii_note = command("note", &data_home, &project)
        .args(["Contact", RELEASE_NOTE_EMAIL, "after", "release", "gate."])
        .output()
        .expect("release PII note");
    assert_success_ref(&pii_note);

    let sensitive_codex_root = tmp.path().join("sensitive-codex-root");
    let sensitive_codex_sessions = sensitive_codex_root.join("sessions");
    std::fs::create_dir_all(&sensitive_codex_sessions).expect("create sensitive codex root");
    let sensitive_codex = format!(
        r#"{{"type":"session_meta","timestamp":"2026-05-24T10:10:00Z","payload":{{"id":"release-codex-sensitive","cwd":"{}","model_provider":"openai"}}}}
{{"type":"event_msg","timestamp":"2026-05-24T10:10:01Z","payload":{{"type":"user_message","message":"Release Codex imported secret OPENAI_API_KEY={RELEASE_NOTE_SECRET}"}}}}
{{"type":"event_msg","timestamp":"2026-05-24T10:10:02Z","payload":{{"type":"user_message","message":"Release Codex imported contact {RELEASE_NOTE_EMAIL}."}}}}
"#,
        project.to_str().expect("project path UTF-8")
    );
    std::fs::write(
        sensitive_codex_sessions.join("release-codex-sensitive.jsonl"),
        sensitive_codex,
    )
    .expect("write sensitive codex fixture");
    let sensitive_import = command("ingest events", &data_home, &project)
        .args([
            "--source",
            "codex",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            sensitive_codex_root
                .to_str()
                .expect("sensitive codex root UTF-8"),
        ])
        .output()
        .expect("release sensitive codex import");
    assert_success_ref(&sensitive_import);
    let sensitive_import_json: serde_json::Value =
        serde_json::from_slice(&sensitive_import.stdout).expect("sensitive import JSON");
    assert_eq!(sensitive_import_json["source"], "codex");
    assert_eq!(
        sensitive_import_json["imported_events"].as_u64().unwrap(),
        3
    );
    for output in [&note, &unsafe_note, &pii_note] {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stdout.contains(RELEASE_NOTE_SECRET));
        assert!(!stderr.contains(RELEASE_NOTE_SECRET));
        assert!(!stdout.contains(RELEASE_NOTE_EMAIL));
        assert!(!stderr.contains(RELEASE_NOTE_EMAIL));
    }

    let projects_output = command("list projects", &data_home, &project)
        .output()
        .expect("release raw projects");
    assert_success_ref(&projects_output);
    let projects_json: serde_json::Value =
        serde_json::from_slice(&projects_output.stdout).expect("projects JSON");
    let project_sources = projects_json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["source"].as_str().unwrap())
        .collect::<Vec<_>>();
    for source in ["codex", "claude", "cursor"] {
        assert!(
            project_sources.contains(&source),
            "projects should include {source}"
        );
    }

    let sessions_output = command("list sessions", &data_home, &project)
        .arg("--all")
        .output()
        .expect("release raw sessions");
    assert_success_ref(&sessions_output);
    let sessions_json: serde_json::Value =
        serde_json::from_slice(&sessions_output.stdout).expect("sessions JSON");
    let session_ids = sessions_json["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    for session_id in ["release-codex", "release-claude", "release-cursor"] {
        assert!(
            session_ids.contains(&session_id),
            "sessions should include {session_id}"
        );
    }

    let messages_output = command("read project", &data_home, &project)
        .output()
        .expect("release raw messages");
    assert_success_ref(&messages_output);
    let messages_text = String::from_utf8(messages_output.stdout).expect("messages stdout UTF-8");
    assert!(
        messages_text.contains("Release Codex fixture")
            && messages_text.contains("Release Claude fixture")
            && messages_text.contains("Release Cursor fixture"),
        "messages should expose fixture-backed raw transcript history from every source"
    );

    let export_output = command("read project", &data_home, &project)
        .output()
        .expect("release raw export");
    assert_success_ref(&export_output);
    let export_text = String::from_utf8(export_output.stdout).expect("export stdout UTF-8");
    assert!(
        export_text.contains("Release Codex fixture")
            && export_text.contains("Release Claude fixture")
            && export_text.contains("Release Cursor fixture"),
        "export should expose fixture-backed raw transcript history from every source"
    );

    for text in [messages_text.as_str(), export_text.as_str()] {
        assert!(
            text.contains("Release Codex fixture")
                && text.contains("Release Claude fixture")
                && text.contains("Release Cursor fixture"),
            "raw retrieval should include all source fixtures"
        );
    }

    let search = command("find", &data_home, &project)
        .arg("Release gate safe note")
        .output()
        .expect("release search");
    assert_success_ref(&search);
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("release search JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 1);
    let evidence_event_id = search_json["results"][0]["event_id"]
        .as_str()
        .expect("evidence event id")
        .to_string();
    assert!(
        search_json["results"][0]["citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://event/")
    );
    let evidence_ref = format!("mmr://event/{evidence_event_id}");

    let rg = command("find", &data_home, &project)
        .args(["Release gate safe note", "--format", "line"])
        .output()
        .expect("release rg");
    assert_success_ref(&rg);
    assert!(
        String::from_utf8(rg.stdout)
            .unwrap()
            .contains("mmr://event/")
    );

    let structured_search = command("find", &data_home, &project)
        .args(["Release Claude fixture", "--role", "assistant"])
        .env("MMR_DEFAULT_SOURCE", "claude")
        .output()
        .expect("release structured search");
    assert_success_ref(&structured_search);
    let structured_search_json: serde_json::Value =
        serde_json::from_slice(&structured_search.stdout).expect("structured search JSON");
    assert_eq!(structured_search_json["total_results"].as_u64().unwrap(), 1);
    assert_eq!(structured_search_json["results"][0]["source"], "claude");

    let summary_response = format!(
        r#"{{"id":"release-summary","model":"test-model","choices":[{{"message":{{"role":"assistant","content":"Release summary cites {evidence_ref}"}}}}]}}"#
    );
    let (summary_base_url, summary_request, summary_handle) =
        start_mock_chat_completions_server(summary_response);
    let summary = command("summarize project", &data_home, &project)
        .args([
            "--project",
            project.to_str().expect("project path UTF-8"),
            "-O",
            "json",
        ])
        .env("OPENAI_API_KEY", "test-key")
        .env("OPENAI_BASE_URL", summary_base_url)
        .output()
        .expect("release summary");
    assert_success_ref(&summary);
    summary_handle.join().expect("summary server thread");
    let summary_json: serde_json::Value =
        serde_json::from_slice(&summary.stdout).expect("summary JSON");
    assert_eq!(summary_json["backend"], "openai-compatible");
    assert!(
        summary_json["text"]
            .as_str()
            .unwrap()
            .contains(&evidence_ref)
    );
    let summary_body = summary_request
        .lock()
        .expect("summary request")
        .clone()
        .expect("summary request body");
    let summary_input = first_input_text(&summary_body);
    assert!(summary_input.contains("Release Codex fixture"));
    assert!(summary_input.contains("Release Claude fixture"));
    assert!(summary_input.contains("Release Cursor fixture"));

    let (remember_base_url, remember_request, remember_handle) = start_mock_chat_completions_server(
        r#"{"id":"release-summarize","model":"test-model","choices":[{"message":{"role":"assistant","content":"Summarize project works"}}]}"#.to_string(),
    );
    let remember = command("summarize project", &data_home, &project)
        .args([
            "--project",
            project.to_str().expect("project path UTF-8"),
            "-O",
            "json",
        ])
        .env("OPENAI_API_KEY", "test-key")
        .env("OPENAI_BASE_URL", remember_base_url)
        .output()
        .expect("release summarize project");
    assert_success_ref(&remember);
    remember_handle.join().expect("remember server thread");
    let remember_json: serde_json::Value =
        serde_json::from_slice(&remember.stdout).expect("remember JSON");
    assert_eq!(remember_json["backend"], "openai-compatible");
    assert_eq!(remember_json["text"], "Summarize project works");
    let remember_body = remember_request
        .lock()
        .expect("remember request")
        .clone()
        .expect("remember request body");
    let remember_input = first_input_text(&remember_body);
    assert!(remember_input.contains("Release Codex fixture"));
    assert!(remember_input.contains("Release Claude fixture"));
    assert!(remember_input.contains("Release Cursor fixture"));

    let redaction = command("redact", &data_home, &project)
        .args([
            "scan",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .output()
        .expect("release redaction scan");
    assert_success_ref(&redaction);
    let redaction_stderr = String::from_utf8_lossy(&redaction.stderr);
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!redaction_stderr.contains(sensitive));
    }
    let redaction_stdout = String::from_utf8(redaction.stdout).expect("redaction stdout UTF-8");
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!redaction_stdout.contains(sensitive));
    }
    let redaction_json: serde_json::Value =
        serde_json::from_str(&redaction_stdout).expect("redaction JSON");
    assert!(redaction_json["blocked"].as_u64().unwrap() >= 1);

    let dry_run = command("sync", &data_home, &project)
        .arg("--dry-run")
        .output()
        .expect("release sync dry run");
    assert_success_ref(&dry_run);
    let dry_run_stderr = String::from_utf8_lossy(&dry_run.stderr);
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!dry_run_stderr.contains(sensitive));
    }
    let dry_run_text = String::from_utf8(dry_run.stdout).expect("dry-run stdout UTF-8");
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!dry_run_text.contains(sensitive));
    }
    let dry_run_json: serde_json::Value =
        serde_json::from_str(&dry_run_text).expect("dry-run JSON");
    assert!(dry_run_json["blocked_events"].as_u64().unwrap() >= 1);
    assert!(
        dry_run_json["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| {
                event["blocked_reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason.as_str().unwrap().contains("deterministic secret"))
            }),
        "unsafe note secret should be blocked before sync"
    );

    let dream = command("assimilate project", &data_home, &project)
        .env(
            "MMR_DREAM_MOCK_OUTPUT",
            dream_output_json(
                &evidence_ref,
                "Ignored because public dream no longer runs mock output.",
                0.94,
                "",
            ),
        )
        .output()
        .expect("release dream guide");
    assert_success_ref(&dream);
    let dream_json: serde_json::Value = serde_json::from_slice(&dream.stdout).expect("dream JSON");
    assert_eq!(dream_json["mode"], "prompt_runbook");
    assert!(
        dream_json["evidence"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["evidence_ref"] == evidence_ref),
        "dream evidence should include the searched event ref"
    );
    assert!(
        dream_json["system_prompt"]
            .as_str()
            .unwrap()
            .contains("Perform memory deduplication")
    );

    let learned_search = command("find", &data_home, &project)
        .arg("release-gate evidence checks")
        .output()
        .expect("release learned search");
    assert_success_ref(&learned_search);
    let learned_search_json: serde_json::Value =
        serde_json::from_slice(&learned_search.stdout).expect("learned search JSON");
    assert_eq!(learned_search_json["total_results"].as_u64().unwrap(), 0);

    let sync = command("sync", &data_home, &project)
        .output()
        .expect("release sync");
    assert_success_ref(&sync);
    let sync_json: serde_json::Value = serde_json::from_slice(&sync.stdout).expect("sync JSON");
    assert!(matches!(
        sync_json["status"].as_str().unwrap(),
        "partial" | "synced"
    ));
    assert!(sync_json["blocked_events"].as_u64().unwrap() >= 1);
    assert!(
        sync_json["blocked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| {
                event["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason.as_str().unwrap().contains("deterministic secret"))
            }),
        "sync should report the unsafe note secret blocker"
    );
    let remote_text = remote_file_text(&remote);
    for sensitive in [RELEASE_NOTE_SECRET, RELEASE_NOTE_EMAIL] {
        assert!(!remote_text.contains(sensitive));
    }
    assert!(!remote_text.contains(&project.to_string_lossy().to_string()));
    assert!(!remote_text.contains(&fresh_project.to_string_lossy().to_string()));
    assert!(remote_text.contains("Release gate safe note"));
    assert!(remote_text.contains("[REDACTED:private_email]"));
    assert!(remote_text.contains("Release Codex imported contact [REDACTED:private_email]."));
    assert!(!remote_text.contains("Prefer release-gate evidence checks."));

    let hydrate = fresh_command("init").output().expect("release hydrate");
    assert_success_ref(&hydrate);
    let hydrate_json: serde_json::Value =
        serde_json::from_slice(&hydrate.stdout).expect("hydrate JSON");
    assert!(
        hydrate_json["hydration"]["inserted_events"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert_eq!(
        hydrate_json["hydration"]["inserted_learned_memory"]
            .as_u64()
            .unwrap(),
        0
    );
    assert!(
        hydrate_json["imports"]
            .as_array()
            .expect("hydrate imports array")
            .iter()
            .all(|import| import["imported_events"].as_u64().unwrap_or(0) == 0),
        "fresh host should hydrate from remote instead of local source fixtures: {hydrate_json}"
    );

    let hydrated_event_search = fresh_command("find")
        .arg("Release gate safe note")
        .output()
        .expect("hydrated event search");
    assert_success_ref(&hydrated_event_search);
    let hydrated_event_search_json: serde_json::Value =
        serde_json::from_slice(&hydrated_event_search.stdout).expect("hydrated event search JSON");
    assert_eq!(
        hydrated_event_search_json["total_results"]
            .as_u64()
            .unwrap(),
        1
    );
    let fresh_evidence_ref = format!(
        "mmr://event/{}",
        hydrated_event_search_json["results"][0]["event_id"]
            .as_str()
            .expect("hydrated event id")
    );

    let fresh_store = Store::open(fresh_data_home.join("mmr").join("mmr.db")).expect("fresh store");
    let fresh_project_record = fresh_store
        .project_by_path(&fresh_project)
        .expect("fresh project lookup")
        .expect("fresh project");
    let fresh_events = fresh_store
        .events_for_project(&fresh_project_record.id, None, None)
        .expect("fresh events");
    assert!(
        fresh_events.iter().any(|event| event.id
            == hydrated_event_search_json["results"][0]["event_id"]
                .as_str()
                .unwrap()),
        "hydrated search event should exist in the fresh store"
    );
    let fresh_memory = fresh_store
        .learned_memory_for_project(&fresh_project_record.id)
        .expect("fresh learned memory");
    assert!(fresh_memory.is_empty());

    let fresh_dream = fresh_command("assimilate project")
        .output()
        .expect("fresh release dream");
    assert_success_ref(&fresh_dream);
    let fresh_dream_json: serde_json::Value =
        serde_json::from_slice(&fresh_dream.stdout).expect("fresh dream JSON");
    assert_eq!(fresh_dream_json["mode"], "prompt_runbook");
    assert!(
        fresh_dream_json["evidence"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["evidence_ref"] == fresh_evidence_ref),
        "fresh dream evidence should include the hydrated searched event ref"
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
        .args(["find", "panic at src/main.rs:42"])
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
        .args(["find", "ERROR[abc]*"])
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
        .args(["--source", "codex", "find", "PANIC", "--ignore-case"])
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
        .args(["find", "panic at src/main.rs:42", "--format", "line"])
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
}

#[test]
fn search_cli_contract_is_implemented() {
    let (_tmp, home, data_home, project, _, note_event_id) = seed_search_fixture();

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["find", "decision", "--role", "user", "--session", "notes"])
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
            "find",
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
fn retrieve_fixture_smoke_find_and_read_are_isolated() {
    let fixture = RetrieveContractFixture::seeded();

    let find = fixture.run_cli(&[
        "find",
        "retrieve fixture smoke",
        "--project",
        fixture.project_arg(),
    ]);
    assert_success_ref(&find);
    let find_json: serde_json::Value =
        serde_json::from_slice(&find.stdout).expect("fixture smoke find JSON");
    assert_eq!(find_json["total_results"].as_u64().unwrap(), 1);

    let read = fixture.run_cli(&[
        "read",
        "session",
        "retrieve-codex-alpha",
        "--project",
        fixture.project_arg(),
    ]);
    assert_success_ref(&read);
    let read_json: serde_json::Value =
        serde_json::from_slice(&read.stdout).expect("fixture smoke read JSON");
    assert_eq!(read_json["total_messages"].as_u64().unwrap(), 6);
    assert!(
        read_json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| {
                message["session_id"] == "retrieve-codex-alpha" && message["source"] == "codex"
            })
    );
}

#[test]
fn retrieve_store_to_provider_mapping_uses_public_source_session_id() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&[
        "retrieve",
        "public mapping marker",
        "--max-sessions",
        "1",
        "--before-messages",
        "1",
        "--after-messages",
        "1",
    ]);
    assert_success_ref(&output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("retrieve JSON");

    assert_eq!(json["query"], "public mapping marker");
    assert_eq!(json["total_selected_sessions"].as_u64().unwrap(), 1);
    let selected = &json["selected_sessions"][0];
    assert_eq!(selected["source"], "codex");
    assert_eq!(selected["source_session_id"], "retrieve-codex-alpha");
    assert!(
        !selected
            .as_object()
            .expect("selected session object")
            .contains_key("session_id"),
        "retrieve selected sessions must not expose Store-internal session_id"
    );
    let messages = selected["messages"].as_array().expect("messages");
    assert!(
        !messages.is_empty(),
        "retrieve should read provider messages"
    );
    assert!(messages.iter().all(|message| {
        message["session_id"] == "retrieve-codex-alpha" && message["source"] == "codex"
    }));
}

#[test]
fn retrieve_ranking_ties_use_documented_order() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&["retrieve", "ranking tie marker", "--max-sessions", "3"]);
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("ranking retrieve JSON");
    let sessions = json["selected_sessions"].as_array().expect("sessions");

    let ranked = sessions
        .iter()
        .map(|session| {
            (
                session["source"].as_str().unwrap(),
                session["source_session_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ranked,
        vec![
            ("claude", "retrieve-claude-alpha"),
            ("codex", "retrieve-codex-alpha"),
            ("codex", "retrieve-codex-beta"),
        ]
    );
    assert!(sessions.iter().enumerate().all(|(idx, session)| {
        session["rank"].as_u64().unwrap() == (idx + 1) as u64
            && session["match_count"].as_u64().unwrap() == 2
            && session["rank_reason"]["tie_break"]
                .as_array()
                .unwrap()
                .len()
                == 3
            && session["rank_reason"]["match_count"].as_u64().unwrap() == 2
            && session["rank_reason"]["latest_match_timestamp"] == "2026-06-28T08:00:00Z"
    }));
    assert!(json["suggested_next_action"].is_null());
    assert!(sessions.iter().all(|session| {
        session["rank_reason"]["match_count"].as_u64().unwrap() == 2
            && session["rank_reason"]["latest_match_timestamp"] == "2026-06-28T08:00:00Z"
    }));
}

#[test]
fn retrieve_bounded_windows_merge_truncate_and_preserve_citations() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&[
        "retrieve",
        "window marker",
        "--before-messages",
        "2",
        "--after-messages",
        "2",
        "--max-messages-per-session",
        "3",
        "--limit",
        "3",
        "-C",
        "1",
    ]);
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("window retrieve JSON");
    let selected = &json["selected_sessions"][0];

    assert!(
        selected["first_match_citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://event/")
    );
    assert!(selected["matches"].as_array().unwrap().iter().all(|item| {
        item["citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://event/")
    }));
    assert_eq!(selected["message_window"]["before_messages"], 2);
    assert_eq!(selected["message_window"]["after_messages"], 2);
    assert_eq!(selected["message_window"]["max_messages_per_session"], 3);
    assert_eq!(selected["message_window"]["truncated"], true);
    assert!(selected["messages"].as_array().unwrap().len() <= 3);
    assert!(
        selected["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["content"]
                    .as_str()
                    .unwrap()
                    .contains("window marker anchor")
            })
    );
}

#[test]
fn retrieve_unreadable_matches_include_learned_memory_and_db_only_events() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&["retrieve", "unreadable marker", "--max-sessions", "5"]);
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("unreadable retrieve JSON");
    let unreadable = json["unreadable_matches"]
        .as_array()
        .expect("unreadable matches");

    assert_eq!(unreadable.len(), 2);
    assert!(unreadable.iter().any(|item| {
        item["citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://learned-memory/")
            && item["source"] == "learned_memory"
            && item["snippet"]
                .as_str()
                .unwrap()
                .contains("unreadable marker")
    }));
    assert!(unreadable.iter().any(|item| {
        item["citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://event/")
            && item["source"] == "codex"
            && item["event_id"].as_str().unwrap().starts_with("event:v1:")
    }));
    assert!(unreadable.iter().all(|item| {
        item.get("reason").is_some()
            && item["before"].is_array()
            && item["after"].is_array()
            && item.get("raw_local_ref").is_none()
    }));
}

#[test]
fn retrieve_empty_match_returns_success_json_with_next_action() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&["retrieve", "no such retrieve fixture phrase"]);
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("empty retrieve JSON");

    assert_eq!(json["query"], "no such retrieve fixture phrase");
    assert_eq!(json["total_matches"].as_u64().unwrap(), 0);
    assert_eq!(json["total_selected_sessions"].as_u64().unwrap(), 0);
    assert!(json["selected_sessions"].as_array().unwrap().is_empty());
    assert!(json["unreadable_matches"].as_array().unwrap().is_empty());
    assert_eq!(json["next_page"], false);
    assert!(json["next_command"].is_null());
    assert!(
        json["suggested_next_action"]
            .as_str()
            .unwrap()
            .contains("--ignore-case")
    );
}

#[test]
fn retrieve_output_does_not_leak_raw_local_ref() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&["retrieve", "citation marker", "-C", "1"]);
    assert_success_ref(&output);
    let stdout = String::from_utf8(output.stdout).expect("retrieve stdout UTF-8");

    assert!(stdout.contains("mmr://event/"));
    assert!(!stdout.contains("raw_local_ref"));
    assert!(!stdout.contains("tests/fixtures/retrieve"));
    assert!(!stdout.contains("retrieve-codex-alpha.jsonl:1"));
}

#[test]
fn summary_cli_contract_is_implemented() {
    let summary_help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["summarize", "--help"])
        .output()
        .expect("summary help");
    assert_success_ref(&summary_help);
    let summary_help_text = String::from_utf8(summary_help.stdout).expect("summary help UTF-8");
    assert!(summary_help_text.contains("project"));
    assert!(summary_help_text.contains("source"));
    assert!(summary_help_text.contains("session"));

    let project_help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["summarize", "project", "--help"])
        .output()
        .expect("summarize project help");
    assert_success_ref(&project_help);
    let project_help_text =
        String::from_utf8(project_help.stdout).expect("summarize project help UTF-8");
    assert!(!project_help_text.contains("--agent"));
    assert!(project_help_text.contains("--model"));
    assert!(project_help_text.contains("--output-format"));
    assert!(project_help_text.contains("--limit"));
    assert!(project_help_text.contains("--offset"));

    let session_help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["summarize", "session", "--help"])
        .output()
        .expect("summarize session help");
    assert_success_ref(&session_help);
    let session_help_text =
        String::from_utf8(session_help.stdout).expect("summarize session help UTF-8");
    assert!(session_help_text.contains("--limit"));
    assert!(session_help_text.contains("--offset"));
}

#[test]
fn dream_runner_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let project_dir = tmp.path().join("dream-project");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let mut store = Store::open_in_memory().expect("store");
    let project = store.ensure_project_link(&project_dir).expect("project");
    let event = store
        .insert_event(
            &project.id,
            &mmr::store::NewEvent::new(
                "note",
                "notes",
                "message",
                "user",
                "2026-05-24T15:00:00Z",
                "Email person@example.com and prefer fixture-driven tests.",
                "note-v1",
            ),
        )
        .expect("dream evidence event");
    store
        .insert_event(
            &project.id,
            &mmr::store::NewEvent::new(
                "note",
                "notes",
                "message",
                "user",
                "2026-05-24T15:01:00Z",
                "API_KEY=sk-test-secret",
                "note-v1",
            ),
        )
        .expect("blocked dream evidence event");

    let config = DreamRunnerConfig {
        runner: "mock".to_string(),
        model: Some("mock-v1".to_string()),
        evidence_access: EvidenceAccess::SharedSafe,
        allow_raw_evidence: false,
        best_of: 1,
        retries: 0,
    };
    let request = build_evidence_request(&store, &project, &config).expect("dream request");
    assert_eq!(request.evidence.len(), 1);
    assert!(
        !request.evidence[0]
            .content_text
            .contains("person@example.com")
    );
    let bundle =
        build_evidence_bundle(&store, &project, DreamEvidenceMode::SharedSafe).expect("bundle");
    assert_eq!(
        bundle.omitted_events.len(),
        1,
        "secret-bearing evidence is not sent to shared-safe runners"
    );

    let event_ref = format!("mmr://event/{}", event.id);
    let runner = MockDreamRunner::returning_json(dream_observation_json(&event_ref, 0.91));
    let output = runner.run(&request).expect("valid dream output");
    assert_eq!(
        output.observations[0].status,
        DreamObservationStatus::Active
    );

    let invalid_runner = MockDreamRunner::returning_json(dream_observation_json(
        &bundle.omitted_events[0].evidence_ref,
        0.91,
    ));
    let err = invalid_runner
        .run(&request)
        .expect_err("omitted evidence ref should fail");
    assert!(err.to_string().contains("missing evidence"));

    let pending_runner = MockDreamRunner::returning_json(dream_observation_json(&event_ref, 0.42));
    let pending = pending_runner.run(&request).expect("pending dream");
    assert_eq!(
        pending.observations[0].status,
        DreamObservationStatus::Pending
    );
}

#[test]
fn dream_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let data_home_fresh = tmp.path().join("data-fresh");
    let project = tmp.path().join("dream-project");
    let fresh_project = tmp.path().join("fresh-dream-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&fresh_project).expect("create fresh project");

    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg("init")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&project)
            .output()
            .expect("link before dream"),
    );
    let note = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "note",
            "Email",
            "person@example.com",
            "while",
            "discussing",
            "durable",
            "assimilation",
            "evidence.",
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("add dream evidence note");
    assert_success_ref(&note);
    let note = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["find", "durable assimilation evidence"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("find note evidence");
    assert_success_ref(&note);
    let note_json: serde_json::Value =
        serde_json::from_slice(&note.stdout).expect("note search stdout JSON");
    let evidence_event_id = note_json["results"][0]["event_id"]
        .as_str()
        .expect("evidence event id")
        .to_string();
    let evidence_ref = format!("mmr://event/{evidence_event_id}");
    let ignored_runner_output = dream_output_json(
        &evidence_ref,
        "Prefer durable assimilation checks.",
        0.93,
        "",
    );
    let dream = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["assimilate", "project"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_DREAM_MOCK_OUTPUT", &ignored_runner_output)
        .env("MMR_DEFAULT_DREAM_RUNNER", "unsupported")
        .env("MMR_DREAM_COMMAND", "false")
        .current_dir(&project)
        .output()
        .expect("dream guide");
    assert_success_ref(&dream);
    let dream_json: serde_json::Value =
        serde_json::from_slice(&dream.stdout).expect("dream stdout JSON");
    assert_eq!(dream_json["command"], "assimilate/project");
    assert_eq!(dream_json["mode"], "prompt_runbook");
    assert_eq!(
        dream_json["evidence"]["included_events"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        dream_json["evidence"]["events"][0]["evidence_ref"],
        evidence_ref
    );
    assert!(
        dream_json["system_prompt"]
            .as_str()
            .unwrap()
            .contains("knowledge assimilation")
    );
    assert!(
        dream_json["runbook"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["step"] == "generalise")
    );
    {
        let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
        let project_record = store
            .project_by_path(&project)
            .expect("project lookup")
            .expect("project");
        assert!(
            store
                .learned_memory_for_project(&project_record.id)
                .expect("learned memory")
                .is_empty()
        );
    }

    let legacy_dry_run = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["assimilate", "project", "--dry-run"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("dream dry-run legacy flag");
    assert!(!legacy_dry_run.status.success());
    assert!(
        String::from_utf8_lossy(&legacy_dry_run.stderr).contains("unexpected argument"),
        "stderr={}",
        String::from_utf8_lossy(&legacy_dry_run.stderr)
    );

    let legacy_runner = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["assimilate", "project", "--runner", "command"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("dream runner legacy flag");
    assert!(!legacy_runner.status.success());

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["find", "durable assimilation checks"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("search learned memory");
    assert_success_ref(&search);
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("search JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 0);

    let reserved_flag = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["assimilate", "project", "--best-of", "2"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&project)
        .output()
        .expect("dream reserved flag");
    assert!(!reserved_flag.status.success());
    assert!(
        String::from_utf8_lossy(&reserved_flag.stderr).contains("unexpected argument"),
        "stderr={}",
        String::from_utf8_lossy(&reserved_flag.stderr)
    );

    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg("sync")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&project)
            .output()
            .expect("sync dream evidence"),
    );
    let remote_text = remote_file_text(&remote);
    assert!(!remote_text.contains("person@example.com"));
    assert!(!remote_text.contains(&evidence_event_id));
    assert!(remote_text.contains("[REDACTED:private_email]"));
    assert!(!remote_text.contains("Prefer durable assimilation checks."));

    let hydrate = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home_fresh)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&fresh_project)
        .output()
        .expect("hydrate dream evidence");
    assert_success_ref(&hydrate);
    let hydrate_json: serde_json::Value =
        serde_json::from_slice(&hydrate.stdout).expect("hydrate JSON");
    assert_eq!(
        hydrate_json["hydration"]["inserted_learned_memory"]
            .as_u64()
            .unwrap(),
        0
    );
    let hydrated_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["find", "durable assimilation checks"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home_fresh)
        .current_dir(&fresh_project)
        .output()
        .expect("search hydrated learned memory");
    assert_success_ref(&hydrated_search);
    let hydrated_search_json: serde_json::Value =
        serde_json::from_slice(&hydrated_search.stdout).expect("hydrated search JSON");
    assert_eq!(hydrated_search_json["total_results"].as_u64().unwrap(), 0);
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
            "read",
            "project",
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
fn codex_import_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    let other_project = tmp.path().join("other-project");
    let codex_root = home.join(".codex");
    let sessions_dir = codex_root.join("sessions/2026/05/24");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&other_project).expect("create other project");
    std::fs::create_dir_all(&sessions_dir).expect("create codex sessions");
    let matching_rollout = include_str!("fixtures/memory_fabric/codex_rollout_session.jsonl")
        .replace(
            "/Users/test/memory-fabric",
            project.to_str().expect("project path UTF-8"),
        );
    std::fs::write(sessions_dir.join("rollout.jsonl"), matching_rollout)
        .expect("write codex rollout");
    let unrelated_rollout = include_str!("fixtures/memory_fabric/codex_rollout_session.jsonl")
        .replace("codex-rollout-1", "codex-rollout-other")
        .replace(
            "/Users/test/memory-fabric",
            other_project.to_str().expect("other project UTF-8"),
        )
        .replace(
            "Investigate importer drift.",
            "Unrelated project should not import.",
        );
    std::fs::write(sessions_dir.join("other-rollout.jsonl"), unrelated_rollout)
        .expect("write unrelated codex rollout");

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "codex",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            codex_root.to_str().expect("codex root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("codex import");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("import JSON");
    assert_eq!(json["source"], "codex");
    assert_eq!(json["discovered_sessions"].as_u64().unwrap(), 1);
    assert_eq!(json["imported_events"].as_u64().unwrap(), 6);
    assert!(json["warnings"].as_array().unwrap().is_empty());

    let replay = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "codex",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            codex_root.to_str().expect("codex root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("codex import replay");
    assert!(
        replay.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_json: serde_json::Value =
        serde_json::from_slice(&replay.stdout).expect("replay JSON");
    assert_eq!(replay_json["imported_events"].as_u64().unwrap(), 0);

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "codex",
            "find",
            "CodexAdapter",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search imported codex");
    assert!(
        search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("search JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 1);
    assert_eq!(search_json["results"][0]["source"], "codex");

    let unrelated_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "codex",
            "find",
            "Unrelated project should not import.",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search unrelated codex");
    assert!(
        unrelated_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&unrelated_search.stderr)
    );
    let unrelated_search_json: serde_json::Value =
        serde_json::from_slice(&unrelated_search.stdout).expect("unrelated search JSON");
    assert_eq!(unrelated_search_json["total_results"].as_u64().unwrap(), 0);

    let cwd_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "codex",
            "find",
            project.to_str().expect("project path UTF-8"),
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search project path leakage");
    assert!(
        cwd_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&cwd_search.stderr)
    );
    let cwd_search_json: serde_json::Value =
        serde_json::from_slice(&cwd_search.stdout).expect("cwd search JSON");
    assert_eq!(cwd_search_json["total_results"].as_u64().unwrap(), 0);

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let cursor = store
        .source_cursor(
            &project_record.id,
            "codex",
            sessions_dir
                .join("rollout.jsonl")
                .to_str()
                .expect("cursor key UTF-8"),
        )
        .expect("cursor read")
        .expect("cursor");
    assert_eq!(cursor.parser_version, CodexAdapter::PARSER_VERSION);
    assert!(cursor.last_event_hash.is_some());
}

#[test]
fn claude_import_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-claude-project");
    let other_project = tmp.path().join("other-claude-project");
    let claude_root = home.join(".claude");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&other_project).expect("create other project");
    let project = std::fs::canonicalize(&project).expect("canonical project");
    let other_project = std::fs::canonicalize(&other_project).expect("canonical other project");
    let sessions_dir = claude_root
        .join("projects")
        .join(encode_claude_project_name(&project));
    let other_sessions_dir = claude_root
        .join("projects")
        .join(encode_claude_project_name(&other_project));
    std::fs::create_dir_all(&sessions_dir).expect("create claude sessions");
    std::fs::create_dir_all(&other_sessions_dir).expect("create other claude sessions");
    let matching_rollout = include_str!("fixtures/memory_fabric/claude_code_session.jsonl")
        .replace(
            "/Users/test/memory-fabric",
            project.to_str().expect("project path UTF-8"),
        );
    std::fs::write(sessions_dir.join("session.jsonl"), matching_rollout)
        .expect("write claude rollout");
    std::fs::write(
        sessions_dir.join("no-cwd-session.jsonl"),
        r#"{"type":"user","sessionId":"claude-no-cwd","message":{"role":"user","content":"Hyphen fallback Claude project imported."},"timestamp":"2026-05-24T13:45:00Z","uuid":"u-claude-no-cwd"}
"#,
    )
    .expect("write no-cwd claude rollout");
    let unrelated_rollout = include_str!("fixtures/memory_fabric/claude_code_session.jsonl")
        .replace("claude-code-rollout-1", "claude-code-other")
        .replace(
            "/Users/test/memory-fabric",
            other_project.to_str().expect("other project UTF-8"),
        )
        .replace(
            "Summarize the Claude importer plan.",
            "Unrelated Claude project should not import.",
        );
    std::fs::write(
        other_sessions_dir.join("other-session.jsonl"),
        unrelated_rollout,
    )
    .expect("write unrelated claude rollout");

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "claude",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            claude_root.to_str().expect("claude root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("claude import");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("import JSON");
    assert_eq!(json["source"], "claude");
    assert_eq!(json["discovered_sessions"].as_u64().unwrap(), 2);
    assert_eq!(json["imported_events"].as_u64().unwrap(), 8);
    assert!(json["warnings"].as_array().unwrap().is_empty());

    let replay = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "claude",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            claude_root.to_str().expect("claude root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("claude import replay");
    assert!(
        replay.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_json: serde_json::Value =
        serde_json::from_slice(&replay.stdout).expect("replay JSON");
    assert_eq!(replay_json["imported_events"].as_u64().unwrap(), 0);

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "claude",
            "find",
            "TodoWrite",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search imported claude");
    assert!(
        search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("search JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 1);
    assert_eq!(search_json["results"][0]["source"], "claude");

    let fallback_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "claude",
            "find",
            "Hyphen fallback Claude project imported.",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search no-cwd claude");
    assert!(
        fallback_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&fallback_search.stderr)
    );
    let fallback_search_json: serde_json::Value =
        serde_json::from_slice(&fallback_search.stdout).expect("fallback search JSON");
    assert_eq!(fallback_search_json["total_results"].as_u64().unwrap(), 1);

    let unrelated_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "claude",
            "find",
            "Unrelated Claude project should not import.",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search unrelated claude");
    assert!(
        unrelated_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&unrelated_search.stderr)
    );
    let unrelated_search_json: serde_json::Value =
        serde_json::from_slice(&unrelated_search.stdout).expect("unrelated search JSON");
    assert_eq!(unrelated_search_json["total_results"].as_u64().unwrap(), 0);

    let cwd_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "claude",
            "find",
            project.to_str().expect("project path UTF-8"),
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search claude project path leakage");
    assert!(
        cwd_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&cwd_search.stderr)
    );
    let cwd_search_json: serde_json::Value =
        serde_json::from_slice(&cwd_search.stdout).expect("cwd search JSON");
    assert_eq!(cwd_search_json["total_results"].as_u64().unwrap(), 0);

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let cursor = store
        .source_cursor(
            &project_record.id,
            "claude",
            sessions_dir
                .join("session.jsonl")
                .to_str()
                .expect("cursor key UTF-8"),
        )
        .expect("cursor read")
        .expect("cursor");
    assert_eq!(cursor.parser_version, ClaudeAdapter::PARSER_VERSION);
    assert!(cursor.last_event_hash.is_some());
}

#[test]
fn cursor_import_cli_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-cursor-project");
    let other_project = tmp.path().join("other-cursor-project");
    let cursor_root = home.join(".cursor");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&other_project).expect("create other project");
    let project = std::fs::canonicalize(&project).expect("canonical project");
    let other_project = std::fs::canonicalize(&other_project).expect("canonical other project");
    let sessions_dir = cursor_root
        .join("projects")
        .join(encode_cursor_project_name(&project))
        .join("agent-transcripts")
        .join("cursor-agent-rollout-1");
    let other_sessions_dir = cursor_root
        .join("projects")
        .join(encode_cursor_project_name(&other_project))
        .join("agent-transcripts")
        .join("cursor-agent-other");
    std::fs::create_dir_all(&sessions_dir).expect("create cursor sessions");
    std::fs::create_dir_all(&other_sessions_dir).expect("create other cursor sessions");
    let matching_rollout = include_str!("fixtures/memory_fabric/cursor_agent_session.jsonl")
        .replace(
            "/Users/test/memory-fabric",
            project.to_str().expect("project path UTF-8"),
        );
    std::fs::write(sessions_dir.join("session.jsonl"), matching_rollout)
        .expect("write cursor rollout");
    std::fs::write(
        sessions_dir.join("flat.jsonl"),
        r#"{"role":"user","content":"Flat Cursor import found by encoded project directory.","timestamp":"2026-05-24T14:40:00Z","id":"flat-cursor-u"}
"#,
    )
    .expect("write flat cursor rollout");
    let unrelated_rollout = include_str!("fixtures/memory_fabric/cursor_agent_session.jsonl")
        .replace(
            "Plan the Cursor importer.",
            "Unrelated Cursor project should not import.",
        );
    std::fs::write(
        other_sessions_dir.join("other-session.jsonl"),
        unrelated_rollout,
    )
    .expect("write unrelated cursor rollout");

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            cursor_root.to_str().expect("cursor root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("cursor import");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("import JSON");
    assert_eq!(json["source"], "cursor");
    assert_eq!(json["discovered_sessions"].as_u64().unwrap(), 2);
    assert_eq!(json["imported_events"].as_u64().unwrap(), 7);
    assert!(json["warnings"].as_array().unwrap().is_empty());

    let replay = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            cursor_root.to_str().expect("cursor root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("cursor import replay");
    assert!(
        replay.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_json: serde_json::Value =
        serde_json::from_slice(&replay.stdout).expect("replay JSON");
    assert_eq!(replay_json["imported_events"].as_u64().unwrap(), 0);

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "find",
            "CursorAdapter",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search imported cursor");
    assert!(
        search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("search JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 1);
    assert_eq!(search_json["results"][0]["source"], "cursor");

    let flat_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "find",
            "Flat Cursor import found by encoded project directory.",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search flat cursor");
    assert!(
        flat_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&flat_search.stderr)
    );
    let flat_search_json: serde_json::Value =
        serde_json::from_slice(&flat_search.stdout).expect("flat search JSON");
    assert_eq!(flat_search_json["total_results"].as_u64().unwrap(), 1);

    let unrelated_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "find",
            "Unrelated Cursor project should not import.",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search unrelated cursor");
    assert!(
        unrelated_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&unrelated_search.stderr)
    );
    let unrelated_search_json: serde_json::Value =
        serde_json::from_slice(&unrelated_search.stdout).expect("unrelated search JSON");
    assert_eq!(unrelated_search_json["total_results"].as_u64().unwrap(), 0);

    let cwd_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "find",
            project.to_str().expect("project path UTF-8"),
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search cursor project path leakage");
    assert!(
        cwd_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&cwd_search.stderr)
    );
    let cwd_search_json: serde_json::Value =
        serde_json::from_slice(&cwd_search.stdout).expect("cwd search JSON");
    assert_eq!(cwd_search_json["total_results"].as_u64().unwrap(), 0);

    let store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project_record = store
        .project_by_path(&project)
        .expect("project lookup")
        .expect("project");
    let cursor = store
        .source_cursor(
            &project_record.id,
            "cursor",
            sessions_dir
                .join("session.jsonl")
                .to_str()
                .expect("cursor key UTF-8"),
        )
        .expect("cursor read")
        .expect("cursor");
    assert_eq!(cursor.parser_version, CursorAdapter::PARSER_VERSION);
    assert!(cursor.last_event_hash.is_some());

    let flat_root = tmp.path().join("flat-cursor-root");
    std::fs::create_dir_all(&flat_root).expect("create flat cursor root");
    std::fs::write(
        flat_root.join("direct.jsonl"),
        r#"{"role":"user","content":"Direct flat Cursor root imports without row cwd.","timestamp":"2026-05-24T14:45:00Z","id":"direct-cursor-u"}
"#,
    )
    .expect("write direct flat cursor rollout");
    let flat_import = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "ingest",
            "events",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--source-root",
            flat_root.to_str().expect("flat cursor root UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("flat cursor import");
    assert!(
        flat_import.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&flat_import.stderr)
    );
    let flat_import_json: serde_json::Value =
        serde_json::from_slice(&flat_import.stdout).expect("flat import JSON");
    assert_eq!(flat_import_json["discovered_sessions"].as_u64().unwrap(), 1);
    assert_eq!(flat_import_json["imported_events"].as_u64().unwrap(), 1);

    let direct_search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "cursor",
            "find",
            "Direct flat Cursor root imports without row cwd.",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("search direct flat cursor");
    assert!(
        direct_search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&direct_search.stderr)
    );
    let direct_search_json: serde_json::Value =
        serde_json::from_slice(&direct_search.stdout).expect("direct search JSON");
    assert_eq!(direct_search_json["total_results"].as_u64().unwrap(), 1);
}

#[test]
fn summary_generation_contract_is_implemented() {
    if !loopback_bind_available() {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(
        home.join(".claude")
            .join("projects")
            .join("-Users-test-proj"),
    )
    .expect("create claude fixture dir");
    std::fs::create_dir_all(home.join(".codex").join("sessions"))
        .expect("create codex fixture dir");
    std::fs::write(
        home.join(".claude")
            .join("projects")
            .join("-Users-test-proj")
            .join("sess-claude-summary.jsonl"),
        r#"{"type":"user","sessionId":"sess-claude-summary","message":{"role":"user","content":"hello from claude summary"},"timestamp":"2025-01-01T00:00:00","uuid":"u1","cwd":"/Users/test/proj"}
{"type":"assistant","sessionId":"sess-claude-summary","message":{"role":"assistant","content":"hi from claude summary"},"timestamp":"2025-01-01T00:01:00","uuid":"a1","parentUuid":"u1","cwd":"/Users/test/proj"}"#,
    )
    .expect("write claude summary fixture");
    std::fs::write(
        home.join(".codex")
            .join("sessions")
            .join("sess-codex-summary.jsonl"),
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-summary","cwd":"/Users/test/proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"hello from codex summary"}}
{"type":"response_item","timestamp":"2025-01-02T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"hi from codex summary"}]}}"#,
    )
    .expect("write codex summary fixture");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");
    let captured: std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_for_thread = std::sync::Arc::clone(&captured);
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut bytes = Vec::new();
        let mut header_end = None;
        let mut content_length = 0usize;
        loop {
            let mut chunk = [0_u8; 4096];
            let read = std::io::Read::read(&mut stream, &mut chunk).expect("read request");
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
            if header_end.is_none()
                && let Some(idx) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
            {
                header_end = Some(idx + 4);
                let header = String::from_utf8_lossy(&bytes[..idx + 4]);
                content_length = header
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
            }
            if let Some(end) = header_end
                && bytes.len() >= end + content_length
            {
                break;
            }
        }
        let request = String::from_utf8(bytes).expect("request UTF-8");
        let body = request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or_default();
        let body_json: serde_json::Value = serde_json::from_str(body).expect("request JSON body");
        *captured_for_thread.lock().expect("lock captured body") = Some(body_json);
        let response = r#"{"id":"summary-interaction","model":"test-model","choices":[{"message":{"role":"assistant","content":"summary output"}}]}"#;
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        );
        std::io::Write::write_all(&mut stream, http_response.as_bytes()).expect("write response");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ])
        .env("HOME", &home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("MMR_CONFIG_FILE")
        .env("OPENAI_API_KEY", "test-key")
        .env("OPENAI_BASE_URL", base_url.as_str())
        .output()
        .expect("summary generation");

    assert_success_ref(&output);
    handle.join().expect("mock server thread");
    let stdout_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("summary stdout JSON");
    assert_eq!(stdout_json["backend"], "openai-compatible");
    assert_eq!(stdout_json["model"], "test-model");
    assert_eq!(stdout_json["text"], "summary output");
    assert!(stdout_json.get("thread_or_interaction_id").is_none());

    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(input.contains("hello from claude summary"));
    assert!(input.contains("hello from codex summary"));
    assert!(system_message_text(&body).contains("Memory Agent"));
}

#[test]
fn summarize_config_api_key_contract_is_implemented() {
    if !loopback_bind_available() {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(
        home.join(".claude")
            .join("projects")
            .join("-Users-test-proj"),
    )
    .expect("create claude fixture dir");
    std::fs::write(
        home.join(".claude")
            .join("projects")
            .join("-Users-test-proj")
            .join("sess-claude-summary-config.jsonl"),
        r#"{"type":"user","sessionId":"sess-claude-summary-config","message":{"role":"user","content":"config contract question"},"timestamp":"2025-01-01T00:00:00","uuid":"u1","cwd":"/Users/test/proj"}
{"type":"assistant","sessionId":"sess-claude-summary-config","message":{"role":"assistant","content":"config contract answer"},"timestamp":"2025-01-01T00:01:00","uuid":"a1","parentUuid":"u1","cwd":"/Users/test/proj"}"#,
    )
    .expect("write claude fixture");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).expect("read request");
        let response = r#"{"id":"config-contract","model":"gpt-5.5","choices":[{"message":{"role":"assistant","content":"apiKeyEnv contract summary"}}]}"#;
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        );
        std::io::Write::write_all(&mut stream, http_response.as_bytes()).expect("write response");
    });

    mmr::config::write_summarize_config_for_tests_with_api(
        &home,
        base_url.as_str(),
        "gpt-5.5",
        None,
        Some("MMR_SUMMARY_API_KEY"),
    )
    .expect("write summarize config");

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ])
        .env("HOME", &home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("MMR_CONFIG_FILE")
        .env_remove("OPENAI_API_KEY")
        .env("MMR_SUMMARY_API_KEY", "contract-key")
        .output()
        .expect("summarize with apiKeyEnv config");

    assert_success_ref(&output);
    handle.join().expect("mock server thread");
    let stdout_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("summary stdout JSON");
    assert_eq!(stdout_json["model"], "gpt-5.5");
    assert_eq!(stdout_json["text"], "apiKeyEnv contract summary");
}

#[test]
fn optional_external_summary_provider_smoke_is_gated() {
    if std::env::var("MMR_RUN_EXTERNAL_SUMMARY_SMOKE")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }

    let has_openai_key = std::env::var("OPENAI_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    assert!(
        has_openai_key,
        "set OPENAI_API_KEY with MMR_RUN_EXTERNAL_SUMMARY_SMOKE=1"
    );

    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let project = tmp.path().join("external-summary-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    let project = std::fs::canonicalize(&project).expect("canonical project");
    seed_release_gate_sources(&home, &project);

    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "summarize",
            "project",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "-O",
            "json",
        ])
        .env("HOME", &home)
        .env("SIMPLEMMR_HOME", &home)
        .output()
        .expect("external summary smoke");
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("external summary JSON");
    assert_eq!(json["backend"], "openai-compatible");
    assert!(
        !json["text"].as_str().unwrap_or_default().trim().is_empty(),
        "external summary smoke should return non-empty text"
    );
}

#[test]
fn optional_external_dream_command_smoke_is_gated() {
    if std::env::var("MMR_RUN_EXTERNAL_DREAM_SMOKE")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("external-dream-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    let project = std::fs::canonicalize(&project).expect("canonical project");
    seed_release_gate_sources(&home, &project);

    let link = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("SIMPLEMMR_HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("external dream link");
    assert_success_ref(&link);

    let dream = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["assimilate", "project"])
        .env("HOME", &home)
        .env("SIMPLEMMR_HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .env("MMR_DREAM_COMMAND", "false")
        .current_dir(&project)
        .output()
        .expect("external dream guide smoke");
    assert_success_ref(&dream);
    let json: serde_json::Value =
        serde_json::from_slice(&dream.stdout).expect("external dream JSON");
    assert_eq!(json["command"], "assimilate/project");
    assert_eq!(json["mode"], "prompt_runbook");
    assert!(json["evidence"]["included_events"].as_u64().unwrap() > 0);
    assert!(
        json["system_prompt"]
            .as_str()
            .unwrap()
            .contains("deduplication"),
        "dream should return guidance for the calling agent"
    );
}

#[test]
fn dream_assimilation_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let project_dir = tmp.path().join("dream-assimilation");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let mut store = Store::open_in_memory().expect("store");
    let project = store.ensure_project_link(&project_dir).expect("project");
    let event = store
        .insert_event(
            &project.id,
            &mmr::store::NewEvent::new(
                "note",
                "notes",
                "note",
                "user",
                "2026-05-24T16:00:00Z",
                "durable dream evidence",
                "note-v1",
            )
            .with_source_event_id("dream-assimilation-evidence"),
        )
        .expect("evidence event");
    let evidence_ref = format!("mmr://event/{}", event.id);
    let run = store
        .start_dream_run(&project.id, "mock", "mock", "sha256:evidence")
        .expect("start dream run");
    let persisted = store
        .complete_dream_run(
            &run.id,
            "sha256:output",
            &[NewDreamCandidate {
                kind: "preference".to_string(),
                claim: "Prefer durable assimilation checks.".to_string(),
                confidence: 0.93,
                evidence_refs: vec![evidence_ref.clone()],
                counterevidence_refs: Vec::new(),
                status: "accepted".to_string(),
            }],
            &[NewLearnedMemory {
                kind: "preference".to_string(),
                claim: "Prefer durable assimilation checks.".to_string(),
                confidence: 0.93,
                evidence_refs: vec![evidence_ref.clone()],
                counterevidence_refs: Vec::new(),
                status: "active".to_string(),
            }],
        )
        .expect("complete dream run");
    assert_eq!(persisted.run.status, "completed");
    assert_eq!(persisted.candidates[0].status, "accepted");
    assert_eq!(persisted.learned_memory[0].status, "active");
    assert_eq!(
        persisted.learned_memory[0].evidence_refs,
        vec![evidence_ref]
    );
    let original_memory = persisted.learned_memory[0].clone();

    let duplicate = store
        .start_dream_run(&project.id, "mock", "mock", "sha256:evidence-duplicate")
        .expect("start duplicate dream run");
    let duplicate_persisted = store
        .complete_dream_run(
            &duplicate.id,
            "sha256:duplicate-output",
            &[],
            &[NewLearnedMemory {
                kind: original_memory.kind.clone(),
                claim: original_memory.claim.clone(),
                confidence: 0.99,
                evidence_refs: original_memory.evidence_refs.clone(),
                counterevidence_refs: original_memory.counterevidence_refs.clone(),
                status: "active".to_string(),
            }],
        )
        .expect("complete duplicate dream run");
    assert!(
        duplicate_persisted.learned_memory.is_empty(),
        "duplicate learned memory must not silently overwrite the original row"
    );
    let preserved = store
        .learned_memory_by_id(&original_memory.id)
        .expect("original learned memory preserved");
    assert_eq!(preserved.dream_run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(preserved.confidence, 0.93);

    let second = store
        .start_dream_run(&project.id, "mock", "mock", "sha256:evidence-2")
        .expect("start second dream run");
    let err = store
        .complete_dream_run(
            &second.id,
            "sha256:bad-output",
            &[],
            &[NewLearnedMemory {
                kind: "preference".to_string(),
                claim: "Missing evidence should not persist.".to_string(),
                confidence: 0.91,
                evidence_refs: Vec::new(),
                counterevidence_refs: Vec::new(),
                status: "active".to_string(),
            }],
        )
        .expect_err("missing evidence refs should fail atomically");
    assert!(
        err.to_string()
            .contains("requires at least one evidence ref")
    );
    assert!(
        store
            .learned_memory_for_dream_run(&second.id)
            .expect("failed run learned memory")
            .is_empty()
    );
    let failed = store
        .fail_dream_run(&second.id, Some("sha256:bad-output"))
        .expect("mark failed");
    assert_eq!(failed.status, "failed");

    let other_project_dir = tmp.path().join("other-dream-project");
    std::fs::create_dir_all(&other_project_dir).expect("other project dir");
    let other_project = store
        .ensure_project_link(&other_project_dir)
        .expect("other project");
    let other_event = store
        .insert_event(
            &other_project.id,
            &mmr::store::NewEvent::new(
                "note",
                "notes",
                "note",
                "user",
                "2026-05-24T16:01:00Z",
                "other project evidence",
                "note-v1",
            )
            .with_source_event_id("other-dream-evidence"),
        )
        .expect("other evidence event");
    let cross_project = store
        .start_dream_run(&project.id, "mock", "mock", "sha256:evidence-cross")
        .expect("start cross-project dream run");
    let err = store
        .complete_dream_run(
            &cross_project.id,
            "sha256:cross-output",
            &[],
            &[NewLearnedMemory {
                kind: "preference".to_string(),
                claim: "Cross-project evidence must not persist.".to_string(),
                confidence: 0.91,
                evidence_refs: vec![format!("mmr://event/{}", other_event.id)],
                counterevidence_refs: Vec::new(),
                status: "active".to_string(),
            }],
        )
        .expect_err("cross-project evidence refs should fail atomically");
    assert!(err.to_string().contains("missing evidence"), "err={err}");
    store
        .fail_dream_run(&cross_project.id, Some("sha256:cross-output"))
        .expect("mark cross-project run failed");

    let third = store
        .start_dream_run(&project.id, "mock", "mock", "sha256:evidence-3")
        .expect("start third dream run");
    assert!(
        store
            .complete_dream_run(
                &third.id,
                "sha256:output-3",
                &[NewDreamCandidate {
                    kind: "pattern".to_string(),
                    claim: "Queued memories remain internal.".to_string(),
                    confidence: 0.42,
                    evidence_refs: vec![format!("mmr://event/{}", event.id)],
                    counterevidence_refs: Vec::new(),
                    status: "pending".to_string(),
                }],
                &[],
            )
            .expect("complete pending run")
            .learned_memory
            .is_empty()
    );
    let pending_candidates = store
        .dream_candidates_for_run(&third.id)
        .expect("pending candidates");
    assert_eq!(pending_candidates[0].status, "pending");
}

#[test]
fn sync_manifest_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home_one = tmp.path().join("data-one");
    let data_home_two = tmp.path().join("data-two");
    let project = tmp.path().join("plain-project");
    let fresh_project = tmp.path().join("fresh-host-project");
    let remote = tmp.path().join("fake-github");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&fresh_project).expect("create fresh project");

    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg("init")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home_one)
            .env("MMR_FAKE_REMOTE_DIR", &remote)
            .env("MMR_GITHUB_USER", "fixture-user")
            .current_dir(&project)
            .output()
            .expect("link first store"),
    );
    assert_success(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(["note", "portable", "decision", "stays", "searchable"])
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home_one)
            .current_dir(&project)
            .output()
            .expect("note first store"),
    );
    let sync = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("sync")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home_one)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&project)
        .output()
        .expect("sync first store");
    assert!(
        sync.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let sync_json: serde_json::Value =
        serde_json::from_slice(&sync.stdout).expect("sync stdout JSON");
    assert_eq!(sync_json["status"], "synced");

    let store_one = Store::open(data_home_one.join("mmr").join("mmr.db")).expect("store one");
    let project_one = store_one
        .project_by_path(&project)
        .expect("project one lookup")
        .expect("project one");
    let manifests = store_one
        .sync_manifests_for_project(&project_one.id)
        .expect("sync manifests");
    let manifest_with_entries = manifests
        .iter()
        .find(|manifest| {
            store_one
                .sync_manifest_entries(&manifest.id)
                .expect("manifest entries")
                .len()
                >= 2
        })
        .expect("manifest with event and search entries");
    let entries = store_one
        .sync_manifest_entries(&manifest_with_entries.id)
        .expect("manifest entries");
    assert!(entries.iter().any(|entry| entry.entry_kind == "event"));
    assert!(
        entries
            .iter()
            .any(|entry| entry.entry_kind == "search_document")
    );
    let remote_text = remote_file_text(&remote);
    assert!(!remote_text.contains(&project.to_string_lossy().to_string()));
    assert!(!remote_text.contains(&fresh_project.to_string_lossy().to_string()));

    let hydrate = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("init")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home_two)
        .env("MMR_FAKE_REMOTE_DIR", &remote)
        .env("MMR_GITHUB_USER", "fixture-user")
        .current_dir(&fresh_project)
        .output()
        .expect("hydrate second store");
    assert!(
        hydrate.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&hydrate.stderr)
    );
    let hydrate_json: serde_json::Value =
        serde_json::from_slice(&hydrate.stdout).expect("hydrate stdout JSON");
    assert_eq!(hydrate_json["hydration"]["inserted_events"], 1);

    let search = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["find", "portable decision"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home_two)
        .current_dir(&fresh_project)
        .output()
        .expect("search hydrated store");
    assert!(
        search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("search stdout JSON");
    assert_eq!(search_json["total_results"].as_u64().unwrap(), 1);
    assert_eq!(remote_event_file_count(&remote), 1);
}

fn dream_observation_json(evidence_ref: &str, confidence: f64) -> String {
    dream_output_json(
        evidence_ref,
        "Prefer fixture-driven tests.",
        confidence,
        r#""patterns": ["evidence-linked memory"],
      "open_loops": [],
      "counterevidence_refs": [],"#,
    )
}

fn dream_output_json(
    evidence_ref: &str,
    claim: &str,
    confidence: f64,
    extra_fields: &str,
) -> String {
    let extra_fields = if extra_fields.trim().is_empty() {
        String::new()
    } else {
        format!("{extra_fields}\n      ")
    };
    format!(
        r#"{{
  "observations": [
    {{
      "kind": "preference",
      "claim": "{claim}",
      "confidence": {confidence},
      "scope": "project",
      "recommended_action": "Keep evidence refs attached.",
      {extra_fields}
      "evidence_refs": ["{evidence_ref}"]
    }}
  ],
  "learned_memory_updates": [
    {{
      "kind": "preference",
      "claim": "{claim}",
      "confidence": {confidence},
      "evidence_refs": ["{evidence_ref}"]
    }}
  ]
}}"#
    )
}

fn assert_success(output: std::process::Output) {
    assert_success_ref(&output);
}

fn assert_success_ref(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn first_input_text(body: &serde_json::Value) -> &str {
    body["messages"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["role"] == "user"))
        .and_then(|item| item["content"].as_str())
        .expect("user message text")
}

fn system_message_text(body: &serde_json::Value) -> &str {
    body["messages"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["role"] == "system"))
        .and_then(|item| item["content"].as_str())
        .expect("system message text")
}

fn start_mock_chat_completions_server(
    response_body: String,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    std::thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_for_thread = std::sync::Arc::clone(&captured);

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let request_bytes = read_http_request(&mut stream);
        let request = String::from_utf8(request_bytes).expect("request UTF-8");
        let body = request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or_default();
        let body_json: serde_json::Value = serde_json::from_str(body).expect("request JSON body");
        *captured_for_thread.lock().expect("lock captured body") = Some(body_json);

        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        std::io::Write::write_all(&mut stream, http_response.as_bytes()).expect("write response");
    });

    (format!("http://{addr}"), captured, handle)
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let mut chunk = [0_u8; 4096];
        let read = std::io::Read::read(stream, &mut chunk).expect("read request");
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);

        if header_end.is_none()
            && let Some(idx) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
        {
            header_end = Some(idx + 4);
            let header = String::from_utf8_lossy(&bytes[..idx + 4]);
            content_length = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
        }

        if let Some(end) = header_end
            && bytes.len() >= end + content_length
        {
            break;
        }
    }

    bytes
}

fn seed_release_gate_sources(home: &Path, project: &Path) {
    let project_text = project.to_str().expect("project path UTF-8");
    let codex_sessions = home.join(".codex").join("sessions");
    std::fs::create_dir_all(&codex_sessions).expect("create codex sessions");
    let codex_fixture = format!(
        r#"{{"type":"session_meta","timestamp":"2026-05-24T10:00:00","payload":{{"id":"release-codex","cwd":"{project_text}","cli_version":"1.0.0","model_provider":"openai","timestamp":"2026-05-24T10:00:00","git":{{"branch":"main"}}}}}}
{{"type":"event_msg","timestamp":"2026-05-24T10:00:01","payload":{{"type":"user_message","message":"Release Codex fixture records adapter setup."}}}}
{{"type":"response_item","timestamp":"2026-05-24T10:00:02","payload":{{"role":"assistant","content":[{{"type":"output_text","text":"Release Codex fixture is normalized safely."}}]}}}}"#
    );
    std::fs::write(codex_sessions.join("release-codex.jsonl"), codex_fixture)
        .expect("write codex release fixture");

    let claude_sessions = home
        .join(".claude")
        .join("projects")
        .join(encode_claude_project_name(project));
    std::fs::create_dir_all(&claude_sessions).expect("create claude sessions");
    let claude_fixture = format!(
        r#"{{"type":"user","sessionId":"release-claude","message":{{"role":"user","content":"Release Claude fixture records adapter setup."}},"timestamp":"2026-05-24T10:01:00Z","uuid":"release-claude-u","cwd":"{project_text}"}}
{{"type":"assistant","sessionId":"release-claude","message":{{"role":"assistant","content":"Release Claude fixture is normalized safely."}},"timestamp":"2026-05-24T10:01:01Z","uuid":"release-claude-a","parentUuid":"release-claude-u","cwd":"{project_text}"}}"#
    );
    std::fs::write(claude_sessions.join("release-claude.jsonl"), claude_fixture)
        .expect("write claude release fixture");

    let cursor_fixture = r#"{"role":"user","message":{"content":[{"type":"text","text":"Release Cursor fixture records adapter setup."}]}}
{"role":"assistant","message":{"content":[{"type":"text","text":"Release Cursor fixture is normalized safely."}]}}"#;
    let cursor_sessions = home
        .join(".cursor")
        .join("projects")
        .join(encode_cursor_project_name(project))
        .join("agent-transcripts")
        .join("release-cursor");
    std::fs::create_dir_all(&cursor_sessions).expect("create cursor sessions");
    std::fs::write(cursor_sessions.join("release-cursor.jsonl"), cursor_fixture)
        .expect("write cursor release fixture");
    let legacy_cursor_sessions = home
        .join(".cursor")
        .join("projects")
        .join(encode_claude_project_name(project))
        .join("agent-transcripts")
        .join("release-cursor");
    std::fs::create_dir_all(&legacy_cursor_sessions).expect("create legacy cursor sessions");
    std::fs::write(
        legacy_cursor_sessions.join("release-cursor.jsonl"),
        cursor_fixture,
    )
    .expect("write legacy cursor release fixture");
}

fn remote_event_file_count(remote: &Path) -> usize {
    if !remote.exists() {
        return 0;
    }
    walkdir::WalkDir::new(remote)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter(|entry| {
            entry
                .path()
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "events")
        })
        .count()
}

fn first_remote_event_file(remote: &Path) -> PathBuf {
    walkdir::WalkDir::new(remote)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .find(|entry| {
            entry
                .path()
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "events")
        })
        .unwrap_or_else(|| panic!("remote event file under {}", remote.display()))
        .into_path()
}

fn remote_file_text(remote: &Path) -> String {
    if !remote.exists() {
        return String::new();
    }
    let mut text = String::new();
    for entry in walkdir::WalkDir::new(remote)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        text.push_str(
            &std::fs::read_to_string(entry.path())
                .unwrap_or_else(|err| panic!("read remote file {}: {err}", entry.path().display())),
        );
        text.push('\n');
    }
    text
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
