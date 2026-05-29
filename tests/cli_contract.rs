mod common;

use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
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

fn write_executable(path: &Path, contents: &str) {
    write_file(path, contents);
    let mut permissions = fs::metadata(path).expect("script metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod script");
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout UTF-8")
}

fn loopback_bind_available() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
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
    assert_eq!(json["total_messages"].as_i64().unwrap(), 17);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 7);
    // 3 codex projects + 1 claude project + 1 cursor project + 1 grok project + 1 pi project = we should see all sources
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
    assert!(
        sources.contains(&"cursor"),
        "should include cursor projects"
    );
    assert!(sources.contains(&"grok"), "should include grok projects");
    assert!(sources.contains(&"pi"), "should include pi projects");
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

#[test]
fn projects_with_source_cursor_filters() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "cursor", "projects"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let projects = json["projects"].as_array().unwrap();
    assert!(!projects.is_empty(), "cursor projects should exist");
    for project in projects {
        assert_eq!(project["source"].as_str().unwrap(), "cursor");
    }
}

#[test]
fn messages_filtered_by_source_cursor() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "cursor", "messages", "--all"], &[]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    let messages = json["messages"].as_array().unwrap();
    for msg in messages {
        assert_eq!(msg["source"].as_str().unwrap(), "cursor");
    }
}

#[test]
fn sessions_with_source_cursor() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "cursor", "sessions", "--all"], &[]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 1);
    let sessions = json["sessions"].as_array().unwrap();
    for session in sessions {
        assert_eq!(session["source"].as_str().unwrap(), "cursor");
    }
}

#[test]
fn projects_with_source_grok_filters() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "grok", "projects"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 3);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 1);
    let projects = json["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["source"].as_str().unwrap(), "grok");
    assert_eq!(
        projects[0]["name"].as_str().unwrap(),
        "/Users/test/grok-proj"
    );
    assert_eq!(
        projects[0]["original_path"].as_str().unwrap(),
        "/Users/test/grok-proj"
    );
}

#[test]
fn messages_filtered_by_source_grok() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "grok", "messages", "--all"], &[]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 3);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert!(
        messages
            .iter()
            .all(|msg| msg["source"].as_str().unwrap() == "grok")
    );
    assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
    assert_eq!(messages[0]["content"].as_str().unwrap(), "hello from grok");
    assert_eq!(messages[1]["role"].as_str().unwrap(), "assistant");
    assert_eq!(
        messages[1]["content"].as_str().unwrap(),
        "hi from grok assistant"
    );
    assert_eq!(messages[2]["role"].as_str().unwrap(), "user");
    assert_eq!(
        messages[2]["content"].as_str().unwrap(),
        "follow-up from grok"
    );
}

#[test]
fn sessions_with_source_grok() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "grok", "sessions", "--all"], &[]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 1);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["source"].as_str().unwrap(), "grok");
    assert_eq!(sessions[0]["session_id"].as_str().unwrap(), "sess-grok-1");
    assert_eq!(
        sessions[0]["project_path"].as_str().unwrap(),
        "/Users/test/grok-proj"
    );
    assert_eq!(sessions[0]["preview"].as_str().unwrap(), "hello from grok");
}

#[test]
fn projects_with_source_pi_filters() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "pi", "projects"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 1);
    let projects = json["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["source"].as_str().unwrap(), "pi");
    assert_eq!(
        projects[0]["name"].as_str().unwrap(),
        "--Users-test-pi-proj--"
    );
    assert_eq!(
        projects[0]["original_path"].as_str().unwrap(),
        "/Users/test/pi-proj"
    );
}

#[test]
fn messages_filtered_by_source_pi() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "pi", "messages", "--all"], &[]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert!(
        messages
            .iter()
            .all(|msg| msg["source"].as_str().unwrap() == "pi")
    );
    assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
    assert_eq!(messages[0]["content"].as_str().unwrap(), "hello from pi");
    assert_eq!(messages[1]["role"].as_str().unwrap(), "assistant");
    assert_eq!(
        messages[1]["content"].as_str().unwrap(),
        "hi from pi assistant"
    );
    assert_eq!(messages[1]["input_tokens"].as_i64().unwrap(), 12);
    assert_eq!(messages[1]["output_tokens"].as_i64().unwrap(), 6);
}

#[test]
fn sessions_with_source_pi() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "pi", "sessions", "--all"], &[]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 1);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["source"].as_str().unwrap(), "pi");
    assert_eq!(sessions[0]["session_id"].as_str().unwrap(), "sess-pi-1");
    assert_eq!(
        sessions[0]["project_path"].as_str().unwrap(),
        "/Users/test/pi-proj"
    );
    assert_eq!(sessions[0]["preview"].as_str().unwrap(), "hello from pi");
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
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 9);
    assert_eq!(sessions.len(), 9);

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
        9
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
        21
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
    assert_eq!(json["total_messages"].as_i64().unwrap(), 21);
    assert_eq!(messages.len(), 21);
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
fn messages_latest_selects_newest_message_from_latest_session() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "messages", "--all", "--latest", "1"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["session_id"].as_str().unwrap(),
        "sess-codex-recent-1"
    );
    assert_eq!(
        messages[0]["content"].as_str().unwrap(),
        "recent project answer"
    );
    assert!(!json["next_page"].as_bool().unwrap());
    assert!(json["next_command"].is_null());
}

#[test]
fn messages_latest_without_value_defaults_to_one() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "messages", "--all", "--latest"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["session_id"].as_str().unwrap(),
        "sess-codex-recent-1"
    );
    assert_eq!(
        messages[0]["content"].as_str().unwrap(),
        "recent project answer"
    );
}

#[test]
fn messages_latest_window_returns_chronological_tail_of_latest_session() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--latest",
        "5",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["session_id"].as_str().unwrap(), "sess-codex-1");
    assert_eq!(messages[0]["content"].as_str().unwrap(), "hello from codex");
    assert_eq!(
        messages[1]["content"].as_str().unwrap(),
        "short codex answer"
    );
}

#[test]
fn messages_message_index_range_slices_chronological_filtered_messages() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--from-message-index",
        "1",
        "--to-message-index",
        "4",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    assert_eq!(messages.len(), 3);
    let contents = messages
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            "start longer codex thread",
            "first long codex answer",
            "follow-up question"
        ]
    );
    assert!(!json["next_page"].as_bool().unwrap());
    assert!(json["next_command"].is_null());
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
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 9);
    assert_eq!(sessions.len(), 9);
    let sources = sessions
        .iter()
        .map(|session| session["source"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(sources.contains(&"claude"));
    assert!(sources.contains(&"codex"));
    assert!(sources.contains(&"cursor"));
    assert!(sources.contains(&"grok"));
    assert!(sources.contains(&"pi"));
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
fn export_with_source_grok_and_project_filters_by_source() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "grok",
        "export",
        "--project",
        "/Users/test/grok-proj",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 3);
    let messages = json["messages"].as_array().unwrap();
    let contents = messages
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            "hello from grok",
            "hi from grok assistant",
            "follow-up from grok"
        ]
    );
    assert!(
        messages
            .iter()
            .all(|message| message["source"].as_str().unwrap() == "grok")
    );
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

#[test]
fn export_without_project_can_read_grok_cwd() {
    let fixture = TestFixture::seeded();
    let proj_dir = fixture.home.join("grok-cwd-proj");
    fs::create_dir_all(&proj_dir).expect("create grok cwd project dir");
    let cwd_str = fs::canonicalize(&proj_dir)
        .expect("canonicalize grok cwd project")
        .to_string_lossy()
        .into_owned();
    let encoded_cwd = cwd_str.replace('/', "%2F");

    let session_dir = fixture
        .home
        .join(".grok")
        .join("sessions")
        .join(encoded_cwd)
        .join("sess-cwd-grok");
    write_file(
        &session_dir.join("summary.json"),
        &format!(
            r#"{{"info":{{"id":"sess-cwd-grok","cwd":"{}"}},"created_at":"2025-01-09T00:00:00Z","updated_at":"2025-01-09T00:00:02Z","current_model_id":"grok-build"}}"#,
            cwd_str
        ),
    );
    write_file(
        &session_dir.join("updates.jsonl"),
        r#"{"timestamp":1736380801,"method":"session/update","params":{"sessionId":"sess-cwd-grok","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"cwd grok question"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736380801000}}}
{"timestamp":1736380802,"method":"session/update","params":{"sessionId":"sess-cwd-grok","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"cwd grok answer"}},"_meta":{"agentTimestampMs":1736380802000}}}"#,
    );

    let output = fixture.run_cli_in_dir(&["--source", "grok", "export"], &proj_dir);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 2);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages[0]["source"].as_str().unwrap(), "grok");
    assert_eq!(messages[0]["project_name"].as_str().unwrap(), cwd_str);
    assert_eq!(
        messages[0]["content"].as_str().unwrap(),
        "cwd grok question"
    );
    assert_eq!(messages[1]["content"].as_str().unwrap(), "cwd grok answer");
}

// --- remember ---

#[test]
fn remember_all_includes_claude_and_codex_messages() {
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
    if !loopback_bind_available() {
        return;
    }
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
fn prompt_command_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "prompt",
        "implement user authentication",
        "--target",
        "claude",
    ]);

    assert!(!output.status.success(), "prompt should be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") || stderr.contains("unknown subcommand"),
        "stderr should report prompt as an unknown subcommand: {stderr}"
    );
}

#[test]
fn merge_command_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["merge", "--from-session", "sess-claude-1"]);

    assert!(!output.status.success(), "merge should be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") || stderr.contains("unknown subcommand"),
        "stderr should report merge as an unknown subcommand: {stderr}"
    );
}

#[test]
fn sync_command_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["sync", "status"]);

    assert!(!output.status.success(), "sync should be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("unrecognized subcommand"),
        "stderr should report unsupported sync usage: {stderr}"
    );
}

// --- pagination metadata ---

#[test]
fn messages_pagination_includes_next_page_and_next_command() {
    let fixture = TestFixture::seeded();
    // codex-proj has 6 messages (sess-codex-1: 2 + sess-codex-2: 4)
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--limit",
        "2",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    assert_eq!(json["messages"].as_array().unwrap().len(), 2);
    assert!(json["next_page"].as_bool().unwrap());
    assert_eq!(json["next_offset"].as_i64().unwrap(), 2);
    let next_cmd = json["next_command"].as_str().unwrap();
    assert!(next_cmd.contains("messages"), "next_command={next_cmd}");
    assert!(next_cmd.contains("--limit 2"), "next_command={next_cmd}");
    assert!(next_cmd.contains("--offset 2"), "next_command={next_cmd}");
    assert!(
        next_cmd.contains("--source codex"),
        "next_command={next_cmd}"
    );
    assert!(
        next_cmd.contains("--project /Users/test/codex-proj"),
        "next_command={next_cmd}"
    );
}

#[test]
fn messages_pagination_no_next_command_when_all_results_fit() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "messages", "--all", "--limit", "100"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert!(!json["next_page"].as_bool().unwrap());
    assert!(json["next_command"].is_null());
}

#[test]
fn messages_pagination_next_command_preserves_sort_and_order() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--all",
        "--limit",
        "2",
        "-s",
        "message-count",
        "-o",
        "desc",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert!(json["next_page"].as_bool().unwrap());
    let next_cmd = json["next_command"].as_str().unwrap();
    assert!(
        next_cmd.contains("--sort-by message-count"),
        "next_command={next_cmd}"
    );
    assert!(next_cmd.contains("--order desc"), "next_command={next_cmd}");
    assert!(next_cmd.contains("--all"), "next_command={next_cmd}");
}

// ---------------------------------------------------------------------------
// messages --session without --project bypasses cwd auto-discovery
// ---------------------------------------------------------------------------

#[test]
fn messages_session_without_project_searches_all_projects() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    // With auto-discovery enabled and a valid cwd project, passing --session
    // but NOT --project should still return the session even though it belongs
    // to a different project (the seeded claude session lives under
    // /Users/test/proj, not the cwd project).
    let output = fixture.run_cli_in_dir_with_env(
        &["messages", "--session", "sess-claude-1"],
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
    assert_eq!(
        messages.len(),
        2,
        "should find sess-claude-1 even though cwd points to a different project"
    );
    assert_eq!(messages[0]["session_id"].as_str().unwrap(), "sess-claude-1");
}

#[test]
fn messages_session_without_project_or_source_prints_hint() {
    let fixture = TestFixture::seeded();

    let output = fixture.run_cli(&["messages", "--session", "sess-claude-1"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--source"),
        "expected hint about --source in stderr, got: {stderr}"
    );
}

#[test]
fn messages_session_with_source_does_not_print_hint() {
    let fixture = TestFixture::seeded();

    let output = fixture.run_cli(&[
        "--source",
        "claude",
        "messages",
        "--session",
        "sess-claude-1",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("--source"),
        "hint should NOT appear when --source is provided, got: {stderr}"
    );
}

#[test]
fn messages_session_with_explicit_project_uses_project_scope() {
    let fixture = TestFixture::seeded();

    // When --project is explicitly provided alongside --session, the project
    // filter should apply (no bypass).
    let output = fixture.run_cli(&[
        "messages",
        "--session",
        "sess-claude-1",
        "--project=-Users-test-proj",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);

    // Asking for a project that doesn't contain the session → 0 messages
    let empty_output = fixture.run_cli(&[
        "messages",
        "--session",
        "sess-claude-1",
        "--project",
        "/Users/test/codex-proj",
    ]);
    assert!(empty_output.status.success());
    let empty_json = parse_stdout_json(&empty_output);
    assert_eq!(empty_json["total_messages"].as_i64().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// reverse session selection: --session-back / --session-range / mmr prev
// ---------------------------------------------------------------------------

fn assert_messages_failure(output: &Output, expected_error_kind: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["command"], "messages");
    assert_eq!(
        json["error_kind"],
        expected_error_kind,
        "stdout={}",
        stdout_text(output)
    );
    assert!(json["message"].is_string());
}

#[test]
fn prev_returns_previous_session_in_cwd_project() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let output =
        fixture.run_cli_in_dir_with_env(&["prev"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let selection = &json["session_selection"];
    assert_eq!(selection["axis"], "session-back");
    assert_eq!(selection["total_sessions_in_scope"].as_i64().unwrap(), 2);
    let selected = selection["selected"].as_array().unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["age"].as_u64().unwrap(), 1);
    // The newest session in the cwd project is the claude one (last_timestamp 00:02);
    // the previous (age 1) is the codex session (last_timestamp 00:00:02).
    assert_eq!(
        selected[0]["session_id"].as_str().unwrap(),
        "sess-cwd-codex"
    );
    assert_eq!(
        selection["skipped_newest"]["session_id"].as_str().unwrap(),
        "sess-cwd-claude"
    );
    assert!(
        json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["session_id"].as_str().unwrap() == "sess-cwd-codex")
    );
}

#[test]
fn messages_session_back_one_reports_age_one() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--session-back",
        "1",
        "--pretty",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let selection = &json["session_selection"];
    assert_eq!(selection["axis"], "session-back");
    assert_eq!(selection["total_sessions_in_scope"].as_i64().unwrap(), 2);
    let selected = selection["selected"].as_array().unwrap();
    assert_eq!(selected[0]["age"].as_u64().unwrap(), 1);
    assert_eq!(selected[0]["session_id"].as_str().unwrap(), "sess-codex-2");
    assert_eq!(
        selected[0]["equivalent_command"].as_str().unwrap(),
        "mmr messages --session sess-codex-2"
    );
    assert!(
        json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["session_id"].as_str().unwrap() == "sess-codex-2")
    );
}

#[test]
fn messages_session_range_merges_two_previous_sessions() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--all",
        "--session-range",
        "2..1",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let selection = &json["session_selection"];
    assert_eq!(selection["axis"], "session-range");
    assert_eq!(selection["total_sessions_in_scope"].as_i64().unwrap(), 3);
    let ages: Vec<(u64, &str)> = selection["selected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["age"].as_u64().unwrap(),
                s["session_id"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(ages, vec![(1, "sess-codex-1"), (2, "sess-codex-2")]);

    let contents: Vec<&str> = json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    // Merged chronologically across both sessions (age 1 oldest message is 00:00:01).
    assert_eq!(contents.first().copied(), Some("hello from codex"));
    assert_eq!(contents.last().copied(), Some("short codex answer"));
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
}

#[test]
fn messages_session_range_all_sources_carries_source_and_project() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["messages", "--all", "--session-range", "2..1"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let selection = &json["session_selection"];
    assert!(selection["scope"]["all"].as_bool().unwrap());
    assert!(selection["scope"]["source"].is_null());
    let selected = selection["selected"].as_array().unwrap();
    let ages: Vec<u64> = selected
        .iter()
        .map(|s| s["age"].as_u64().unwrap())
        .collect();
    assert_eq!(ages, vec![1, 2]);
    for entry in selected {
        assert!(
            !entry["source"].as_str().unwrap().is_empty(),
            "each selected entry self-describes its source"
        );
        assert!(
            !entry["project_name"].as_str().unwrap().is_empty(),
            "each selected entry self-describes its project_name"
        );
    }
}

#[test]
fn messages_session_back_zero_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--session-back",
        "0",
    ]);
    assert_messages_failure(&output, "age_zero_not_selectable");
}

#[test]
fn messages_session_back_zero_with_include_newest_returns_newest() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--session-back",
        "0",
        "--include-newest",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    let selection = &json["session_selection"];
    let selected = selection["selected"].as_array().unwrap();
    assert_eq!(selected[0]["age"].as_u64().unwrap(), 0);
    assert_eq!(selected[0]["session_id"].as_str().unwrap(), "sess-codex-1");
    assert!(
        selection["skipped_newest"].is_null(),
        "include-newest means nothing is skipped"
    );
}

#[test]
fn messages_session_back_out_of_range_names_counts() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--all",
        "--session-back",
        "5",
    ]);
    assert_messages_failure(&output, "session_back_out_of_range");
    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions_in_scope"].as_i64().unwrap(), 3);
    assert_eq!(json["max_selectable_age"].as_u64().unwrap(), 2);
    assert_eq!(json["requested_age"].as_u64().unwrap(), 5);
}

#[test]
fn messages_rejects_multiple_session_selectors() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--all",
        "--session-back",
        "1",
        "--latest",
        "5",
    ]);
    assert_messages_failure(&output, "multiple_session_selectors");
}

#[test]
fn messages_without_session_axis_omits_session_selection_field() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "messages", "--all", "--limit", "5"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert!(
        json.get("session_selection").is_none(),
        "session_selection must be absent for plain messages queries"
    );
}

#[test]
fn messages_strawman_from_index_flags_are_rejected_by_clap() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["messages", "--from-index", "-1", "--to-index", "-1"]);
    assert!(
        !output.status.success(),
        "strawman --from-index/--to-index must not be accepted"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--from-index"),
        "expected clap unexpected-argument error, got: {stderr}"
    );
}

#[test]
fn session_axis_pagination_pins_to_concrete_session_not_recency_age() {
    let fixture = TestFixture::seeded();

    // Page 1: previous session (age 1) in codex-proj is sess-codex-2 (4 messages).
    let page1 = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--session-back",
        "1",
        "--limit",
        "2",
    ]);
    assert!(
        page1.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page1.stderr)
    );
    let page1_json = parse_stdout_json(&page1);
    assert!(page1_json["next_page"].as_bool().unwrap());
    let next_cmd = page1_json["next_command"].as_str().unwrap().to_string();
    // The next command must pin to the concrete session id, never echo the unstable age axis.
    assert!(
        next_cmd.contains("--session sess-codex-2"),
        "next_command should pin to the resolved session id, got: {next_cmd}"
    );
    assert!(
        !next_cmd.contains("--session-back") && !next_cmd.contains("--session-range"),
        "next_command must not echo the recency-age selector, got: {next_cmd}"
    );
    assert!(
        next_cmd.contains("--offset 2"),
        "next_command should advance the offset, got: {next_cmd}"
    );

    // A new, newer session lands between page reads, shifting every recency age by one.
    let injected = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-injected.jsonl");
    write_file(
        &injected,
        r#"{"type":"session_meta","timestamp":"2025-01-09T00:00:00","payload":{"id":"sess-codex-injected","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-09T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-09T00:00:01","payload":{"type":"user_message","message":"freshly injected session"}}
{"type":"response_item","timestamp":"2025-01-09T00:01:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"injected answer"}]}}"#,
    );

    // Page 2 follows the pinned next_command verbatim and must stay on sess-codex-2.
    let page2_args: Vec<&str> = next_cmd.split_whitespace().skip(1).collect();
    let page2 = fixture.run_cli(&page2_args);
    assert!(
        page2.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page2.stderr)
    );
    let page2_json = parse_stdout_json(&page2);
    let page2_messages = page2_json["messages"].as_array().unwrap();
    assert_eq!(page2_messages.len(), 2);
    assert!(
        page2_messages
            .iter()
            .all(|m| m["session_id"].as_str().unwrap() == "sess-codex-2"),
        "paged window must stay pinned to the original session after a new one is injected"
    );

    // Contrast: re-running the age-based selector after injection shifts to a different session,
    // proving the pin is load-bearing (age 1 is now sess-codex-1, not sess-codex-2).
    let shifted = fixture.run_cli(&[
        "--source",
        "codex",
        "messages",
        "--project",
        "/Users/test/codex-proj",
        "--session-back",
        "1",
        "--limit",
        "2",
        "--offset",
        "2",
    ]);
    let shifted_json = parse_stdout_json(&shifted);
    let shifted_session = shifted_json["session_selection"]["selected"][0]["session_id"]
        .as_str()
        .unwrap();
    assert_eq!(
        shifted_session, "sess-codex-1",
        "the recency age axis is intentionally unstable across an injected session"
    );
}

#[test]
fn teleport_bundle_pack_inspect_apply_round_trip() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-session.mmr");

    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        pack_output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&pack_output.stderr)
    );
    let pack_json = parse_stdout_json(&pack_output);
    assert_eq!(pack_json["command"], "teleport/pack");
    assert_eq!(pack_json["status"], "ok");
    assert_eq!(pack_json["session"]["source_session_id"], "sess-codex-1");
    assert!(bundle_path.is_file(), "bundle file should exist");

    let inspect_output = fixture.run_cli(&[
        "teleport",
        "inspect",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        inspect_output.status.success(),
        "inspect stderr={}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspect_json = parse_stdout_json(&inspect_output);
    assert_eq!(inspect_json["command"], "teleport/inspect");
    assert_eq!(inspect_json["status"], "ok");
    assert_eq!(inspect_json["apply_ready"], true);
    assert!(
        inspect_json["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .all(|artifact| artifact["verified"] == true)
    );

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove seeded native session before apply");

    let apply_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        apply_output.status.success(),
        "apply stderr={}",
        String::from_utf8_lossy(&apply_output.stderr)
    );
    let apply_json = parse_stdout_json(&apply_output);
    assert_eq!(apply_json["command"], "teleport/apply");
    assert_eq!(apply_json["status"], "ok");
    assert_eq!(apply_json["native"]["written"], true);
    assert_eq!(
        apply_json["resume"]["status"].as_str().unwrap(),
        "visible_but_not_resumable"
    );
    assert!(
        apply_json["resume"]["documented_command"]
            .as_str()
            .unwrap()
            .contains("codex exec resume sess-codex-1")
    );

    assert!(native_path.is_file(), "native codex session should exist");

    let reapply_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(reapply_output.status.success());
    let reapply_json = parse_stdout_json(&reapply_output);
    assert_eq!(reapply_json["status"], "skipped");
}

fn assert_teleport_failure(output: &Output, expected_exit: i32, expected_command: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["command"], expected_command);
    assert!(json["message"].is_string());
}

#[test]
fn teleport_inspect_rejects_positional_and_to_together() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-session.mmr");
    fs::write(&bundle_path, "{}").expect("write dummy bundle");

    let output = fixture.run_cli(&[
        "teleport",
        "inspect",
        bundle_path.to_str().expect("bundle path"),
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert_teleport_failure(&output, 2, "teleport/inspect");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("only one bundle locator"));
}

#[test]
fn teleport_inspect_missing_locator_fails_with_json() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["teleport", "inspect"]);
    assert_teleport_failure(&output, 2, "teleport/inspect");
}

#[test]
fn teleport_apply_missing_locator_fails_with_json() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["teleport", "apply"]);
    assert_teleport_failure(&output, 2, "teleport/apply");
}

#[test]
fn teleport_inspect_rejects_as_flag_with_json() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-session.mmr");
    fs::write(&bundle_path, "{}").expect("write dummy bundle");

    let output = fixture.run_cli(&[
        "teleport",
        "inspect",
        bundle_path.to_str().expect("bundle path"),
        "--as",
        "native",
    ]);
    assert_teleport_failure(&output, 2, "teleport/inspect");
}

#[test]
fn teleport_apply_rejects_as_flag_with_json() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-session.mmr");
    fs::write(&bundle_path, "{}").expect("write dummy bundle");

    let output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--as",
        "native",
    ]);
    assert_teleport_failure(&output, 2, "teleport/apply");
}

#[test]
fn teleport_pack_rejects_shared_safe_with_json() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--as",
        "shared-safe",
        "--dry-run",
    ]);
    assert_teleport_failure(&output, 3, "teleport/pack");
}

#[test]
fn teleport_pack_help_does_not_advertise_shared_safe() {
    let fixture = TestFixture::seeded();
    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["teleport", "pack", "--help"])
        .env("HOME", &fixture.home)
        .output()
        .expect("run mmr teleport pack --help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("codex, claude, cursor, grok, and pi"),
        "help should describe multi-provider native scope: {help}"
    );
    assert!(
        !help.contains("shared-safe"),
        "help must not list shared-safe as a supported pack fidelity: {help}"
    );
}

#[test]
fn teleport_pack_claude_session_via_provider_profile() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "claude",
        "--session",
        "sess-claude-1",
        "--project=-Users-test-proj",
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/pack");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["session"]["source"], "claude");
    assert!(
        json["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .any(|artifact| artifact["path"] == "native/claude/transcript.jsonl")
    );
}

#[test]
fn teleport_serve_grok_session_packs_and_starts() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "teleport",
            "serve",
            "--source",
            "grok",
            "--session",
            "sess-grok-1",
            "--project",
            "/Users/test/grok-proj",
            "--bind",
            "127.0.0.1:0",
            "--timeout",
            "30",
        ])
        .env("HOME", &fixture.home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn teleport serve for grok");

    let stdout = child.stdout.take().expect("serve stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read serve startup json");
    let startup: serde_json::Value =
        serde_json::from_str(line.trim()).expect("parse serve startup json");
    assert_eq!(startup["command"], "teleport/serve");
    assert_eq!(startup["status"], "ok");
    assert_eq!(startup["session"]["source"], "grok");
    assert_eq!(startup["session"]["source_session_id"], "sess-grok-1");
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn teleport_bundle_id_is_stable_across_repacks() {
    let fixture = TestFixture::seeded();
    let args = [
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--dry-run",
    ];

    let first = fixture.run_cli(&args);
    assert!(
        first.status.success(),
        "first pack stderr={}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_json = parse_stdout_json(&first);

    let second = fixture.run_cli(&args);
    assert!(
        second.status.success(),
        "second pack stderr={}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_json = parse_stdout_json(&second);

    assert_eq!(first_json["bundle_id"], second_json["bundle_id"]);
}

#[test]
fn teleport_pack_latest_selects_newest_session_in_project_scope() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--latest",
        "--source",
        "codex",
        "--project",
        "/Users/test/codex-proj",
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["session"]["source_session_id"], "sess-codex-1");
}

#[test]
fn teleport_pack_resolves_project_basename_alias() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--latest",
        "--source",
        "codex",
        "--project",
        "codex-proj",
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["session"]["source_session_id"], "sess-codex-1");
}

#[test]
fn teleport_pack_rejects_session_and_latest_together() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--session",
        "sess-codex-1",
        "--latest",
        "--source",
        "codex",
        "--dry-run",
    ]);
    assert_teleport_failure(&output, 2, "teleport/pack");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not both"));
}

#[test]
fn teleport_pack_missing_session_fails_with_json() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--session",
        "sess-does-not-exist",
        "--source",
        "codex",
        "--project",
        "/Users/test/codex-proj",
        "--dry-run",
    ]);
    assert_teleport_failure(&output, 2, "teleport/pack");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found in scope"));
}

#[test]
fn teleport_pack_ambiguous_session_id_fails_with_json() {
    let fixture = TestFixture::seeded();
    let duplicate_session = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-duplicate.jsonl");
    write_file(
        &duplicate_session,
        r#"{"type":"session_meta","timestamp":"2025-01-04T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/other-codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-04T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-04T00:00:01","payload":{"type":"user_message","message":"duplicate id different project"}}
{"type":"response_item","timestamp":"2025-01-04T00:01:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"duplicate response"}]}}"#,
    );

    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--session",
        "sess-codex-1",
        "--source",
        "codex",
        "--dry-run",
    ]);
    assert_teleport_failure(&output, 2, "teleport/pack");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("multiple sessions matched"));
}

#[test]
fn teleport_pack_default_without_session_selects_latest() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--project",
        "/Users/test/codex-proj",
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["session"]["source_session_id"], "sess-codex-1");
}

#[test]
fn teleport_apply_remaps_project_path_in_native_transcript() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-remap.mmr");
    let target_project = "/Users/test/remapped-proj";

    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(pack_output.status.success());

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove seeded native session before apply");

    let apply_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--project",
        target_project,
    ]);
    assert!(
        apply_output.status.success(),
        "apply stderr={}",
        String::from_utf8_lossy(&apply_output.stderr)
    );
    let apply_json = parse_stdout_json(&apply_output);
    assert_eq!(apply_json["target_project"], target_project);
    assert_eq!(apply_json["path_remap_applied"], true);

    let native_content = fs::read_to_string(&native_path).expect("read applied native transcript");
    assert!(
        native_content.contains(&format!(r#""cwd":"{target_project}""#)),
        "native transcript should rewrite session_meta cwd, got: {native_content}"
    );
}

#[test]
fn teleport_apply_rechecks_native_target_after_cached_apply() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-remap-cache.mmr");
    let first_target = "/Users/test/remapped-one";
    let second_target = "/Users/test/remapped-two";

    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(pack_output.status.success());

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove seeded native session before apply");

    let first_apply = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--project",
        first_target,
    ]);
    assert!(
        first_apply.status.success(),
        "first apply stderr={}",
        String::from_utf8_lossy(&first_apply.stderr)
    );

    let second_apply = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--project",
        second_target,
    ]);
    assert!(
        second_apply.status.success(),
        "second apply stderr={}",
        String::from_utf8_lossy(&second_apply.stderr)
    );
    let second_json = parse_stdout_json(&second_apply);
    assert_eq!(second_json["status"], "ok");
    assert_eq!(second_json["target_project"], second_target);

    let native_content = fs::read_to_string(&native_path).expect("read applied native transcript");
    assert!(
        native_content.contains(&format!(r#""cwd":"{second_target}""#)),
        "second apply should rewrite the native transcript, got: {native_content}"
    );
}

#[test]
fn teleport_apply_rejects_newer_existing_transcript_without_force() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-newer-guard.mmr");

    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(pack_output.status.success());

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::write(
        &native_path,
        r#"{"type":"session_meta","timestamp":"2025-01-09T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/codex-proj","timestamp":"2025-01-09T00:00:00"}}
{"type":"event_msg","timestamp":"2025-01-09T00:10:00","payload":{"type":"user_message","message":"newer local transcript"}}"#,
    )
    .expect("write newer native transcript");

    let apply_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert_teleport_failure(&apply_output, 3, "teleport/apply");
    let message = parse_stdout_json(&apply_output)["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("newer than bundle"));
    assert!(message.contains("--force"));

    let force_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--force",
    ]);
    assert!(
        force_output.status.success(),
        "force apply stderr={}",
        String::from_utf8_lossy(&force_output.stderr)
    );
    let force_json = parse_stdout_json(&force_output);
    assert_eq!(force_json["status"], "ok");
    assert_eq!(force_json["native"]["written"], true);
}

#[test]
fn teleport_apply_makes_session_visible_to_mmr_queries() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-visible.mmr");
    let target_project = "/Users/test/remapped-proj";
    let session_id = "sess-codex-1";

    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        session_id,
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        pack_output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&pack_output.stderr)
    );

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join(format!("{session_id}.jsonl"));
    fs::remove_file(&native_path).expect("remove seeded native session before apply");

    let apply_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--project",
        target_project,
    ]);
    assert!(
        apply_output.status.success(),
        "apply stderr={}",
        String::from_utf8_lossy(&apply_output.stderr)
    );
    let apply_json = parse_stdout_json(&apply_output);
    assert_eq!(apply_json["status"], "ok");
    assert_eq!(apply_json["target_project"], target_project);
    assert_eq!(apply_json["store"]["imported_events"], 0);

    let messages_output =
        fixture.run_cli(&["messages", "--session", session_id, "--source", "codex"]);
    assert!(
        messages_output.status.success(),
        "messages stderr={}",
        String::from_utf8_lossy(&messages_output.stderr)
    );
    let messages_json = parse_stdout_json(&messages_output);
    assert!(
        messages_json["total_messages"].as_i64().unwrap() >= 1,
        "applied session should be readable via mmr messages"
    );
    assert!(
        messages_json["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .all(|message| message["session_id"] == session_id),
        "messages should be scoped to the teleported session"
    );

    let sessions_output = fixture.run_cli(&[
        "sessions",
        "--project",
        "remapped-proj",
        "--source",
        "codex",
    ]);
    assert!(
        sessions_output.status.success(),
        "sessions stderr={}",
        String::from_utf8_lossy(&sessions_output.stderr)
    );
    let sessions_json = parse_stdout_json(&sessions_output);
    assert!(
        sessions_json["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["session_id"] == session_id),
        "applied session should appear under basename project alias"
    );

    let projects_output = fixture.run_cli(&["projects", "--source", "codex"]);
    assert!(
        projects_output.status.success(),
        "projects stderr={}",
        String::from_utf8_lossy(&projects_output.stderr)
    );
    let projects_json = parse_stdout_json(&projects_output);
    let remapped_project = projects_json["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .find(|project| project["name"] == target_project)
        .expect("applied project should appear in mmr projects");
    let aliases = remapped_project["aliases"]
        .as_array()
        .expect("project aliases");
    assert!(
        aliases.iter().any(|alias| alias == "remapped-proj"),
        "projects should expose basename alias for applied project"
    );

    let reapply_output = fixture.run_cli(&[
        "teleport",
        "apply",
        bundle_path.to_str().expect("bundle path"),
        "--project",
        target_project,
    ]);
    assert!(
        reapply_output.status.success(),
        "reapply stderr={}",
        String::from_utf8_lossy(&reapply_output.stderr)
    );
    let reapply_json = parse_stdout_json(&reapply_output);
    assert_eq!(reapply_json["status"], "skipped");
}

#[test]
fn teleport_send_dry_run_reports_ssh_plan_without_remote_contact() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "send",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        "bob@macbook",
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "send dry-run stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/send");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["transport"], "ssh");
    assert_eq!(json["to"], "bob@macbook");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["session"]["source_session_id"], "sess-codex-1");
    assert!(json["bundle_id"].is_string());
    assert!(json["bundle_path"].is_string());
    assert_eq!(json["remote_apply"]["attempted"], false);
    assert_eq!(json["remote_apply"]["status"], "not_attempted");
    let planned = json["planned_commands"]
        .as_object()
        .expect("planned_commands");
    assert!(
        planned["stream_apply"]
            .as_array()
            .expect("stream_apply argv")
            .iter()
            .any(|arg| arg.as_str() == Some("mmr teleport apply --to -")),
        "dry-run should include stream apply command"
    );
    assert!(
        planned["scp_bundle"]
            .as_array()
            .expect("scp argv")
            .last()
            .and_then(|arg| arg.as_str())
            .unwrap_or("")
            .contains("bob@macbook:"),
        "dry-run should include scp fallback target"
    );
}

#[test]
fn teleport_send_missing_to_fails_with_json() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "send",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
    ]);
    assert_teleport_failure(&output, 2, "teleport/send");
}

#[test]
fn teleport_send_stages_bundle_when_remote_mmr_is_missing() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    let ssh_log = fixture.home.join("ssh.log");
    let scp_log = fixture.home.join("scp.log");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MMR_FAKE_SSH_LOG"
case "$*" in
  *"command -v mmr"*) exit 1 ;;
  *"mkdir -p"*) exit 0 ;;
  *) exit 0 ;;
esac
"#,
    );
    write_executable(
        &fake_bin.join("scp"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MMR_FAKE_SCP_LOG"
exit 0
"#,
    );

    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let ssh_log_value = ssh_log.to_string_lossy().to_string();
    let scp_log_value = scp_log.to_string_lossy().to_string();
    let output = fixture.run_cli_with_env(
        &[
            "teleport",
            "send",
            "--source",
            "codex",
            "--session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--to",
            "bob@macbook",
        ],
        &[
            ("PATH", path.as_str()),
            ("MMR_FAKE_SSH_LOG", ssh_log_value.as_str()),
            ("MMR_FAKE_SCP_LOG", scp_log_value.as_str()),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing remote mmr should return partial exit code; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/send");
    assert_eq!(json["status"], "partial");
    assert_eq!(json["remote_apply"]["attempted"], false);
    assert_eq!(json["remote_apply"]["mode"], "inbox_copy");
    assert!(
        json["next_command"]
            .as_str()
            .expect("next_command")
            .contains("mmr teleport apply --to ~/.mmr/teleport/inbox/")
    );
    assert!(
        fs::read_to_string(&ssh_log)
            .expect("ssh log")
            .contains("mkdir -p ~/.mmr/teleport/inbox/")
    );
    assert!(
        fs::read_to_string(&scp_log)
            .expect("scp log")
            .contains("bob@macbook:~/.mmr/teleport/inbox/")
    );
}

#[test]
fn teleport_send_file_writes_atomic_inbox_layout() {
    let fixture = TestFixture::seeded();
    let inbox = fixture.home.join("teleport-inbox");
    let to = format!("file://{}", inbox.display());
    let output = fixture.run_cli(&[
        "teleport",
        "send",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        &to,
    ]);
    assert!(
        output.status.success(),
        "send file stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/send");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["transport"], "file");
    assert_eq!(json["to"], to);
    assert_eq!(json["dry_run"], false);
    assert_eq!(json["session"]["source_session_id"], "sess-codex-1");
    let bundle_id = json["bundle_id"].as_str().expect("bundle_id");
    let entry = inbox.join(bundle_id);
    assert_eq!(json["inbox_path"].as_str(), Some(entry.to_str().unwrap()));
    assert!(entry.join("bundle.mmr").is_file());
    assert!(entry.join("bundle.sha256").is_file());
    assert!(entry.join("ready").is_file());
    assert!(!entry.join("bundle.mmr.partial").exists());
    assert_eq!(
        json["bundle_path"].as_str(),
        Some(entry.join("bundle.mmr").to_str().unwrap())
    );
    assert_eq!(
        json["ready_path"].as_str(),
        Some(entry.join("ready").to_str().unwrap())
    );
    assert!(json["sha256"].is_string());
    assert!(json["bytes"].as_u64().is_some());
}

#[test]
fn teleport_send_file_dry_run_does_not_write_files() {
    let fixture = TestFixture::seeded();
    let inbox = fixture.home.join("teleport-inbox-dry-run");
    let to = format!("file://{}", inbox.display());
    let output = fixture.run_cli(&[
        "teleport",
        "send",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        &to,
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "send file dry-run stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["transport"], "file");
    assert_eq!(json["dry_run"], true);
    assert!(json["planned_inbox"].is_object());
    assert!(!inbox.exists());
}

#[test]
fn teleport_send_file_transport_rejects_non_file_target() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "teleport",
        "send",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--to",
        "bob@macbook",
        "--transport",
        "file",
    ]);
    assert_teleport_failure(&output, 2, "teleport/send");
}

#[test]
fn teleport_receive_waiting_inbox_returns_empty_staged() {
    let fixture = TestFixture::seeded();
    let entry = fixture.home.join("waiting-inbox").join("tp:v1:waiting");
    fs::create_dir_all(&entry).expect("entry dir");
    write_file(&entry.join("bundle.mmr.partial"), "partial");

    let output = fixture.run_cli(&[
        "teleport",
        "receive",
        "--to",
        entry.to_str().expect("entry path"),
    ]);
    assert!(
        output.status.success(),
        "receive waiting stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/receive");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["transport"], "file");
    assert!(json["staged"].as_array().expect("staged").is_empty());
    assert!(json.get("apply").is_none());
}

#[test]
fn teleport_receive_corrupt_inbox_fails_json() {
    let fixture = TestFixture::seeded();
    let entry = fixture.home.join("corrupt-inbox").join("tp:v1:bad");
    fs::create_dir_all(&entry).expect("entry dir");
    write_file(&entry.join("bundle.mmr"), "{not-json");
    write_file(&entry.join("bundle.sha256"), "sha256:deadbeef\n");
    write_file(&entry.join("ready"), "");

    let output = fixture.run_cli(&[
        "teleport",
        "receive",
        "--to",
        entry.to_str().expect("entry path"),
    ]);
    assert_teleport_failure(&output, 3, "teleport/receive");
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
}

#[test]
fn teleport_receive_hash_mismatch_fails_json() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("valid-bundle.mmr");
    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(pack_output.status.success());
    let pack_json = parse_stdout_json(&pack_output);
    let bundle_id = pack_json["bundle_id"].as_str().expect("bundle_id");

    let entry = fixture.home.join("mismatch-inbox").join(bundle_id);
    fs::create_dir_all(&entry).expect("entry dir");
    fs::copy(&bundle_path, entry.join("bundle.mmr")).expect("copy bundle");
    write_file(&entry.join("bundle.sha256"), "sha256:wrong\n");
    write_file(&entry.join("ready"), "");

    let output = fixture.run_cli(&[
        "teleport",
        "receive",
        "--to",
        entry.to_str().expect("entry path"),
    ]);
    assert_teleport_failure(&output, 3, "teleport/receive");
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "bundle_hash_mismatch");
}

#[test]
fn teleport_receive_direct_corrupt_bundle_fails_as_receive_json() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("corrupt-direct.mmr");
    write_file(&bundle_path, "{not-json");

    let output = fixture.run_cli(&[
        "teleport",
        "receive",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert_teleport_failure(&output, 3, "teleport/receive");
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/receive");
    assert_eq!(json["status"], "failed");
}

#[test]
fn teleport_receive_valid_inbox_applies_and_second_receive_is_idempotent() {
    let fixture = TestFixture::seeded();
    let inbox = fixture.home.join("receive-inbox");
    let to = format!("file://{}", inbox.display());
    let send_output = fixture.run_cli(&[
        "teleport",
        "send",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        &to,
    ]);
    assert!(send_output.status.success());
    let send_json = parse_stdout_json(&send_output);
    let bundle_id = send_json["bundle_id"].as_str().expect("bundle_id");
    let entry = inbox.join(bundle_id);

    let receive_output = fixture.run_cli(&[
        "teleport",
        "receive",
        "--to",
        entry.to_str().expect("entry path"),
        "--project",
        "/Users/test/target-proj",
    ]);
    assert!(
        receive_output.status.success(),
        "receive stderr={}",
        String::from_utf8_lossy(&receive_output.stderr)
    );
    let receive_json = parse_stdout_json(&receive_output);
    assert_eq!(receive_json["status"], "ok");
    assert_eq!(receive_json["apply"]["status"], "ok");
    assert!(
        !receive_json["staged"]
            .as_array()
            .expect("staged")
            .is_empty()
    );

    let second_output = fixture.run_cli(&[
        "teleport",
        "receive",
        "--to",
        entry.to_str().expect("entry path"),
        "--project",
        "/Users/test/target-proj",
    ]);
    assert!(second_output.status.success());
    let second_json = parse_stdout_json(&second_output);
    assert_eq!(second_json["apply"]["status"], "skipped");
}

fn spawn_teleport_serve(
    fixture: &TestFixture,
    extra_args: &[&str],
) -> (std::process::Child, serde_json::Value) {
    let mut args = vec![
        "teleport",
        "serve",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--bind",
        "127.0.0.1:0",
        "--timeout",
        "30",
    ];
    args.extend_from_slice(extra_args);

    let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(&args)
        .env("HOME", &fixture.home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn teleport serve");

    let stdout = child.stdout.take().expect("serve stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read teleport serve startup JSON");
    let startup = serde_json::from_str(&line).expect("parse serve startup JSON");
    (child, startup)
}

#[test]
fn teleport_serve_receive_http_loopback_applies_and_serve_exits() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (mut serve_child, startup) = spawn_teleport_serve(&fixture, &[]);

    assert_eq!(startup["command"], "teleport/serve");
    assert_eq!(startup["status"], "ok");
    assert_eq!(startup["transport"], "http");
    assert_eq!(startup["dry_run"], false);
    assert!(
        startup["listen_url"]
            .as_str()
            .expect("listen_url")
            .starts_with("mmtp://127.0.0.1:")
    );
    assert!(startup["token"].as_str().expect("token").len() >= 64);
    assert!(
        startup["bind_addr"]
            .as_str()
            .expect("bind_addr")
            .starts_with("127.0.0.1:")
    );

    let listen_url = startup["listen_url"].as_str().expect("listen_url");
    let receive_output = fixture.run_cli(&[
        "teleport",
        "receive",
        listen_url,
        "--project",
        "/Users/test/target-proj",
    ]);
    assert!(
        receive_output.status.success(),
        "receive stderr={}",
        String::from_utf8_lossy(&receive_output.stderr)
    );
    let receive_json = parse_stdout_json(&receive_output);
    assert_eq!(receive_json["command"], "teleport/receive");
    assert_eq!(receive_json["status"], "ok");
    assert_eq!(receive_json["transport"], "http");
    assert_eq!(receive_json["locator"], listen_url);
    assert_eq!(receive_json["apply"]["status"], "ok");
    assert!(
        !receive_json["staged"]
            .as_array()
            .expect("staged")
            .is_empty()
    );

    let serve_status = serve_child.wait().expect("wait for serve");
    assert!(
        serve_status.success(),
        "serve should exit after one download"
    );
}

#[test]
fn teleport_serve_invalid_token_does_not_consume_bundle() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (mut serve_child, startup) = spawn_teleport_serve(&fixture, &[]);
    let listen_url = startup["listen_url"].as_str().expect("listen_url");
    let token = startup["token"].as_str().expect("token");
    let bad_url = listen_url.replace(token, "0".repeat(token.len()).as_str());

    let bad_output = fixture.run_cli(&[
        "teleport",
        "receive",
        &bad_url,
        "--project",
        "/Users/test/target-proj",
    ]);
    assert_teleport_failure(&bad_output, 3, "teleport/receive");
    let bad_json = parse_stdout_json(&bad_output);
    assert_eq!(bad_json["error_kind"], "http_invalid_token");

    let good_output = fixture.run_cli(&[
        "teleport",
        "receive",
        listen_url,
        "--project",
        "/Users/test/target-proj",
    ]);
    assert!(good_output.status.success());
    let good_json = parse_stdout_json(&good_output);
    assert_eq!(good_json["apply"]["status"], "ok");

    let serve_status = serve_child.wait().expect("wait for serve");
    assert!(serve_status.success());
}

#[test]
fn teleport_serve_second_receive_fails_after_bundle_consumed() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (mut serve_child, startup) = spawn_teleport_serve(&fixture, &[]);
    let listen_url = startup["listen_url"].as_str().expect("listen_url");

    let first_output = fixture.run_cli(&[
        "teleport",
        "receive",
        listen_url,
        "--project",
        "/Users/test/target-proj",
    ]);
    assert!(first_output.status.success());
    let serve_status = serve_child.wait().expect("wait for serve");
    assert!(serve_status.success());

    let second_output = fixture.run_cli(&[
        "teleport",
        "receive",
        listen_url,
        "--project",
        "/Users/test/target-proj",
    ]);
    assert_teleport_failure(&second_output, 3, "teleport/receive");
    let second_json = parse_stdout_json(&second_output);
    assert_eq!(second_json["error_kind"], "http_connect_failed");
}

#[test]
fn teleport_serve_read_http_loopback_caches_without_apply() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    let native_before = fs::read(&native_path).ok();

    let (mut serve_child, startup) = spawn_teleport_serve(&fixture, &[]);
    let listen_url = startup["listen_url"].as_str().expect("listen_url");

    let read_output = fixture.run_cli(&["teleport", "read", listen_url]);
    assert!(
        read_output.status.success(),
        "read stderr={}",
        String::from_utf8_lossy(&read_output.stderr)
    );
    let read_json = parse_stdout_json(&read_output);
    assert_eq!(read_json["command"], "teleport/read");
    assert_eq!(read_json["status"], "ok");
    assert_eq!(read_json["transport"], "http");
    assert!(read_json.get("apply").is_none());
    assert!(!read_json["cached"].as_array().expect("cached").is_empty());
    assert!(
        read_json["bundle_path"]
            .as_str()
            .expect("bundle_path")
            .contains("/.mmr/teleport/cache/")
    );
    let messages = read_json["messages"].as_array().expect("messages");
    assert!(!messages.is_empty());
    assert_eq!(
        read_json["message_count"].as_u64().expect("message_count"),
        messages.len() as u64
    );
    assert!(read_json.get("next_command").is_none());

    let native_after = fs::read(&native_path).ok();
    assert_eq!(
        native_before, native_after,
        "read must not modify native Codex files"
    );

    let serve_status = serve_child.wait().expect("wait for serve");
    assert!(serve_status.success());
}

#[test]
fn teleport_serve_read_same_http_locator_uses_cache_after_server_exits() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (mut serve_child, startup) = spawn_teleport_serve(&fixture, &[]);
    let listen_url = startup["listen_url"].as_str().expect("listen_url");

    let first_output = fixture.run_cli(&["teleport", "read", listen_url]);
    assert!(
        first_output.status.success(),
        "first read stderr={}",
        String::from_utf8_lossy(&first_output.stderr)
    );
    let first_json = parse_stdout_json(&first_output);
    assert_eq!(first_json["status"], "ok");

    let serve_status = serve_child.wait().expect("wait for serve");
    assert!(serve_status.success());

    let second_output = fixture.run_cli(&["teleport", "read", listen_url]);
    assert!(
        second_output.status.success(),
        "second read stderr={}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    let second_json = parse_stdout_json(&second_output);
    assert_eq!(second_json["command"], "teleport/read");
    assert_eq!(second_json["status"], "skipped");
    assert_eq!(second_json["transport"], "http");
    assert_eq!(second_json["locator"], listen_url);
    assert_eq!(second_json["bundle_id"], first_json["bundle_id"]);
    assert_eq!(second_json["bundle_path"], first_json["bundle_path"]);
    assert_eq!(
        second_json["messages"].as_array().expect("messages").len(),
        first_json["messages"].as_array().expect("messages").len()
    );
}

#[test]
fn teleport_read_local_bundle_caches_and_second_read_is_skipped() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("read-handoff.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let first_output = fixture.run_cli(&[
        "teleport",
        "read",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        first_output.status.success(),
        "first read stderr={}",
        String::from_utf8_lossy(&first_output.stderr)
    );
    let first_json = parse_stdout_json(&first_output);
    assert_eq!(first_json["command"], "teleport/read");
    assert_eq!(first_json["status"], "ok");
    assert!(
        !first_json["messages"]
            .as_array()
            .expect("messages")
            .is_empty()
    );
    let cache_path = first_json["bundle_path"].as_str().expect("bundle_path");
    assert!(Path::new(cache_path).exists());

    let second_output = fixture.run_cli(&["teleport", "read", cache_path]);
    assert!(second_output.status.success());
    let second_json = parse_stdout_json(&second_output);
    assert_eq!(second_json["status"], "skipped");
    assert_eq!(
        second_json["messages"].as_array().expect("messages").len(),
        first_json["messages"].as_array().expect("messages").len()
    );
}

#[test]
fn teleport_read_output_format_md_prints_readable_messages() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("read-md.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let output = fixture.run_cli(&[
        "teleport",
        "read",
        bundle_path.to_str().expect("bundle path"),
        "-O",
        "md",
    ]);
    assert!(
        output.status.success(),
        "read md stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/read");
    assert_eq!(json["status"], "ok");
    let text = json["text"].as_str().expect("markdown text");
    assert!(text.contains("# Teleport read"));
    assert!(text.contains("- source: codex"));
    assert!(text.contains("- session_id: sess-codex-1"));
    assert!(text.contains("hello from codex"));
    assert!(text.contains("short codex answer"));
}

#[test]
fn teleport_read_http_locator_cache_falls_back_to_refetch_when_cached_bundle_is_missing() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (mut first_serve_child, first_startup) = spawn_teleport_serve(&fixture, &[]);
    let first_url = first_startup["listen_url"].as_str().expect("listen_url");

    let first_output = fixture.run_cli(&["teleport", "read", first_url]);
    assert!(
        first_output.status.success(),
        "first read stderr={}",
        String::from_utf8_lossy(&first_output.stderr)
    );
    let first_json = parse_stdout_json(&first_output);
    assert_eq!(first_json["status"], "ok");
    let cached_bundle_path = first_json["bundle_path"].as_str().expect("bundle_path");
    fs::remove_file(cached_bundle_path).expect("remove cached bundle to force refetch");

    let first_serve_status = first_serve_child.wait().expect("wait for first serve");
    assert!(first_serve_status.success());

    let (mut second_serve_child, second_startup) = spawn_teleport_serve(&fixture, &[]);
    let second_url = second_startup["listen_url"].as_str().expect("listen_url");
    let second_output = fixture.run_cli(&["teleport", "read", second_url]);
    assert!(
        second_output.status.success(),
        "second read stderr={}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    let second_json = parse_stdout_json(&second_output);
    assert_eq!(second_json["status"], "ok");
    assert_eq!(second_json["bundle_id"], first_json["bundle_id"]);
    assert_eq!(second_json["bundle_path"], first_json["bundle_path"]);
    assert!(Path::new(cached_bundle_path).exists());

    let second_serve_status = second_serve_child.wait().expect("wait for second serve");
    assert!(second_serve_status.success());
}

#[test]
fn teleport_read_dry_run_does_not_write_cache() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("read-dry-run.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let output = fixture.run_cli(&[
        "teleport",
        "read",
        bundle_path.to_str().expect("bundle path"),
        "--dry-run",
    ]);
    assert!(
        output.status.success(),
        "read dry-run stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/read");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["dry_run"], true);
    assert!(
        json["bundle_id"]
            .as_str()
            .expect("bundle_id")
            .starts_with("tp:v1:")
    );
    assert_eq!(json["session"]["source"], "codex");
    assert_eq!(json["session"]["source_session_id"], "sess-codex-1");
    assert!(!json["messages"].as_array().expect("messages").is_empty());
    assert_eq!(
        json["message_count"].as_u64().expect("message_count"),
        json["messages"].as_array().expect("messages").len() as u64
    );
    let cache_path = json["bundle_path"].as_str().expect("bundle_path");
    assert!(
        cache_path.contains("/.mmr/teleport/cache/"),
        "bundle_path should be a planned cache path"
    );
    assert!(
        !Path::new(cache_path).exists(),
        "dry-run must not write cache file"
    );
}

fn pack_codex_teleport_bundle(fixture: &TestFixture, bundle_path: &Path) {
    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        pack_output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&pack_output.stderr)
    );
}

fn assert_teleport_unsupported(output: &Output, expected_command: &str) {
    assert_eq!(
        output.status.code(),
        Some(3),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(output);
    assert_eq!(json["status"], "unsupported");
    assert_eq!(json["command"], expected_command);
    assert!(json["message"].is_string());
}

#[test]
fn teleport_resume_default_applies_and_second_resume_is_idempotent() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-resume.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove seeded native session before resume");

    let resume_output = fixture.run_cli(&[
        "teleport",
        "resume",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        resume_output.status.success(),
        "resume stderr={}",
        String::from_utf8_lossy(&resume_output.stderr)
    );
    let resume_json = parse_stdout_json(&resume_output);
    assert_eq!(resume_json["command"], "teleport/resume");
    assert_eq!(resume_json["status"], "ok");
    assert_eq!(resume_json["requested_as"], "same");
    assert_eq!(resume_json["target_agent"], "codex");
    assert_eq!(resume_json["apply"]["status"], "ok");
    assert_eq!(resume_json["agent"]["provider"], "codex");
    assert_eq!(resume_json["agent"]["executed"], false);
    assert!(
        resume_json["agent"]["command"]
            .as_str()
            .unwrap()
            .contains("codex exec resume sess-codex-1")
    );
    assert!(
        resume_json["agent"]["manual_steps"]
            .as_array()
            .expect("manual_steps")
            .iter()
            .any(|step| step
                .as_str()
                .unwrap()
                .contains("codex exec resume sess-codex-1"))
    );
    assert!(native_path.is_file());

    let second_output = fixture.run_cli(&[
        "teleport",
        "resume",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(second_output.status.success());
    let second_json = parse_stdout_json(&second_output);
    assert_eq!(second_json["status"], "skipped");
    assert_eq!(second_json["apply"]["status"], "skipped");
}

#[test]
fn teleport_resume_as_claude_returns_structured_unsupported() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-resume-claude.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let output = fixture.run_cli(&[
        "teleport",
        "resume",
        bundle_path.to_str().expect("bundle path"),
        "--as",
        "claude",
    ]);
    assert_teleport_unsupported(&output, "teleport/resume");
    let json = parse_stdout_json(&output);
    assert_eq!(json["requested_as"], "claude");
    assert_eq!(json["target_agent"], "claude");
    assert!(json.get("apply").is_none());
}

#[test]
fn teleport_resume_as_native_is_usage_error() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-resume-native.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let output = fixture.run_cli(&[
        "teleport",
        "resume",
        bundle_path.to_str().expect("bundle path"),
        "--as",
        "native",
    ]);
    assert_teleport_failure(&output, 2, "teleport/resume");
}

#[test]
fn teleport_resume_rejects_positional_and_to_together() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-resume-locator.mmr");
    fs::write(&bundle_path, "{}").expect("write dummy bundle");

    let output = fixture.run_cli(&[
        "teleport",
        "resume",
        bundle_path.to_str().expect("bundle path"),
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert_teleport_failure(&output, 2, "teleport/resume");
}

#[test]
fn teleport_export_writes_native_transcript_as_codex() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-export.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);
    let out_path = fixture.home.join("exported-transcript.jsonl");

    let output = fixture.run_cli(&[
        "teleport",
        "export",
        bundle_path.to_str().expect("bundle path"),
        "--to",
        out_path.to_str().expect("out path"),
        "--as",
        "codex",
    ]);
    assert!(
        output.status.success(),
        "export stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "teleport/export");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["target_format"], "codex");
    assert_eq!(json["requested_as"], "codex");
    assert!(json["bytes"].as_u64().unwrap() > 0);
    assert!(out_path.is_file());
}

#[test]
fn teleport_export_as_claude_returns_structured_unsupported() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-export-claude.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);
    let out_path = fixture.home.join("exported-claude.jsonl");

    let output = fixture.run_cli(&[
        "teleport",
        "export",
        bundle_path.to_str().expect("bundle path"),
        "--to",
        out_path.to_str().expect("out path"),
        "--as",
        "claude",
    ]);
    assert_teleport_unsupported(&output, "teleport/export");
    let json = parse_stdout_json(&output);
    assert_eq!(json["requested_as"], "claude");
    assert_eq!(json["target_format"], "claude");
    assert!(!out_path.exists());
}

#[test]
fn teleport_export_missing_to_is_usage_error() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-export-missing-to.mmr");
    pack_codex_teleport_bundle(&fixture, &bundle_path);

    let output = fixture.run_cli(&[
        "teleport",
        "export",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert_teleport_failure(&output, 2, "teleport/export");
}

#[test]
fn teleport_provider_profile_dispatch_unknown_provider() {
    match mmr::teleport::profile_for("unknown-provider") {
        Err(err) => assert!(err.message.contains("unsupported teleport provider")),
        Ok(_) => panic!("expected unknown provider to fail"),
    }
}

#[derive(Clone, Copy)]
struct TeleportProviderCase {
    source: &'static str,
    session_id: &'static str,
    project: &'static str,
    target_project: &'static str,
    query_project: &'static str,
}

fn teleport_provider_cases() -> [TeleportProviderCase; 5] {
    [
        TeleportProviderCase {
            source: "codex",
            session_id: "sess-codex-1",
            project: "/Users/test/codex-proj",
            target_project: "/Users/test/target-codex-proj",
            query_project: "target-codex-proj",
        },
        TeleportProviderCase {
            source: "claude",
            session_id: "sess-claude-1",
            project: "/Users/test/proj",
            target_project: "/Users/test/target-claude-proj",
            query_project: "target-claude-proj",
        },
        TeleportProviderCase {
            source: "cursor",
            session_id: "sess-cursor-1",
            project: "-Users-test-cursor-proj",
            target_project: "/Users/test/target-cursor-proj",
            query_project: "-Users-test-target-cursor-proj",
        },
        TeleportProviderCase {
            source: "grok",
            session_id: "sess-grok-1",
            project: "/Users/test/grok-proj",
            target_project: "/Users/test/target-grok-proj",
            query_project: "target-grok-proj",
        },
        TeleportProviderCase {
            source: "pi",
            session_id: "sess-pi-1",
            project: "/Users/test/pi-proj",
            target_project: "/Users/test/target-pi-proj",
            query_project: "target-pi-proj",
        },
    ]
}

fn expected_native_artifacts(source: &str) -> &'static [&'static str] {
    match source {
        "codex" => &["native/codex/transcript.jsonl"],
        "claude" => &["native/claude/transcript.jsonl"],
        "cursor" => &["native/cursor/transcript.jsonl"],
        "grok" => &["native/grok/summary.json", "native/grok/updates.jsonl"],
        "pi" => &["native/pi/transcript.jsonl"],
        _ => &[],
    }
}

fn run_cli_with_project_arg(
    fixture: &TestFixture,
    before_project: &[&str],
    project: &str,
    after_project: &[&str],
) -> Output {
    let mut args = before_project
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    if project.starts_with('-') {
        args.push(format!("--project={project}"));
    } else {
        args.push("--project".to_string());
        args.push(project.to_string());
    }
    args.extend(after_project.iter().map(|arg| (*arg).to_string()));
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    fixture.run_cli(&refs)
}

#[test]
fn teleport_provider_matrix_pack_inspect_apply() {
    for case in teleport_provider_cases() {
        let fixture = TestFixture::seeded();
        let bundle_path = fixture
            .home
            .join(format!("teleport-matrix-{}.mmr", case.source));

        let pack_output = run_cli_with_project_arg(
            &fixture,
            &[
                "teleport",
                "pack",
                "--source",
                case.source,
                "--session",
                case.session_id,
            ],
            case.project,
            &["--to", bundle_path.to_str().expect("bundle path")],
        );
        assert!(
            pack_output.status.success(),
            "{} pack stderr={}",
            case.source,
            String::from_utf8_lossy(&pack_output.stderr)
        );
        let pack_json = parse_stdout_json(&pack_output);
        assert_eq!(pack_json["session"]["source"], case.source);
        assert_eq!(pack_json["session"]["source_session_id"], case.session_id);
        let artifacts = pack_json["artifacts"].as_array().expect("artifacts");
        for expected_path in expected_native_artifacts(case.source) {
            assert!(
                artifacts
                    .iter()
                    .any(|artifact| artifact["path"] == *expected_path),
                "{} bundle should include {expected_path}",
                case.source
            );
        }

        let inspect_output = fixture.run_cli(&[
            "teleport",
            "inspect",
            bundle_path.to_str().expect("bundle path"),
        ]);
        assert!(inspect_output.status.success());
        let inspect_json = parse_stdout_json(&inspect_output);
        assert_eq!(inspect_json["apply_ready"], true);

        let read_output = fixture.run_cli(&[
            "teleport",
            "read",
            bundle_path.to_str().expect("bundle path"),
        ]);
        assert!(read_output.status.success());
        let read_json = parse_stdout_json(&read_output);
        assert!(read_json.get("apply").is_none());

        let apply_output = fixture.run_cli(&[
            "teleport",
            "apply",
            bundle_path.to_str().expect("bundle path"),
            "--project",
            case.target_project,
            "--force",
        ]);
        assert!(
            apply_output.status.success(),
            "{} apply stderr={}",
            case.source,
            String::from_utf8_lossy(&apply_output.stderr)
        );
        let apply_json = parse_stdout_json(&apply_output);
        assert_eq!(apply_json["target_project"], case.target_project);
        assert_eq!(apply_json["path_remap_applied"], true);

        let messages_output = run_cli_with_project_arg(
            &fixture,
            &[
                "messages",
                "--source",
                case.source,
                "--session",
                case.session_id,
            ],
            case.query_project,
            &[],
        );
        assert!(
            messages_output.status.success(),
            "{} messages stderr={}",
            case.source,
            String::from_utf8_lossy(&messages_output.stderr)
        );
        let messages_json = parse_stdout_json(&messages_output);
        assert!(
            messages_json["total_messages"].as_i64().unwrap() > 0,
            "{} applied bundle should be visible through mmr messages",
            case.source
        );

        let resume_output = fixture.run_cli(&[
            "teleport",
            "resume",
            bundle_path.to_str().expect("bundle path"),
            "--project",
            case.target_project,
            "--as",
            "same",
            "--force",
        ]);
        assert!(
            resume_output.status.success(),
            "{} resume stderr={}",
            case.source,
            String::from_utf8_lossy(&resume_output.stderr)
        );
        let resume_json = parse_stdout_json(&resume_output);
        assert_eq!(resume_json["target_agent"], case.source);

        let export_path = if case.source == "grok" {
            fixture.home.join("exported-grok")
        } else {
            fixture.home.join(format!("exported-{}.jsonl", case.source))
        };
        let export_output = fixture.run_cli(&[
            "teleport",
            "export",
            bundle_path.to_str().expect("bundle path"),
            "--to",
            export_path.to_str().expect("export path"),
            "--as",
            "same",
        ]);
        assert!(
            export_output.status.success(),
            "{} export stderr={}",
            case.source,
            String::from_utf8_lossy(&export_output.stderr)
        );
        if case.source == "grok" {
            assert!(export_path.join("summary.json").is_file());
            assert!(export_path.join("updates.jsonl").is_file());
        } else {
            assert!(export_path.is_file());
        }
    }
}

#[test]
fn teleport_provider_matrix_pack_latest() {
    for case in teleport_provider_cases() {
        let fixture = TestFixture::seeded();
        let output = run_cli_with_project_arg(
            &fixture,
            &["teleport", "pack", "--source", case.source],
            case.project,
            &["--dry-run"],
        );
        assert!(
            output.status.success(),
            "{} latest pack stderr={}",
            case.source,
            String::from_utf8_lossy(&output.stderr)
        );
        let json = parse_stdout_json(&output);
        assert_eq!(json["session"]["source"], case.source);
        assert_eq!(json["session"]["source_session_id"], case.session_id);
    }
}

#[test]
fn teleport_provider_matrix_file_send_read_receive() {
    for case in teleport_provider_cases() {
        let fixture = TestFixture::seeded();
        let inbox = fixture.home.join(format!("provider-inbox-{}", case.source));
        let to = format!("file://{}", inbox.display());
        let send_output = run_cli_with_project_arg(
            &fixture,
            &[
                "teleport",
                "send",
                "--source",
                case.source,
                "--session",
                case.session_id,
            ],
            case.project,
            &["--to", &to, "--transport", "file"],
        );
        assert!(
            send_output.status.success(),
            "{} send stderr={}",
            case.source,
            String::from_utf8_lossy(&send_output.stderr)
        );
        let send_json = parse_stdout_json(&send_output);
        assert_eq!(send_json["session"]["source"], case.source);
        let entry = inbox.join(send_json["bundle_id"].as_str().expect("bundle_id"));

        let read_output =
            fixture.run_cli(&["teleport", "read", entry.to_str().expect("entry path")]);
        assert!(
            read_output.status.success(),
            "{} read stderr={}",
            case.source,
            String::from_utf8_lossy(&read_output.stderr)
        );
        let read_json = parse_stdout_json(&read_output);
        assert_eq!(read_json["session"]["source"], case.source);
        assert!(
            !read_json["messages"]
                .as_array()
                .expect("messages")
                .is_empty()
        );
        assert!(read_json.get("apply").is_none());

        let receive_output = fixture.run_cli(&[
            "teleport",
            "receive",
            "--to",
            entry.to_str().expect("entry path"),
            "--project",
            case.target_project,
            "--force",
        ]);
        assert!(
            receive_output.status.success(),
            "{} receive stderr={}",
            case.source,
            String::from_utf8_lossy(&receive_output.stderr)
        );
        let receive_json = parse_stdout_json(&receive_output);
        assert_eq!(receive_json["apply"]["target_project"], case.target_project);
    }
}

#[test]
fn teleport_cross_provider_resume_is_structured_unsupported() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-cross.mmr");
    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "grok",
        "--session",
        "sess-grok-1",
        "--project",
        "/Users/test/grok-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(pack_output.status.success());

    let output = fixture.run_cli(&[
        "teleport",
        "resume",
        bundle_path.to_str().expect("bundle path"),
        "--as",
        "codex",
    ]);
    assert_teleport_unsupported(&output, "teleport/resume");
}

#[test]
fn teleport_cross_provider_export_is_structured_unsupported() {
    let fixture = TestFixture::seeded();
    let bundle_path = fixture.home.join("teleport-cross-export.mmr");
    let pack_output = fixture.run_cli(&[
        "teleport",
        "pack",
        "--source",
        "grok",
        "--session",
        "sess-grok-1",
        "--project",
        "/Users/test/grok-proj",
        "--to",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(pack_output.status.success());

    let out_path = fixture.home.join("cross-export-claude.jsonl");
    let output = fixture.run_cli(&[
        "teleport",
        "export",
        bundle_path.to_str().expect("bundle path"),
        "--to",
        out_path.to_str().expect("out path"),
        "--as",
        "claude",
    ]);
    assert_teleport_unsupported(&output, "teleport/export");
    assert!(!out_path.exists());
}
