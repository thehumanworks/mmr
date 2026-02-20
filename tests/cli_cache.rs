use duckdb::{params, Connection};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn append_file(path: &Path, contents: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
}

fn run_cli(args: &[&str], home: &Path, db_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(args)
        .env("HOME", home)
        .env("MMR_DB_PATH", db_path)
        .output()
        .unwrap()
}

fn projects_total_messages(output: &Output) -> i64 {
    assert!(
        output.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    json["total_messages"].as_i64().unwrap()
}

fn seed_small_fixture(home: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let claude_session = home
        .join(".claude")
        .join("projects")
        .join("-Users-test-proj")
        .join("sess-claude-1.jsonl");
    write_file(
        &claude_session,
        r#"{"type":"user","sessionId":"sess-claude-1","message":{"role":"user","content":"hello from claude"},"timestamp":"2025-01-01T00:00:00","uuid":"u1"}
{"type":"assistant","sessionId":"sess-claude-1","message":{"role":"assistant","content":"hi from assistant","model":"claude-3-opus","usage":{"input_tokens":100,"output_tokens":50}},"timestamp":"2025-01-01T00:01:00","uuid":"a1","parentUuid":"u1"}"#,
    );

    let codex_session = home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"hello from codex"}}
{"type":"response_item","timestamp":"2025-01-02T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"hi from codex assistant"}]}}"#,
    );

    (claude_session, codex_session)
}

#[test]
fn cli_projects_auto_builds_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let out = run_cli(&["projects"], &home, &db_path);
    let total_messages = projects_total_messages(&out);
    assert_eq!(total_messages, 2);
    assert!(
        db_path.exists(),
        "expected cache db at {}",
        db_path.display()
    );
}

#[test]
fn cli_projects_auto_refreshes_incremental_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let (claude_session, _) = seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");

    let first = run_cli(&["--source", "claude", "projects"], &home, &db_path);
    assert_eq!(projects_total_messages(&first), 2);

    append_file(
        &claude_session,
        "\n{\"type\":\"user\",\"sessionId\":\"sess-claude-1\",\"message\":{\"role\":\"user\",\"content\":\"new question\"},\"timestamp\":\"2025-01-01T00:02:00\",\"uuid\":\"u2\"}\n{\"type\":\"assistant\",\"sessionId\":\"sess-claude-1\",\"message\":{\"role\":\"assistant\",\"content\":\"new answer\",\"model\":\"claude-3-opus\",\"usage\":{\"input_tokens\":80,\"output_tokens\":30}},\"timestamp\":\"2025-01-01T00:03:00\",\"uuid\":\"a2\",\"parentUuid\":\"u2\"}",
    );

    let second = run_cli(&["--source", "claude", "projects"], &home, &db_path);
    assert_eq!(projects_total_messages(&second), 4);

    let third = run_cli(&["--source", "claude", "projects"], &home, &db_path);
    assert_eq!(projects_total_messages(&third), 4);
}

#[test]
fn incremental_state_tracks_offsets_and_last_message() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let (claude_session, _) = seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let out = run_cli(&["--source", "claude", "projects"], &home, &db_path);
    assert_eq!(projects_total_messages(&out), 2);

    let conn = Connection::open(&db_path).unwrap();

    let ingest_files_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ingest_files", [], |row| row.get(0))
        .unwrap();
    assert!(ingest_files_count >= 2);

    let file_path = claude_session.to_string_lossy().to_string();
    let (last_offset, last_ts, last_key): (i64, String, String) = conn
        .query_row(
            "SELECT last_offset, last_message_timestamp, last_message_key FROM ingest_files WHERE source = 'claude' AND file_path = ?",
            params![file_path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert!(last_offset > 0);
    assert!(!last_ts.is_empty());
    assert!(!last_key.is_empty());

    let ingest_sessions_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ingest_sessions", [], |row| row.get(0))
        .unwrap();
    assert!(ingest_sessions_count >= 2);
}

#[test]
fn incremental_refresh_removes_deleted_source_files() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let (_, codex_session) = seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");

    let first = run_cli(&["--source", "all", "projects"], &home, &db_path);
    assert_eq!(projects_total_messages(&first), 4);

    fs::remove_file(codex_session).unwrap();

    let second = run_cli(&["--source", "all", "projects"], &home, &db_path);
    assert_eq!(projects_total_messages(&second), 2);
}

#[test]
fn cli_sessions_normalizes_codex_project_without_leading_slash() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let _ = run_cli(&["projects"], &home, &db_path);

    let out = run_cli(
        &[
            "sessions",
            "--source",
            "codex",
            "--project",
            "Users/test/codex-proj",
        ],
        &home,
        &db_path,
    );

    assert!(
        out.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["project_name"].as_str().unwrap(),
        "/Users/test/codex-proj"
    );
    assert_eq!(json["sessions"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_sessions_defaults_source_to_codex() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let _ = run_cli(&["projects"], &home, &db_path);

    let out = run_cli(
        &["sessions", "--project", "Users/test/codex-proj"],
        &home,
        &db_path,
    );

    assert!(
        out.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["source"].as_str().unwrap(), "codex");
    assert_eq!(
        json["project_name"].as_str().unwrap(),
        "/Users/test/codex-proj"
    );
    assert_eq!(json["sessions"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_projects_sessions_messages_support_limit_and_offset_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let _ = run_cli(&["projects"], &home, &db_path);

    let projects = run_cli(
        &[
            "--source", "all", "projects", "--limit", "1", "--offset", "1",
        ],
        &home,
        &db_path,
    );
    assert!(
        projects.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&projects.stderr),
        String::from_utf8_lossy(&projects.stdout)
    );
    let projects_json: serde_json::Value = serde_json::from_slice(&projects.stdout).unwrap();
    assert_eq!(projects_json["projects"].as_array().unwrap().len(), 1);
    assert_eq!(
        projects_json["projects"][0]["name"].as_str().unwrap(),
        "-Users-test-proj"
    );

    let sessions = run_cli(
        &[
            "sessions",
            "--source",
            "codex",
            "--project",
            "/Users/test/codex-proj",
            "--limit",
            "1",
            "--offset",
            "0",
        ],
        &home,
        &db_path,
    );
    assert!(
        sessions.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&sessions.stderr),
        String::from_utf8_lossy(&sessions.stdout)
    );
    let sessions_json: serde_json::Value = serde_json::from_slice(&sessions.stdout).unwrap();
    assert_eq!(sessions_json["sessions"].as_array().unwrap().len(), 1);

    let messages = run_cli(
        &[
            "messages",
            "--session",
            "sess-claude-1",
            "--limit",
            "1",
            "--offset",
            "1",
        ],
        &home,
        &db_path,
    );
    assert!(
        messages.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&messages.stderr),
        String::from_utf8_lossy(&messages.stdout)
    );
    let messages_json: serde_json::Value = serde_json::from_slice(&messages.stdout).unwrap();
    assert_eq!(messages_json["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        messages_json["messages"][0]["role"].as_str().unwrap(),
        "user"
    );
}

#[test]
fn cli_messages_prints_chronological_order() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let _ = run_cli(&["projects"], &home, &db_path);

    let out = run_cli(&["messages", "--session", "sess-claude-1"], &home, &db_path);
    assert!(
        out.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
    assert_eq!(messages[1]["role"].as_str().unwrap(), "assistant");
}

#[test]
fn cli_messages_repairs_missing_session_and_project_rows_on_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    seed_small_fixture(&home);

    let db_path = tmp.path().join("cache.duckdb");
    let first = run_cli(&["projects"], &home, &db_path);
    assert!(
        first.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&first.stderr),
        String::from_utf8_lossy(&first.stdout)
    );

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "DELETE FROM sessions WHERE session_id = 'sess-claude-1' AND project = '-Users-test-proj' AND source = 'claude'",
            [],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM projects WHERE name = '-Users-test-proj' AND source = 'claude'",
            [],
        )
        .unwrap();
    }

    let out = run_cli(&["messages", "--session", "sess-claude-1"], &home, &db_path);
    assert!(
        out.status.success(),
        "command failed, stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["session_id"].as_str().unwrap(), "sess-claude-1");
    assert_eq!(json["project_name"].as_str().unwrap(), "-Users-test-proj");
    assert_eq!(json["source"].as_str().unwrap(), "claude");
    assert_eq!(json["messages"].as_array().unwrap().len(), 2);

    let conn = Connection::open(&db_path).unwrap();
    let (session_count, project_count): (i64, i64) = conn
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM sessions WHERE session_id = 'sess-claude-1' AND project = '-Users-test-proj' AND source = 'claude'),
                (SELECT COUNT(*) FROM projects WHERE name = '-Users-test-proj' AND source = 'claude')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(session_count, 1);
    assert_eq!(project_count, 1);
}
