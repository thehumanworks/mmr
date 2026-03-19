mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use common::{TestFixture, parse_stdout_json};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout UTF-8")
}

fn first_input_text(body: &serde_json::Value) -> &str {
    body["input"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .expect("first input text")
}

fn encode_claude_project_name(cwd: &str) -> String {
    if cwd == "/" {
        "-".to_string()
    } else {
        format!("-{}", cwd.trim_start_matches('/').replace('/', "-"))
    }
}

fn run_cli_with_home(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run mmr")
}

fn run_cli_with_home_and_env(home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
    command.args(args).env("HOME", home);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run mmr")
}

fn start_mock_gemini_server(
    response_body: &str,
) -> (
    String,
    Arc<Mutex<Option<serde_json::Value>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let captured = Arc::new(Mutex::new(None));
    let captured_for_thread = Arc::clone(&captured);
    let response = response_body.to_string();

    let handle = thread::spawn(move || {
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
            response.len(),
            response
        );
        stream
            .write_all(http_response.as_bytes())
            .expect("write response");
    });

    (format!("http://{addr}"), captured, handle)
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);

        if header_end.is_none()
            && let Some(idx) = find_subsequence(&bytes, b"\r\n\r\n")
        {
            header_end = Some(idx + 4);
            let header = String::from_utf8_lossy(&bytes[..idx + 4]);
            content_length = parse_content_length(&header);
        }

        if let Some(end) = header_end
            && bytes.len() >= end + content_length
        {
            break;
        }
    }

    bytes
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_content_length(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if !name.eq_ignore_ascii_case("content-length") {
                return None;
            }
            value.trim().parse::<usize>().ok()
        })
        .unwrap_or(0)
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

fn seed_cwd_project_with_history(fixture: &TestFixture) -> PathBuf {
    let cwd = fixture.home.join("cwd-project");
    fs::create_dir_all(&cwd).expect("create cwd project dir");
    let cwd_str = fs::canonicalize(&cwd)
        .expect("canonicalize cwd project")
        .to_string_lossy()
        .into_owned();

    let codex_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-cwd-codex.jsonl");
    write_file(
        &codex_session,
        &format!(
            r#"{{"type":"session_meta","timestamp":"2025-01-07T00:00:00","payload":{{"id":"sess-cwd-codex","cwd":"{}","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-07T00:00:00","git":{{"branch":"main"}}}}}}
{{"type":"event_msg","timestamp":"2025-01-07T00:00:01","payload":{{"type":"user_message","message":"cwd codex question"}}}}
{{"type":"response_item","timestamp":"2025-01-07T00:00:02","payload":{{"role":"assistant","content":[{{"type":"output_text","text":"cwd codex answer"}}]}}}}"#,
            cwd_str
        ),
    );

    let claude_project = encode_claude_project_name(&cwd_str);
    let claude_session = fixture
        .home
        .join(".claude")
        .join("projects")
        .join(claude_project)
        .join("sess-cwd-claude.jsonl");
    write_file(
        &claude_session,
        &format!(
            r#"{{"type":"user","sessionId":"sess-cwd-claude","message":{{"role":"user","content":"cwd claude question"}},"timestamp":"2025-01-07T00:01:00","uuid":"u-cwd-1","cwd":"{}"}}
{{"type":"assistant","sessionId":"sess-cwd-claude","message":{{"role":"assistant","content":"cwd claude answer","model":"claude-3-opus","usage":{{"input_tokens":120,"output_tokens":60}}}},"timestamp":"2025-01-07T00:02:00","uuid":"a-cwd-1","parentUuid":"u-cwd-1","cwd":"{}"}}"#,
            cwd_str, cwd_str
        ),
    );

    cwd
}

fn seed_empty_discovered_project(fixture: &TestFixture) -> PathBuf {
    let cwd = fixture.home.join("empty-project");
    fs::create_dir_all(&cwd).expect("create empty project dir");
    let cwd_str = fs::canonicalize(&cwd)
        .expect("canonicalize empty project")
        .to_string_lossy()
        .into_owned();

    let codex_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-empty-project.jsonl");
    write_file(
        &codex_session,
        &format!(
            r#"{{"type":"session_meta","timestamp":"2025-01-08T00:00:00","payload":{{"id":"sess-empty-project","cwd":"{}","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-08T00:00:00","git":{{"branch":"main"}}}}}}"#,
            cwd_str
        ),
    );

    cwd
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
fn sessions_defaults_to_cwd_project_when_discovery_succeeds() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let output =
        fixture.run_cli_in_dir_with_env(&["sessions"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 2);
    assert_eq!(sessions.len(), 2);

    let session_ids = sessions
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(session_ids.contains(&"sess-cwd-codex"));
    assert!(session_ids.contains(&"sess-cwd-claude"));
}

#[test]
fn sessions_all_bypasses_cwd_discovery() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let output = fixture.run_cli_in_dir_with_env(
        &["sessions", "--all"],
        &cwd,
        &[("MMR_AUTO_DISCOVER_PROJECT", "1")],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 6);
    assert_eq!(sessions.len(), 6);

    let session_ids = sessions
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(session_ids.contains(&"sess-claude-1"));
    assert!(session_ids.contains(&"sess-codex-1"));
    assert!(session_ids.contains(&"sess-cwd-codex"));
    assert!(session_ids.contains(&"sess-cwd-claude"));
}

#[test]
fn sessions_returns_empty_for_discovered_but_empty_project() {
    let fixture = TestFixture::seeded();
    let cwd = seed_empty_discovered_project(&fixture);

    let output =
        fixture.run_cli_in_dir_with_env(&["sessions"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 0);
    assert!(sessions.is_empty());
}

#[test]
fn auto_discover_project_env_controls_default_scope_for_sessions_and_messages() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let disabled_sessions =
        fixture.run_cli_in_dir_with_env(&["sessions"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "0")]);
    assert!(
        disabled_sessions.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&disabled_sessions.stderr)
    );
    let disabled_sessions_json = parse_stdout_json(&disabled_sessions);
    assert_eq!(
        disabled_sessions_json["total_sessions"].as_i64().unwrap(),
        6
    );

    let enabled_sessions =
        fixture.run_cli_in_dir_with_env(&["sessions"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);
    assert!(
        enabled_sessions.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&enabled_sessions.stderr)
    );
    let enabled_sessions_json = parse_stdout_json(&enabled_sessions);
    assert_eq!(enabled_sessions_json["total_sessions"].as_i64().unwrap(), 2);

    let disabled_messages =
        fixture.run_cli_in_dir_with_env(&["messages"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "0")]);
    assert!(
        disabled_messages.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&disabled_messages.stderr)
    );
    let disabled_messages_json = parse_stdout_json(&disabled_messages);
    assert_eq!(
        disabled_messages_json["total_messages"].as_i64().unwrap(),
        14
    );

    let enabled_messages =
        fixture.run_cli_in_dir_with_env(&["messages"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);
    assert!(
        enabled_messages.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&enabled_messages.stderr)
    );
    let enabled_messages_json = parse_stdout_json(&enabled_messages);
    assert_eq!(enabled_messages_json["total_messages"].as_i64().unwrap(), 4);
}

#[test]
fn sessions_with_source_only() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "codex", "sessions", "--all"], &[]);

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
fn messages_defaults_to_cwd_project_when_discovery_succeeds() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let output =
        fixture.run_cli_in_dir_with_env(&["messages"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 4);
    assert_eq!(messages.len(), 4);
    assert!(messages.iter().all(|message| {
        message["session_id"]
            .as_str()
            .unwrap()
            .starts_with("sess-cwd-")
    }));
}

#[test]
fn messages_all_bypasses_cwd_discovery() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let output = fixture.run_cli_in_dir_with_env(
        &["messages", "--all"],
        &cwd,
        &[("MMR_AUTO_DISCOVER_PROJECT", "1")],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 14);
    assert_eq!(messages.len(), 14);
    assert!(
        messages
            .iter()
            .any(|message| message["session_id"].as_str().unwrap() == "sess-claude-1")
    );
    assert!(
        messages
            .iter()
            .any(|message| message["session_id"].as_str().unwrap() == "sess-cwd-codex")
    );
}

#[test]
fn messages_returns_empty_for_discovered_but_empty_project() {
    let fixture = TestFixture::seeded();
    let cwd = seed_empty_discovered_project(&fixture);

    let output =
        fixture.run_cli_in_dir_with_env(&["messages"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 0);
    assert!(messages.is_empty());
}

#[test]
fn messages_with_session_are_chronological_and_paginated() {
    let fixture = TestFixture::seeded();

    let all_output = fixture.run_cli(&["messages", "--session", "sess-claude-1", "--all"]);
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
        "--all",
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
    let output = fixture.run_cli(&[
        "messages",
        "--session",
        "sess-claude-1",
        "--all",
        "-o",
        "desc",
    ]);

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
        "--all",
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
fn messages_filtered_by_source() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "claude", "messages", "--all"], &[]);

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

#[test]
fn default_source_empty_string_keeps_both_sources() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let output = fixture.run_cli_in_dir_with_env(
        &["sessions", "--all"],
        &cwd,
        &[
            ("MMR_AUTO_DISCOVER_PROJECT", "1"),
            ("MMR_DEFAULT_SOURCE", ""),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 6);
    assert_eq!(sessions.len(), 6);
    let sources = sessions
        .iter()
        .map(|session| session["source"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(sources.contains(&"claude"));
    assert!(sources.contains(&"codex"));
}

#[test]
fn default_source_env_selects_codex_and_explicit_source_overrides_it() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let codex_output = fixture.run_cli_in_dir_with_env(
        &["sessions", "--all"],
        &cwd,
        &[
            ("MMR_AUTO_DISCOVER_PROJECT", "1"),
            ("MMR_DEFAULT_SOURCE", "codex"),
        ],
    );

    assert!(
        codex_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&codex_output.stderr)
    );

    let codex_json = parse_stdout_json(&codex_output);
    let codex_sessions = codex_json["sessions"].as_array().unwrap();
    assert_eq!(codex_json["total_sessions"].as_i64().unwrap(), 4);
    assert!(
        codex_sessions
            .iter()
            .all(|session| session["source"].as_str().unwrap() == "codex")
    );

    let override_output = fixture.run_cli_in_dir_with_env(
        &["--source", "claude", "messages", "--all"],
        &cwd,
        &[
            ("MMR_AUTO_DISCOVER_PROJECT", "1"),
            ("MMR_DEFAULT_SOURCE", "codex"),
        ],
    );

    assert!(
        override_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&override_output.stderr)
    );

    let override_json = parse_stdout_json(&override_output);
    let override_messages = override_json["messages"].as_array().unwrap();
    assert_eq!(override_json["total_messages"].as_i64().unwrap(), 4);
    assert!(
        override_messages
            .iter()
            .all(|message| message["source"].as_str().unwrap() == "claude")
    );
}

// --- export ---

#[test]
fn export_with_project_returns_all_messages_for_project() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["export", "--project", "Users/test/codex-proj"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert!(json["messages"].is_array());
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    let messages = json["messages"].as_array().unwrap();
    for msg in messages {
        assert!(msg["source"].as_str().is_some());
        assert!(msg["project_name"].as_str().is_some());
        assert!(msg["session_id"].as_str().is_some());
        assert!(msg["timestamp"].as_str().is_some());
        assert!(msg["role"].as_str().is_some());
        assert!(msg["content"].as_str().is_some());
    }
    // Ascending timestamp order
    let timestamps: Vec<&str> = messages
        .iter()
        .map(|m| m["timestamp"].as_str().unwrap())
        .collect();
    let mut sorted = timestamps.clone();
    sorted.sort();
    assert_eq!(timestamps, sorted);
}

#[test]
fn export_with_source_and_project_filters_by_source() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "export",
        "--project",
        "/Users/test/codex-proj",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    let messages = json["messages"].as_array().unwrap();
    for msg in messages {
        assert_eq!(msg["source"].as_str().unwrap(), "codex");
    }
}

#[test]
fn export_without_project_uses_cwd() {
    let fixture = TestFixture::seeded();
    let proj_dir = fixture.home.join("proj");
    fs::create_dir_all(&proj_dir).expect("create proj dir");
    let cwd_str = fs::canonicalize(&proj_dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let session_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-cwd-export.jsonl");
    write_file(
        &session_path,
        &format!(
            r#"{{"type":"session_meta","timestamp":"2025-01-04T00:00:00","payload":{{"id":"sess-cwd-export","cwd":"{}","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-04T00:00:00","git":{{"branch":"main"}}}}}}
{{"type":"event_msg","timestamp":"2025-01-04T00:00:01","payload":{{"type":"user_message","message":"cwd export test"}}}}
{{"type":"response_item","timestamp":"2025-01-04T00:01:00","payload":{{"role":"assistant","content":[{{"type":"output_text","text":"cwd export answer"}}]}}}}"#,
            cwd_str
        ),
    );

    let output = fixture.run_cli_in_dir(&["export"], &proj_dir);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert!(
        !messages.is_empty(),
        "expected at least one message from cwd project"
    );
    for msg in messages {
        assert!(msg["source"].as_str().is_some());
        assert!(msg["project_name"].as_str().is_some());
        assert!(msg["session_id"].as_str().is_some());
        assert!(msg["timestamp"].as_str().is_some());
        assert!(msg["role"].as_str().is_some());
        assert!(msg["content"].as_str().is_some());
    }
}

// --- remember ---

#[test]
fn remember_all_includes_claude_and_codex_messages() {
    let fixture = TestFixture::seeded();
    let codex_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-remember.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-04T00:00:00","payload":{"id":"sess-codex-remember","cwd":"/Users/test/proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-04T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-04T00:00:01","payload":{"type":"user_message","message":"remember codex question"}}
{"type":"response_item","timestamp":"2025-01-04T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"remember codex answer"}]}}"#,
    );

    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-1","outputs":[{"text":"continuity summary"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "all",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
            "-O",
            "json",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["agent"].as_str().unwrap(), "gemini");
    assert_eq!(stdout_json["text"].as_str().unwrap(), "continuity summary");
    assert!(
        stdout_json.get("thread_or_interaction_id").is_none(),
        "remember JSON output should not expose resumability IDs"
    );

    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(
        input.contains("hello from claude"),
        "remember input should include claude transcript"
    );
    assert!(
        input.contains("remember codex question"),
        "remember input should include codex transcript"
    );
    assert!(
        body["system_instruction"]
            .as_str()
            .unwrap()
            .contains("Memory Agent")
    );
}

#[test]
fn remember_without_selector_uses_latest_session() {
    let fixture = TestFixture::seeded();
    let codex_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-remember-latest.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-06T00:00:00","payload":{"id":"sess-codex-remember-latest","cwd":"/Users/test/proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-06T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-06T00:00:01","payload":{"type":"user_message","message":"latest session question"}}
{"type":"response_item","timestamp":"2025-01-06T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"latest session answer"}]}}"#,
    );

    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-latest","outputs":[{"text":"latest summary"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(input.contains("latest session question"));
    assert!(
        !input.contains("hello from claude"),
        "default remember selection should only include the latest session"
    );
    assert_eq!(input.matches("=== Session:").count(), 1);
}

#[test]
fn remember_defaults_to_gemini_when_env_sets_it() {
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-env-default","outputs":[{"text":"gemini env default"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &["remember", "--project", "/Users/test/proj", "-O", "json"],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
            ("MMR_DEFAULT_REMEMBER_AGENT", "gemini"),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["agent"].as_str().unwrap(), "gemini");
    assert_eq!(stdout_json["text"].as_str().unwrap(), "gemini env default");
}

#[test]
fn remember_explicit_agent_overrides_default_env() {
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-env-override","outputs":[{"text":"explicit override"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
            "-O",
            "json",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
            ("MMR_DEFAULT_REMEMBER_AGENT", "codex"),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["agent"].as_str().unwrap(), "gemini");
    assert_eq!(stdout_json["text"].as_str().unwrap(), "explicit override");
}

#[test]
fn remember_with_source_filter_only_includes_requested_source() {
    let fixture = TestFixture::seeded();
    let codex_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-remember-source.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-05T00:00:00","payload":{"id":"sess-codex-remember-source","cwd":"/Users/test/proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-05T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-05T00:00:01","payload":{"type":"user_message","message":"codex-only question"}} "#,
    );

    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-2","outputs":[{"text":"codex-only summary"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "--source",
            "codex",
            "remember",
            "all",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(input.contains("codex-only question"));
    assert!(
        !input.contains("hello from claude"),
        "claude messages should be filtered out when --source codex is used"
    );
}

#[test]
fn remember_session_selector_uses_requested_session() {
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-3","outputs":[{"text":"session summary"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "session",
            "sess-claude-1",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(input.contains("hello from claude"));
    assert!(!input.contains("remember codex question"));
    assert!(
        body["previous_interaction_id"].is_null(),
        "one-shot remember requests should not resume previous interactions"
    );
    let system_instruction = body["system_instruction"].as_str().unwrap();
    assert!(system_instruction.contains("Memory Agent"));
}

#[test]
fn remember_output_format_md_transforms_json_response_to_markdown() {
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-md","outputs":[{"text":"Status\n- Item one"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
            "-O",
            "md",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout = stdout_text(&output);
    assert!(stdout.contains("Status\n- Item one"));
    assert!(!stdout.contains("Interaction ID:"));
    assert!(!stdout.contains("Thread ID:"));
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "markdown output should not be JSON"
    );
}

#[test]
fn remember_output_format_md_trims_summary_and_interaction_id() {
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_gemini_server(
        r#"{"id":"  interaction-trim  ","outputs":[{"text":"  status line\nnext line  "}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
            "--output-format",
            "md",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout = stdout_text(&output);
    assert!(stdout.contains("status line\nnext line"));
    assert!(!stdout.contains("interaction-trim"));
}

#[test]
fn remember_custom_instructions_replace_default_output_section_in_system_prompt() {
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-custom","outputs":[{"text":"custom output"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
            "--instructions",
            "Return only a single keyword.",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let system = body["system_instruction"].as_str().unwrap();
    assert!(
        system.contains("Memory Agent"),
        "base identity must be preserved"
    );
    assert!(
        system.contains("Input Format"),
        "base input format must be preserved"
    );
    assert!(
        system.contains("Return only a single keyword."),
        "custom instructions must appear in system prompt"
    );
    assert!(
        !system.contains("Output Format"),
        "default output format must be replaced by custom instructions"
    );
    assert!(
        !system.contains("continuity brief"),
        "default purpose must be replaced by custom instructions"
    );
    assert!(
        !system.contains("Resume Instructions"),
        "default output sections must be replaced by custom instructions"
    );
}

#[test]
fn remember_without_instructions_includes_default_purpose_and_output_sections() {
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-default","outputs":[{"text":"default output"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "remember",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let system = body["system_instruction"].as_str().unwrap();
    assert!(
        system.contains("Memory Agent"),
        "base identity must be present"
    );
    assert!(
        system.contains("Input Format"),
        "input format must be present"
    );
    assert!(
        system.contains("Purpose"),
        "default Purpose section must be present"
    );
    assert!(
        system.contains("continuity brief"),
        "default purpose text must be present"
    );
    assert!(
        system.contains("Output Format"),
        "default output format must be present"
    );
    assert!(
        system.contains("Resume Instructions"),
        "default output sections must be present"
    );
}

#[test]
fn remember_rejects_legacy_flags() {
    let fixture = TestFixture::seeded();
    let legacy_invocations = [
        vec!["remember", "--mode", "all"],
        vec!["remember", "--session-id", "sess-claude-1"],
        vec!["remember", "--continue-from", "interaction-previous"],
        vec!["remember", "--follow-up", "what next?"],
    ];

    for args in legacy_invocations {
        let output = fixture.run_cli(&args);
        assert!(
            !output.status.success(),
            "legacy remember flags should be rejected: {args:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unexpected argument"),
            "stderr should report an unexpected argument for {args:?}: {stderr}"
        );
    }
}

#[test]
fn remember_rejects_session_selector_without_id() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["remember", "session"]);

    assert!(
        !output.status.success(),
        "session selector without an ID should be rejected"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided")
            || stderr.contains("a value is required"),
        "stderr should explain that the session selector requires an ID: {stderr}"
    );
}

#[test]
fn merge_session_to_session_cross_source_shifts_timestamps_and_collapses_to_codex_provider() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "merge",
        "--from-session",
        "sess-claude-1",
        "--to-session",
        "sess-codex-1",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["mode"].as_str(), Some("session-to-session"));
    assert_eq!(json["from_agent"].as_str(), Some("claude"));
    assert_eq!(json["to_agent"].as_str(), Some("codex"));
    assert_eq!(json["total_sessions_merged"].as_i64(), Some(1));
    assert_eq!(json["total_messages_merged"].as_i64(), Some(2));

    let merge = &json["session_merges"].as_array().unwrap()[0];
    assert_eq!(merge["from_session_id"].as_str(), Some("sess-claude-1"));
    assert_eq!(merge["to_session_id"].as_str(), Some("sess-codex-1"));
    assert_eq!(
        merge["timestamp_strategy"].as_str(),
        Some("shifted-to-append-after-target")
    );
    assert_eq!(
        merge["model_strategy"].as_str(),
        Some("collapsed-source-models-to-existing-codex-provider:openai")
    );

    let considerations = json["schema_considerations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item.as_str())
        .collect::<Vec<_>>();
    assert!(
        considerations
            .iter()
            .any(|item| item.contains("Codex stores model metadata at session scope")),
        "expected codex model-collapse note: {considerations:?}"
    );
    assert!(
        considerations
            .iter()
            .any(|item| item.contains("Session-to-session merges retime copied messages")),
        "expected timestamp retiming note: {considerations:?}"
    );

    let codex_messages = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--limit",
        "10",
    ]);
    assert!(
        codex_messages.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&codex_messages.stderr)
    );
    let messages_json = parse_stdout_json(&codex_messages);
    assert_eq!(messages_json["total_messages"].as_i64(), Some(4));
    let contents = messages_json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            "hello from codex",
            "short codex answer",
            "hello from claude",
            "hi from assistant"
        ]
    );
    assert_eq!(
        messages_json["messages"].as_array().unwrap()[3]["model"].as_str(),
        Some("openai")
    );

    let raw = fs::read_to_string(
        fixture
            .home
            .join(".codex")
            .join("sessions")
            .join("sess-codex-1.jsonl"),
    )
    .expect("read merged codex session");
    let lines = raw.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 5);
    let appended_user: serde_json::Value =
        serde_json::from_str(lines[3]).expect("parse appended user line");
    let appended_assistant: serde_json::Value =
        serde_json::from_str(lines[4]).expect("parse appended assistant line");
    assert_eq!(
        appended_user["timestamp"].as_str(),
        Some("2025-01-02T00:05:01")
    );
    assert_eq!(
        appended_assistant["timestamp"].as_str(),
        Some("2025-01-02T00:06:01")
    );
    assert_eq!(appended_user["type"].as_str(), Some("event_msg"));
    assert_eq!(appended_assistant["type"].as_str(), Some("response_item"));
}

#[test]
fn merge_agent_to_agent_project_creates_claude_sessions_with_transformed_models() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "merge",
        "--from-agent",
        "codex",
        "--to-agent",
        "claude",
        "--project",
        "/Users/test/codex-proj",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["mode"].as_str(), Some("agent-to-agent"));
    assert_eq!(json["from_agent"].as_str(), Some("codex"));
    assert_eq!(json["to_agent"].as_str(), Some("claude"));
    assert_eq!(json["total_sessions_merged"].as_i64(), Some(2));
    assert_eq!(json["total_messages_merged"].as_i64(), Some(6));

    let merges = json["session_merges"].as_array().unwrap();
    assert_eq!(merges.len(), 2);
    for merge in merges {
        assert_eq!(merge["created_target_session"].as_bool(), Some(true));
        assert_eq!(
            merge["to_project_name"].as_str(),
            Some("-Users-test-codex-proj")
        );
        assert_eq!(
            merge["timestamp_strategy"].as_str(),
            Some("preserved-source-timestamps")
        );
        assert_eq!(merge["to_source"].as_str(), Some("claude"));
        let model_strategy = merge["model_strategy"].as_str().unwrap();
        assert!(
            model_strategy.starts_with("expanded-codex-provider-into-claude-assistant-models:"),
            "unexpected model strategy: {model_strategy}"
        );
        let target_file = merge["target_file"].as_str().unwrap();
        assert!(
            Path::new(target_file).exists(),
            "expected target file to exist: {target_file}"
        );
    }

    let considerations = json["schema_considerations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item.as_str())
        .collect::<Vec<_>>();
    assert!(
        considerations
            .iter()
            .any(|item| item.contains("Claude stores model metadata on assistant messages")),
        "expected claude model-expansion note: {considerations:?}"
    );

    let sessions_output = fixture.run_cli(&[
        "--source",
        "claude",
        "sessions",
        "--project=-Users-test-codex-proj",
        "--limit",
        "10",
    ]);
    assert!(
        sessions_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&sessions_output.stderr)
    );
    let sessions_json = parse_stdout_json(&sessions_output);
    assert_eq!(sessions_json["total_sessions"].as_i64(), Some(2));

    let merge_for_session_one = merges
        .iter()
        .find(|merge| merge["from_session_id"].as_str() == Some("sess-codex-1"))
        .expect("merge mapping for sess-codex-1");
    let created_session_id = merge_for_session_one["to_session_id"]
        .as_str()
        .expect("created session id");
    let messages_output = fixture.run_cli(&[
        "--source",
        "claude",
        "messages",
        "--project=-Users-test-codex-proj",
        "--session",
        created_session_id,
        "--limit",
        "10",
    ]);
    assert!(
        messages_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&messages_output.stderr)
    );
    let messages_json = parse_stdout_json(&messages_output);
    let contents = messages_json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(contents, vec!["hello from codex", "short codex answer"]);
    assert_eq!(
        messages_json["messages"].as_array().unwrap()[1]["model"].as_str(),
        Some("openai")
    );

    let target_file = merge_for_session_one["target_file"]
        .as_str()
        .expect("target file");
    let raw = fs::read_to_string(target_file).expect("read merged claude session");
    assert!(raw.contains(&format!("\"sessionId\":\"{created_session_id}\"")));
    assert!(raw.contains("\"model\":\"openai\""));
}

#[test]
fn merge_agent_to_agent_session_filter_limits_the_copy_scope() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "merge",
        "--from-agent",
        "codex",
        "--to-agent",
        "claude",
        "--project",
        "/Users/test/codex-proj",
        "--session",
        "sess-codex-2",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions_merged"].as_i64(), Some(1));
    assert_eq!(json["total_messages_merged"].as_i64(), Some(4));
    let merge = &json["session_merges"].as_array().unwrap()[0];
    assert_eq!(merge["from_session_id"].as_str(), Some("sess-codex-2"));

    let sessions_output = fixture.run_cli(&[
        "--source",
        "claude",
        "sessions",
        "--project=-Users-test-codex-proj",
        "--limit",
        "10",
    ]);
    assert!(
        sessions_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&sessions_output.stderr)
    );
    let sessions_json = parse_stdout_json(&sessions_output);
    assert_eq!(sessions_json["total_sessions"].as_i64(), Some(1));
}

// --- prompt ---

#[test]
fn prompt_with_session_history_sends_transcripts_to_backend() {
    let fixture = TestFixture::seeded();
    let codex_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-prompt.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-04T00:00:00","payload":{"id":"sess-codex-prompt","cwd":"/Users/test/proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-04T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-04T00:00:01","payload":{"type":"user_message","message":"prompt codex question"}}
{"type":"response_item","timestamp":"2025-01-04T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"prompt codex answer"}]}}"#,
    );

    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-prompt-1","outputs":[{"text":"optimized prompt text here"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "prompt",
            "implement user authentication",
            "--target",
            "claude",
            "--agent",
            "gemini",
            "--project",
            "/Users/test/proj",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    // Output is raw text, not JSON
    let stdout = stdout_text(&output);
    assert_eq!(stdout.trim(), "optimized prompt text here");
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "prompt output should be raw text, not JSON"
    );

    // Verify the request body
    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(
        input.contains("implement user authentication"),
        "input should contain the query"
    );
    assert!(input.contains("<query>"), "input should use XML structure");
    assert!(
        input.contains("hello from claude") || input.contains("prompt codex question"),
        "input should include session transcript data"
    );

    let system = body["system_instruction"].as_str().unwrap();
    assert!(
        system.contains("Prompt Optimizer"),
        "system instruction should identify as Prompt Optimizer"
    );
    assert!(
        system.contains("Claude Code"),
        "system instruction should target Claude Code for --target claude"
    );
}

#[test]
fn prompt_for_codex_target_uses_codex_specific_instructions() {
    let fixture = TestFixture::seeded();

    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-prompt-codex","outputs":[{"text":"codex optimized prompt"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "prompt",
            "fix authentication bug",
            "--target",
            "codex",
            "--agent",
            "gemini",
            "--project",
            "/Users/test/proj",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let system = body["system_instruction"].as_str().unwrap();
    assert!(
        system.contains("Codex CLI"),
        "system instruction should target Codex CLI for --target codex"
    );
    assert!(
        system.contains("AGENTS.md"),
        "codex target should reference AGENTS.md"
    );
}

#[test]
fn prompt_without_sessions_falls_back_to_query_only() {
    let fixture = TestFixture::seeded();

    let (base_url, captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-prompt-nosessions","outputs":[{"text":"query-only prompt"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "prompt",
            "build a REST API",
            "--target",
            "claude",
            "--agent",
            "gemini",
            "--project",
            "/nonexistent/project/path",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout = stdout_text(&output);
    assert_eq!(stdout.trim(), "query-only prompt");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = first_input_text(&body);
    assert!(
        input.contains("build a REST API"),
        "input should contain the query even without session history"
    );
    assert!(input.contains("<query>"), "input should use XML structure");
    // No session_history tag since no sessions exist for this project
    assert!(
        !input.contains("<session_history>"),
        "input should not include session_history when no sessions exist"
    );
}

#[test]
fn prompt_outputs_raw_text_not_json() {
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-prompt-raw","outputs":[{"text":"<context>\nHere is a prompt\n</context>"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "prompt",
            "add tests",
            "--target",
            "codex",
            "--agent",
            "gemini",
            "--project",
            "/Users/test/proj",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout = stdout_text(&output);
    assert!(stdout.contains("<context>"));
    assert!(stdout.contains("Here is a prompt"));
}

#[test]
fn prompt_hard_failure_shows_meaningful_error() {
    let fixture = TestFixture::seeded();
    // No GOOGLE_API_KEY set → Gemini init should fail with meaningful error
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "prompt",
            "add auth",
            "--target",
            "claude",
            "--agent",
            "gemini",
            "--project",
            "/Users/test/proj",
        ],
        &[],
    );

    assert!(
        !output.status.success(),
        "prompt should fail without API key"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should contain an error message");
}

#[test]
fn prompt_clipboard_failure_does_not_throw() {
    // On CI without a display server, clipboard will fail, but the command should still succeed
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_gemini_server(
        r#"{"id":"interaction-prompt-clip","outputs":[{"text":"clipboard test prompt"}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "prompt",
            "fix bug",
            "--target",
            "claude",
            "--agent",
            "gemini",
            "--project",
            "/Users/test/proj",
        ],
        &[
            ("GOOGLE_API_KEY", "test-key"),
            ("GEMINI_API_BASE_URL", base_url.as_str()),
            // Force no display to ensure clipboard fails
            ("DISPLAY", ""),
            ("WAYLAND_DISPLAY", ""),
        ],
    );

    assert!(
        output.status.success(),
        "prompt should succeed even when clipboard is unavailable, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout = stdout_text(&output);
    assert_eq!(stdout.trim(), "clipboard test prompt");
}

#[test]
fn prompt_requires_target_flag() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["prompt", "some query"]);
    assert!(
        !output.status.success(),
        "prompt without --target should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--target"),
        "error should mention --target: {stderr}"
    );
}

#[test]
fn prompt_requires_query() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["prompt", "--target", "claude"]);
    assert!(!output.status.success(), "prompt without query should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("query") || stderr.contains("required"),
        "error should mention missing query: {stderr}"
    );
}

#[test]
fn prompt_rejects_invalid_target() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["prompt", "some query", "--target", "gpt"]);
    assert!(
        !output.status.success(),
        "prompt with invalid target should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value"),
        "error should mention invalid value: {stderr}"
    );
}

#[test]
fn merge_rejects_ambiguous_session_ids_without_agent_hints() {
    let fixture = TestFixture::seeded();
    write_file(
        &fixture
            .home
            .join(".codex")
            .join("sessions")
            .join("sess-shared.jsonl"),
        r#"{"type":"session_meta","timestamp":"2025-01-05T00:00:00","payload":{"id":"sess-shared","cwd":"/Users/test/shared-codex","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-05T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-05T00:00:01","payload":{"type":"user_message","message":"codex dup"}} 
{"type":"response_item","timestamp":"2025-01-05T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"codex dup answer"}]}}"#,
    );
    write_file(
        &fixture
            .home
            .join(".claude")
            .join("projects")
            .join("-Users-test-shared-claude")
            .join("sess-shared.jsonl"),
        r#"{"type":"user","sessionId":"sess-shared","message":{"role":"user","content":"claude dup"},"timestamp":"2025-01-05T00:00:00","uuid":"dup-u1","cwd":"/Users/test/shared-claude"}
{"type":"assistant","sessionId":"sess-shared","message":{"role":"assistant","content":"claude dup answer","model":"claude-3-opus","usage":{"input_tokens":11,"output_tokens":7}},"timestamp":"2025-01-05T00:00:01","uuid":"dup-a1","parentUuid":"dup-u1","cwd":"/Users/test/shared-claude"}"#,
    );

    let output = fixture.run_cli(&[
        "merge",
        "--from-session",
        "sess-shared",
        "--to-session",
        "sess-codex-1",
    ]);

    assert!(
        !output.status.success(),
        "merge should fail when a session id maps to multiple sources"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ambiguous"), "stderr={stderr}");
    assert!(stderr.contains("--from-agent"), "stderr={stderr}");
}
