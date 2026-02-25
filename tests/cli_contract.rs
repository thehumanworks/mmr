mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{TestFixture, parse_stdout_json};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
}

fn run_cli_with_home(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run mmr")
}

fn seed_message_count_sort_fixture(home: &Path) {
    // session-a: newest timestamp, smallest message count (2)
    write_file(
        &home.join(".codex").join("sessions").join("session-a.jsonl"),
        r#"{"type":"session_meta","timestamp":"2025-01-03T00:00:00","payload":{"id":"session-a","cwd":"/Users/test/sort-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-03T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-03T00:00:01","payload":{"type":"user_message","message":"a1"}}
{"type":"response_item","timestamp":"2025-01-03T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"a2"}]}}"#,
    );

    // session-b: oldest timestamp, largest message count (5)
    write_file(
        &home.join(".codex").join("sessions").join("session-b.jsonl"),
        r#"{"type":"session_meta","timestamp":"2025-01-01T00:00:00","payload":{"id":"session-b","cwd":"/Users/test/sort-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-01T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-01T00:00:01","payload":{"type":"user_message","message":"b1"}}
{"type":"response_item","timestamp":"2025-01-01T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"b2"}]}}
{"type":"event_msg","timestamp":"2025-01-01T00:00:03","payload":{"type":"user_message","message":"b3"}}
{"type":"response_item","timestamp":"2025-01-01T00:00:04","payload":{"role":"assistant","content":[{"type":"output_text","text":"b4"}]}}
{"type":"event_msg","timestamp":"2025-01-01T00:00:05","payload":{"type":"user_message","message":"b5"}}"#,
    );

    // session-c: middle timestamp, middle message count (3)
    write_file(
        &home.join(".codex").join("sessions").join("session-c.jsonl"),
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"session-c","cwd":"/Users/test/sort-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"c1"}}
{"type":"response_item","timestamp":"2025-01-02T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"c2"}]}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:03","payload":{"type":"user_message","message":"c3"}}"#,
    );
}

// --- projects ---

#[test]
fn projects_without_source_returns_all_sources() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["projects"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 10);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 4);
    // 3 codex projects + 1 claude project = we should see both sources
    let projects = json["projects"].as_array().unwrap();
    let sources: Vec<&str> = projects
        .iter()
        .map(|p| p["source"].as_str().unwrap())
        .collect();
    assert!(sources.contains(&"codex"), "should include codex projects");
    assert!(
        sources.contains(&"claude"),
        "should include claude projects"
    );
}

#[test]
fn projects_with_source_codex_filters() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "projects"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 8);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 3);
    let projects = json["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 2);
    for project in projects {
        assert_eq!(project["source"].as_str().unwrap(), "codex");
    }
}

#[test]
fn projects_sort_and_pagination() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "projects",
        "-s",
        "message-count",
        "-o",
        "desc",
        "--limit",
        "1",
        "--offset",
        "0",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let projects = json["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(
        projects[0]["name"].as_str().unwrap(),
        "/Users/test/codex-proj"
    );
}

#[test]
fn projects_sort_by_message_count_asc_is_monotonic() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "projects",
        "--sort-by",
        "message-count",
        "-o",
        "asc",
        "--limit",
        "10",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let counts = json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["message_count"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert!(counts.windows(2).all(|window| window[0] <= window[1]));
}

#[test]
fn source_all_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "all", "projects"]);
    assert!(
        !output.status.success(),
        "--source all should not be accepted"
    );
}

// --- sessions ---

#[test]
fn sessions_without_any_filters_returns_all() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["sessions"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 4);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 4);
    // Sessions should have per-item source and project metadata
    for session in sessions {
        assert!(
            !session["source"].as_str().unwrap().is_empty(),
            "each session must have a source"
        );
        assert!(
            !session["project_name"].as_str().unwrap().is_empty(),
            "each session must have a project_name"
        );
    }
}

#[test]
fn sessions_with_source_only() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "sessions"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 3);
    let sessions = json["sessions"].as_array().unwrap();
    for session in sessions {
        assert_eq!(session["source"].as_str().unwrap(), "codex");
    }
}

#[test]
fn sessions_with_project_only_searches_both_sources() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["sessions", "--project", "Users/test/codex-proj"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    // Should find the codex sessions via path normalization even without --source
    assert_eq!(sessions.len(), 2);
    for session in sessions {
        assert_eq!(
            session["project_name"].as_str().unwrap(),
            "/Users/test/codex-proj"
        );
    }
}

#[test]
fn sessions_with_source_and_project() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "sessions",
        "--project",
        "Users/test/codex-proj",
        "-s",
        "message-count",
        "-o",
        "desc",
        "--limit",
        "1",
        "--offset",
        "1",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"].as_str().unwrap(), "sess-codex-1");
    assert_eq!(sessions[0]["source"].as_str().unwrap(), "codex");
}

#[test]
fn sessions_sort_by_message_count_desc_is_monotonic() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("create HOME");
    seed_message_count_sort_fixture(&home);

    let output = run_cli_with_home(
        &home,
        &[
            "--source",
            "codex",
            "sessions",
            "--project",
            "/Users/test/sort-proj",
            "-s",
            "message-count",
            "-o",
            "desc",
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    let ids = sessions
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["session-b", "session-c", "session-a"]);
}

#[test]
fn sessions_sort_by_message_count_asc_is_monotonic() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("create HOME");
    seed_message_count_sort_fixture(&home);

    let output = run_cli_with_home(
        &home,
        &[
            "--source",
            "codex",
            "sessions",
            "--project",
            "/Users/test/sort-proj",
            "-s",
            "message-count",
            "-o",
            "asc",
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    let ids = sessions
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["session-a", "session-c", "session-b"]);
}

#[test]
fn sessions_sort_by_message_count_asc_long_flag_matches_expected_order() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("create HOME");
    seed_message_count_sort_fixture(&home);

    let output = run_cli_with_home(
        &home,
        &[
            "--source",
            "codex",
            "sessions",
            "--project",
            "/Users/test/sort-proj",
            "--sort-by",
            "message-count",
            "-o",
            "asc",
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    let ids = sessions
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["session-a", "session-c", "session-b"]);
}

// --- messages ---

#[test]
fn messages_with_session_are_chronological_and_paginated() {
    let fixture = TestFixture::seeded();

    let all_output = fixture.run_cli(&["messages", "--session", "sess-claude-1"]);
    assert!(
        all_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&all_output.stderr)
    );
    let all_json = parse_stdout_json(&all_output);
    let all_messages = all_json["messages"].as_array().unwrap();
    assert_eq!(all_messages.len(), 2);
    assert_eq!(all_messages[0]["role"].as_str().unwrap(), "user");
    assert_eq!(all_messages[1]["role"].as_str().unwrap(), "assistant");
    // Per-message metadata
    assert_eq!(
        all_messages[0]["session_id"].as_str().unwrap(),
        "sess-claude-1"
    );
    assert_eq!(all_messages[0]["source"].as_str().unwrap(), "claude");

    // Pagination: offset 1 from newest → should get the first (oldest) message
    let paged_output = fixture.run_cli(&[
        "messages",
        "--session",
        "sess-claude-1",
        "--limit",
        "1",
        "--offset",
        "1",
    ]);
    assert!(
        paged_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&paged_output.stderr)
    );
    let paged_json = parse_stdout_json(&paged_output);
    let paged_messages = paged_json["messages"].as_array().unwrap();
    assert_eq!(paged_messages.len(), 1);
    assert_eq!(paged_messages[0]["role"].as_str().unwrap(), "user");
}

#[test]
fn messages_order_desc_returns_newest_first() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["messages", "--session", "sess-claude-1", "-o", "desc"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"].as_str().unwrap(), "assistant");
    assert_eq!(messages[1]["role"].as_str().unwrap(), "user");
}

#[test]
fn messages_sort_by_message_count_is_supported() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "-s",
        "message-count",
        "-o",
        "desc",
        "--limit",
        "1",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["session_id"].as_str().unwrap(), "sess-codex-2");
}

#[test]
fn messages_without_session_returns_all() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["messages"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 10);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 10);
}

#[test]
fn messages_filtered_by_source() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "claude", "messages"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    let messages = json["messages"].as_array().unwrap();
    for msg in messages {
        assert_eq!(msg["source"].as_str().unwrap(), "claude");
    }
}

#[test]
fn messages_filtered_by_project() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    // sess-codex-1 (2 msgs) + sess-codex-2 (4 msgs) = 6 messages for codex-proj
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    for msg in messages {
        assert_eq!(
            msg["project_name"].as_str().unwrap(),
            "/Users/test/codex-proj"
        );
    }
}
