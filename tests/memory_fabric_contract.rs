use mmr::capture::{
    ClaudeAdapter, CodexAdapter, CursorAdapter, EventBoundary, FileWatcher, WatchState,
    event_hash_set, parse_claude_jsonl, parse_codex_jsonl, parse_cursor_jsonl, parse_fixture_jsonl,
};
use mmr::store::{LATEST_SCHEMA_VERSION, Store};
#[allow(dead_code)]
mod common;
use common::RetrieveContractFixture;
use std::collections::BTreeSet;
use std::io::Write;
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
    for command in ["init", "sync", "status", "note", "find", "summarize"] {
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

    let status_help = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["status", "--help"])
        .output()
        .expect("status help");
    assert_success_ref(&status_help);
    let status_help_text = String::from_utf8(status_help.stdout).expect("status help UTF-8");
    assert!(status_help_text.contains("db_path"));
    assert!(status_help_text.contains("schema_version"));
    assert!(status_help_text.contains("diagnostics"));
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
fn retrieve_store_to_provider_mapping_uses_public_source_session_id() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&[
        "retrieve",
        "public mapping marker",
        "--full-message-history",
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

fn retrieve_window_loads(path: &Path) -> Vec<(String, String, String)> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    contents
        .lines()
        .map(|line| {
            let parts = line.splitn(3, '\t').collect::<Vec<_>>();
            assert_eq!(
                parts.len(),
                3,
                "invalid retrieve window load log line: {line}"
            );
            (
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
            )
        })
        .collect()
}

#[test]
fn retrieve_ranks_before_provider_window_loading() {
    let fixture = RetrieveContractFixture::seeded();
    let log_path = fixture.data_home.join("retrieve-window-loads.log");
    let log_arg = log_path.to_str().expect("window load log path UTF-8");
    let output = fixture.run_cli_with_env(
        &[
            "retrieve",
            "ranking tie marker",
            "--full-message-history",
            "--max-sessions",
            "1",
            "--max-messages-per-session",
            "4",
            "--limit",
            "4",
        ],
        &[("MMR_TEST_RETRIEVE_WINDOW_LOAD_LOG", log_arg)],
    );
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("ranking retrieve JSON");
    let selected = json["selected_sessions"].as_array().expect("sessions");

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["source"], "claude");
    assert_eq!(selected[0]["source_session_id"], "retrieve-claude-alpha");
    assert_eq!(
        retrieve_window_loads(&log_path),
        vec![(
            "claude".to_string(),
            fixture.project_arg().to_string(),
            "retrieve-claude-alpha".to_string(),
        )]
    );
}

#[test]
fn retrieve_default_output_loads_windows_only_for_selected_sessions() {
    let fixture = RetrieveContractFixture::seeded();
    fixture.add_newer_matching_session("ranking tie marker");
    let log_path = fixture.data_home.join("retrieve-default-window-loads.log");
    let log_arg = log_path.to_str().expect("window load log path UTF-8");
    let output = fixture.run_cli_with_env(
        &["retrieve", "ranking tie marker", "--max-sessions", "3"],
        &[("MMR_TEST_RETRIEVE_WINDOW_LOAD_LOG", log_arg)],
    );
    assert_success_ref(&output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("default retrieve JSON");
    let selected = json["selected_sessions"].as_array().expect("sessions");

    assert_eq!(selected.len(), 3);
    assert!(
        json["total_matches"].as_u64().unwrap() > selected.len() as u64,
        "fixture should contain more matched sessions than max-sessions"
    );
    assert!(selected.iter().all(|session| {
        session.get("messages").is_none() && session.get("message_window").is_none()
    }));
    let selected_keys = selected
        .iter()
        .map(|session| {
            (
                session["source"].as_str().unwrap().to_string(),
                session["project_name"].as_str().unwrap().to_string(),
                session["source_session_id"].as_str().unwrap().to_string(),
            )
        })
        .collect::<BTreeSet<_>>();
    let loads = retrieve_window_loads(&log_path);

    assert!(
        loads.iter().all(|load| selected_keys.contains(load)),
        "discarded matched sessions loaded provider windows: loads={loads:?}, selected={selected_keys:?}"
    );
    assert!(
        !loads
            .iter()
            .any(|(_, _, session_id)| session_id == "retrieve-codex-newer"),
        "lower-ranked discarded session should not load a provider window"
    );
    assert!(
        loads.len() <= selected_keys.len(),
        "provider windows should not be loaded more often than selected sessions"
    );
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
        "--full-message-history",
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
    assert!(json.get("next_page").is_none());
    assert!(json.get("next_command").is_none());
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

    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"config-contract","model":"gpt-5.5","choices":[{"message":{"role":"assistant","content":"apiKeyEnv contract summary"}}]}"#.to_string(),
    );

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
