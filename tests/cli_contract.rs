#[allow(dead_code)]
mod common;

use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use common::{RetrieveContractFixture, TestFixture, parse_stdout_json};

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

#[test]
fn root_version_flag_is_available_for_ssh_share_probe() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_raw(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_text(&output).trim(),
        format!("mmr {}", env!("CARGO_PKG_VERSION"))
    );
}

fn loopback_bind_available() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
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

fn run_cli_with_home_in_dir(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(args)
        .env("HOME", home)
        .current_dir(cwd)
        .output()
        .expect("run mmr")
}

fn run_cli_with_home_and_env(home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
    command
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("MMR_CONFIG_FILE");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run mmr")
}

fn start_mock_chat_completions_server(
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

fn start_mock_compact_server(
    response_body: &str,
) -> (
    String,
    Arc<Mutex<Option<serde_json::Value>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock compact server");
    let addr = listener.local_addr().expect("local addr");
    let captured = Arc::new(Mutex::new(None));
    let captured_for_thread = Arc::clone(&captured);
    let response = response_body.to_string();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept compact request");
        let request_bytes = read_http_request(&mut stream);
        let request = String::from_utf8(request_bytes).expect("request UTF-8");
        let first_line = request.lines().next().unwrap_or_default();
        assert!(
            first_line.starts_with("POST /compact HTTP/1.1"),
            "compact client should call /compact, got {first_line}"
        );
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

// --- skill ---

#[test]
fn skill_load_prints_bundled_skill_for_agent_context() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["skill", "load"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = stdout_text(&output);
    assert!(stdout.starts_with("# mmr skill bundle"));
    assert!(stdout.contains("## mmr/SKILL.md"));
    assert!(stdout.contains("name: mmr"));
    assert!(stdout.contains("`mmr` is the local Rust CLI"));
    assert!(stdout.contains("## mmr/session-mining/SKILL.md"));
    assert!(stdout.contains("session-mining"));
}

#[test]
fn skill_install_replaces_user_scoped_skill() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let target = home.join(".agents").join("skills").join("mmr");
    write_file(&target.join("stale.txt"), "remove me");

    let output = run_cli_with_home(&home, &["skill", "install"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "skill/install");
    assert_eq!(json["scope"], "user");
    assert_eq!(json["path"], target.display().to_string());
    assert_eq!(json["replaced"], true);
    assert!(target.join("SKILL.md").is_file());
    assert!(target.join("session-mining").join("SKILL.md").is_file());
    assert!(
        target
            .join("session-mining")
            .join("references")
            .join("session-retrieval-patterns.md")
            .is_file()
    );
    assert!(!target.join("stale.txt").exists());
}

#[test]
fn skill_install_local_replaces_project_scoped_skill() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let project = tmp.path().join("project");
    fs::create_dir_all(&home).expect("create home");
    fs::create_dir_all(&project).expect("create project");
    let target = project.join(".agents").join("skills").join("mmr");
    write_file(&target.join("stale.txt"), "remove me");

    let output = run_cli_with_home_in_dir(&home, &project, &["skill", "install", "--local"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    let canonical_target = fs::canonicalize(&target).expect("canonical target");
    assert_eq!(json["command"], "skill/install");
    assert_eq!(json["scope"], "local");
    assert_eq!(json["path"], canonical_target.display().to_string());
    assert_eq!(json["replaced"], true);
    assert!(target.join("SKILL.md").is_file());
    assert!(target.join("session-mining").join("SKILL.md").is_file());
    assert!(!target.join("stale.txt").exists());
}

// --- projects ---

#[test]
fn projects_without_source_returns_all_sources() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["list", "projects"]);

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
    let output = fixture.run_cli(&["--source", "codex", "list", "projects"]);

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
        "list",
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
        "list",
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
    let output = fixture.run_cli(&["--source", "all", "list", "projects"]);
    assert!(
        !output.status.success(),
        "--source all should not be accepted"
    );
}

#[test]
fn removed_top_level_commands_are_rejected() {
    let fixture = TestFixture::seeded();
    for command in [
        "projects", "sessions", "messages", "export", "prev", "summary", "remember", "dream",
        "search", "rg", "link",
    ] {
        let output = fixture.run_cli_raw(&[command]);
        assert!(
            !output.status.success(),
            "{command} should be removed as a top-level command"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unrecognized subcommand") || stderr.contains("unknown subcommand"),
            "stderr should reject removed {command}: {stderr}"
        );
    }
}

#[test]
fn list_projects_replaces_projects() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["list", "projects"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert!(json["projects"].as_array().unwrap().len() >= 5);
}

#[test]
fn list_sessions_replaces_sessions() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "list", "sessions", "--all"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 3);
}

#[test]
fn read_session_replaces_messages_session_lookup() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["read", "session", "sess-claude-1"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().all(|m| m["session_id"] == "sess-claude-1"));
}

#[test]
fn read_project_replaces_export_project_history() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["read", "project", "--project", "Users/test/codex-proj"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    let timestamps = json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["timestamp"].as_str().unwrap())
        .collect::<Vec<_>>();
    let mut sorted = timestamps.clone();
    sorted.sort();
    assert_eq!(timestamps, sorted);
}

#[test]
fn read_source_requires_explicit_source_and_reads_across_projects() {
    let fixture = TestFixture::seeded();
    let missing_source = fixture.run_cli(&["read", "source"]);
    assert!(!missing_source.status.success());
    assert!(
        String::from_utf8_lossy(&missing_source.stderr).contains("requires --source"),
        "stderr={}",
        String::from_utf8_lossy(&missing_source.stderr)
    );

    let output = fixture.run_cli(&["--source", "grok", "read", "source"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 3);
    assert!(
        json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["source"] == "grok")
    );
}

#[test]
fn recall_replaces_prev_for_previous_stable_session() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let output =
        fixture.run_cli_in_dir_with_env(&["recall"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    let selected = json["session_selection"]["selected"].as_array().unwrap();
    assert_eq!(selected[0]["age"].as_u64().unwrap(), 1);
    assert_eq!(
        selected[0]["session_id"].as_str().unwrap(),
        "sess-cwd-codex"
    );
}

#[test]
fn projects_with_source_cursor_filters() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "cursor", "list", "projects"]);

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
    let output = fixture.run_cli_with_env(&["--source", "cursor", "read", "source"], &[]);

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
    let output =
        fixture.run_cli_with_env(&["--source", "cursor", "list", "sessions", "--all"], &[]);

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
    let output = fixture.run_cli(&["--source", "grok", "list", "projects"]);

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
    let output = fixture.run_cli_with_env(&["--source", "grok", "read", "source"], &[]);

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
    let output = fixture.run_cli_with_env(&["--source", "grok", "list", "sessions", "--all"], &[]);

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
    let output = fixture.run_cli(&["--source", "pi", "list", "projects"]);

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
    let output = fixture.run_cli_with_env(&["--source", "pi", "read", "source"], &[]);

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
    let output = fixture.run_cli_with_env(&["--source", "pi", "list", "sessions", "--all"], &[]);

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
    let output = fixture.run_cli_in_dir_with_env(
        &["list", "sessions"],
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
        &["list", "sessions", "--all"],
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

    let output = fixture.run_cli_in_dir_with_env(
        &["list", "sessions"],
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
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 0);
    assert!(sessions.is_empty());
}

#[test]
fn auto_discover_project_env_controls_default_scope_for_list_sessions() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let disabled_sessions = fixture.run_cli_in_dir_with_env(
        &["list", "sessions"],
        &cwd,
        &[("MMR_AUTO_DISCOVER_PROJECT", "0")],
    );
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

    let enabled_sessions = fixture.run_cli_in_dir_with_env(
        &["list", "sessions"],
        &cwd,
        &[("MMR_AUTO_DISCOVER_PROJECT", "1")],
    );
    assert!(
        enabled_sessions.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&enabled_sessions.stderr)
    );
    let enabled_sessions_json = parse_stdout_json(&enabled_sessions);
    assert_eq!(enabled_sessions_json["total_sessions"].as_i64().unwrap(), 2);

    let read_project = fixture.run_cli_in_dir_with_env(
        &["read", "project"],
        &cwd,
        &[("MMR_AUTO_DISCOVER_PROJECT", "0")],
    );
    assert!(
        read_project.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&read_project.stderr)
    );
    let read_project_json = parse_stdout_json(&read_project);
    assert_eq!(read_project_json["total_messages"].as_i64().unwrap(), 4);
}

#[test]
fn sessions_with_source_only() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "codex", "list", "sessions", "--all"], &[]);

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
    let output = fixture.run_cli(&["list", "sessions", "--project", "Users/test/codex-proj"]);

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
        "list",
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
            "list",
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
            "list",
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
            "list",
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
    let output = fixture.run_cli_in_dir_with_env(
        &["read", "project"],
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
fn messages_returns_empty_for_discovered_but_empty_project() {
    let fixture = TestFixture::seeded();
    let cwd = seed_empty_discovered_project(&fixture);

    let output = fixture.run_cli_in_dir_with_env(
        &["read", "project"],
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
    assert_eq!(json["total_messages"].as_i64().unwrap(), 0);
    assert!(messages.is_empty());
}

#[test]
fn read_session_is_chronological_and_paginated() {
    let fixture = TestFixture::seeded();

    let all_output = fixture.run_cli(&["read", "session", "sess-claude-1"]);
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
        "read",
        "session",
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
fn messages_filtered_by_source() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(&["--source", "claude", "read", "source"], &[]);

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
        "read",
        "project",
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
fn messages_filtered_by_project_basename_alias() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "read",
        "project",
        "--project",
        "codex-proj",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"].as_i64().unwrap(), 6);
    let messages = json["messages"].as_array().unwrap();
    assert!(!messages.is_empty());
    for msg in messages {
        assert_eq!(
            msg["project_name"].as_str().unwrap(),
            "/Users/test/codex-proj"
        );
    }
}

#[test]
fn sessions_filtered_by_generated_project_alias() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "list",
        "sessions",
        "--project=-Users-test-codex-proj",
    ]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 2);
    let sessions = json["sessions"].as_array().unwrap();
    assert!(
        sessions.iter().all(|session| {
            session["project_name"].as_str().unwrap() == "/Users/test/codex-proj"
        })
    );
}

#[test]
fn default_source_empty_string_keeps_both_sources() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let output = fixture.run_cli_in_dir_with_env(
        &["list", "sessions", "--all"],
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
        &["list", "sessions", "--all"],
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
        &["--source", "claude", "read", "source"],
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

// --- retrieve ---

#[test]
fn retrieve_parser_accepts_documented_flags() {
    let fixture = RetrieveContractFixture::seeded();
    let pinned = format!(
        r#"{{"source":"codex","project_name":"{}","source_session_id":"retrieve-codex-alpha"}}"#,
        fixture.project_arg()
    );
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "retrieve",
        "public mapping marker",
        "--project",
        fixture.project_arg(),
        "--session",
        "retrieve-codex-alpha",
        "--role",
        "assistant",
        "--event-type",
        "message",
        "--ignore-case",
        "-C",
        "1",
        "--max-sessions",
        "1",
        "--before-messages",
        "1",
        "--after-messages",
        "2",
        "--max-messages-per-session",
        "3",
        "--limit",
        "2",
        "--offset",
        "0",
        "--pinned-session",
        pinned.as_str(),
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["limits"]["max_sessions"].as_u64().unwrap(), 1);
    assert_eq!(json["limits"]["before_messages"].as_u64().unwrap(), 1);
    assert_eq!(json["limits"]["after_messages"].as_u64().unwrap(), 2);
    assert_eq!(
        json["limits"]["max_messages_per_session"].as_u64().unwrap(),
        3
    );
    assert_eq!(json["limits"]["limit"].as_u64().unwrap(), 2);
    assert_eq!(json["limits"]["offset"].as_u64().unwrap(), 0);
    assert!(json["suggested_next_action"].is_null());
    let selected = &json["selected_sessions"][0];
    assert_eq!(selected["rank"].as_u64().unwrap(), 1);
    assert_eq!(selected["match_count"].as_u64().unwrap(), 1);
    assert_eq!(
        selected["rank_reason"]["tie_break"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn retrieve_parser_rejects_out_of_scope_all_and_remote_flags() {
    let fixture = RetrieveContractFixture::seeded();

    let all = fixture.run_cli(&["retrieve", "public mapping marker", "--all"]);
    assert!(!all.status.success(), "--all is outside retrieve v1");
    assert!(
        String::from_utf8_lossy(&all.stderr).contains("--all"),
        "stderr={}",
        String::from_utf8_lossy(&all.stderr)
    );

    let remote = fixture.run_cli(&["retrieve", "public mapping marker", "--remote", "mini"]);
    assert!(!remote.status.success(), "--remote is outside retrieve v1");
    assert!(
        String::from_utf8_lossy(&remote.stderr).contains("--remote"),
        "stderr={}",
        String::from_utf8_lossy(&remote.stderr)
    );
}

#[test]
fn retrieve_scope_flags_reject_ambiguous_combinations() {
    let fixture = RetrieveContractFixture::seeded();

    let project_and_all = fixture.run_cli(&[
        "retrieve",
        "system wide marker",
        "--project",
        fixture.project_arg(),
        "--all-projects",
    ]);
    assert!(!project_and_all.status.success());
    let project_json = parse_stdout_json(&project_and_all);
    assert_eq!(project_json["error_kind"], "invalid_scope_flags");

    let source_and_all = fixture.run_cli(&[
        "--source",
        "codex",
        "retrieve",
        "system wide marker",
        "--all-sources",
    ]);
    assert!(!source_and_all.status.success());
    let source_json = parse_stdout_json(&source_and_all);
    assert_eq!(source_json["error_kind"], "invalid_scope_flags");
}

#[test]
fn retrieve_stdout_pretty_is_json_and_stderr_is_diagnostic_only() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&["retrieve", "public mapping marker", "--pretty"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.stderr.is_empty(),
        "successful JSON retrieve should not print stderr diagnostics"
    );
    let stdout = stdout_text(&output);
    assert!(stdout.starts_with("{\n  \"query\":"));
    let json = parse_stdout_json(&output);
    assert_eq!(json["query"], "public mapping marker");
    assert!(json["selected_sessions"].is_array());
    assert!(json["unreadable_matches"].is_array());
}

#[test]
fn retrieve_all_projects_searches_provider_discovered_projects() {
    let fixture = RetrieveContractFixture::seeded();

    let scoped = fixture.run_cli(&["retrieve", "provider only marker"]);
    assert!(
        scoped.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&scoped.stderr)
    );
    let scoped_json = parse_stdout_json(&scoped);
    assert_eq!(scoped_json["total_matches"].as_u64().unwrap(), 0);
    assert_eq!(scoped_json["scope"]["all_projects"], false);
    assert_eq!(
        scoped_json["scope"]["projects"].as_array().unwrap().len(),
        1
    );

    let all_projects = fixture.run_cli(&[
        "retrieve",
        "provider only marker",
        "--all-projects",
        "--max-sessions",
        "5",
    ]);
    assert!(
        all_projects.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&all_projects.stderr)
    );
    let all_json = parse_stdout_json(&all_projects);
    assert_eq!(all_json["scope"]["all_projects"], true);
    assert!(
        all_json["scope"]["projects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|project| project.as_str().unwrap() == fixture.provider_only_project_arg())
    );
    let selected = all_json["selected_sessions"].as_array().unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0]["project_name"],
        fixture.provider_only_project_arg()
    );
    assert_eq!(
        selected[0]["source_session_id"],
        "retrieve-codex-provider-only"
    );
    assert!(
        selected[0]["first_match_citation"]
            .as_str()
            .unwrap()
            .starts_with("mmr://message/message:v1:")
    );
}

#[test]
fn retrieve_filters_apply_source_env_session_role_event_and_context() {
    let fixture = RetrieveContractFixture::seeded();

    let default_source = fixture.run_cli_with_env(
        &[
            "retrieve",
            "ranking tie marker",
            "--max-sessions",
            "3",
            "--event-type",
            "message",
            "--role",
            "assistant",
            "-C",
            "1",
        ],
        &[("MMR_DEFAULT_SOURCE", "claude")],
    );
    assert!(
        default_source.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&default_source.stderr)
    );
    let default_json = parse_stdout_json(&default_source);
    assert!(
        default_json["selected_sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|session| session["source"] == "claude")
    );
    assert!(
        default_json["selected_sessions"][0]["matches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["before"].is_array() && item["after"].is_array())
    );

    let explicit_source = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "retrieve",
            "ranking tie marker",
            "--session",
            "retrieve-codex-beta",
            "--max-sessions",
            "3",
        ],
        &[("MMR_DEFAULT_SOURCE", "claude")],
    );
    assert!(
        explicit_source.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&explicit_source.stderr)
    );
    let explicit_json = parse_stdout_json(&explicit_source);
    let sessions = explicit_json["selected_sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["source"], "codex");
    assert_eq!(sessions[0]["source_session_id"], "retrieve-codex-beta");

    let all_sources = fixture.run_cli_with_env(
        &[
            "retrieve",
            "ranking tie marker",
            "--all-sources",
            "--max-sessions",
            "3",
        ],
        &[("MMR_DEFAULT_SOURCE", "claude")],
    );
    assert!(
        all_sources.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&all_sources.stderr)
    );
    let all_sources_json = parse_stdout_json(&all_sources);
    let sources = all_sources_json["selected_sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["source"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(sources.contains(&"claude"));
    assert!(sources.contains(&"codex"));
    assert_eq!(all_sources_json["scope"]["all_sources"], true);
    assert!(all_sources_json["scope"]["source_filter"].is_null());
}

#[test]
fn retrieve_limit_default_derives_from_session_caps() {
    let fixture = RetrieveContractFixture::seeded();
    let output = fixture.run_cli(&[
        "retrieve",
        "ranking tie marker",
        "--max-sessions",
        "2",
        "--max-messages-per-session",
        "5",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["limits"]["limit"].as_u64().unwrap(), 10);
}

#[test]
fn retrieve_flattened_pagination_across_selected_sessions() {
    let fixture = RetrieveContractFixture::seeded();
    let page1 = fixture.run_cli(&[
        "retrieve",
        "ranking tie marker",
        "--max-sessions",
        "2",
        "--max-messages-per-session",
        "4",
        "--limit",
        "2",
    ]);
    assert!(
        page1.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page1.stderr)
    );
    let page1_json = parse_stdout_json(&page1);
    let selected = page1_json["selected_sessions"].as_array().unwrap();
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0]["rank"].as_u64().unwrap(), 1);
    assert_eq!(selected[1]["rank"].as_u64().unwrap(), 2);
    let total_page1_messages = selected
        .iter()
        .map(|session| session["messages"].as_array().unwrap().len())
        .sum::<usize>();
    assert_eq!(total_page1_messages, 2);
    assert_eq!(selected[0]["messages"].as_array().unwrap().len(), 2);
    assert!(selected[1]["messages"].as_array().unwrap().is_empty());
    assert_eq!(page1_json["next_page"], true);

    let next_command = page1_json["next_command"].as_str().unwrap();
    let page2 = fixture.run_shell_command(next_command);
    assert!(
        page2.status.success(),
        "next_command={next_command}\nstderr={}",
        String::from_utf8_lossy(&page2.stderr)
    );
    let page2_json = parse_stdout_json(&page2);
    let page2_selected = page2_json["selected_sessions"].as_array().unwrap();
    assert_eq!(page2_selected.len(), 2);
    let total_page2_messages = page2_selected
        .iter()
        .map(|session| session["messages"].as_array().unwrap().len())
        .sum::<usize>();
    assert_eq!(total_page2_messages, 2);
    assert!(page2_selected[0]["messages"].as_array().unwrap().is_empty());
    assert_eq!(page2_selected[1]["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn retrieve_broad_scope_next_command_preserves_scope_flags() {
    let fixture = RetrieveContractFixture::seeded();
    let page1 = fixture.run_cli(&[
        "retrieve",
        "ranking tie marker",
        "--all-projects",
        "--all-sources",
        "--max-sessions",
        "2",
        "--max-messages-per-session",
        "4",
        "--limit",
        "2",
    ]);
    assert!(
        page1.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page1.stderr)
    );
    let page1_json = parse_stdout_json(&page1);
    assert_eq!(page1_json["next_page"], true);
    let next_command = page1_json["next_command"].as_str().unwrap();
    assert!(
        next_command.contains("--all-projects"),
        "next_command={next_command}"
    );
    assert!(
        next_command.contains("--all-sources"),
        "next_command={next_command}"
    );
    assert!(
        !next_command.contains("--project"),
        "next_command={next_command}"
    );

    let page2 = fixture.run_shell_command(next_command);
    assert!(
        page2.status.success(),
        "next_command={next_command}\nstderr={}",
        String::from_utf8_lossy(&page2.stderr)
    );
    let page2_json = parse_stdout_json(&page2);
    assert_eq!(page2_json["scope"]["all_projects"], true);
    assert_eq!(page2_json["scope"]["all_sources"], true);
}

#[test]
fn retrieve_pinned_next_command_executes_as_printed_and_freezes_sessions() {
    let fixture = RetrieveContractFixture::seeded();
    let page1 = fixture.run_cli(&[
        "retrieve",
        "next command marker",
        "--project",
        fixture.project_arg(),
        "--max-sessions",
        "1",
        "--max-messages-per-session",
        "6",
        "--limit",
        "2",
    ]);
    assert!(
        page1.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page1.stderr)
    );
    let page1_json = parse_stdout_json(&page1);
    assert_eq!(page1_json["next_page"], true);
    let next_command = page1_json["next_command"].as_str().expect("next_command");
    assert!(next_command.contains("--pinned-session"));
    assert!(next_command.contains(fixture.project_arg()));

    fixture.add_newer_matching_session("next command marker");
    let page2 = fixture.run_shell_command(next_command);
    assert!(
        page2.status.success(),
        "next_command={next_command}\nstderr={}",
        String::from_utf8_lossy(&page2.stderr)
    );
    let page2_json = parse_stdout_json(&page2);
    assert_eq!(page2_json["selected_sessions"].as_array().unwrap().len(), 1);
    assert_eq!(
        page2_json["selected_sessions"][0]["source_session_id"],
        "retrieve-codex-alpha"
    );
    assert!(
        !serde_json::to_string(&page2_json)
            .unwrap()
            .contains("retrieve-codex-newer"),
        "pinned continuation must not drift to newer matching sessions"
    );
}

#[test]
fn retrieve_pinned_session_validation_returns_structured_errors() {
    let fixture = RetrieveContractFixture::seeded();

    let malformed = fixture.run_cli(&[
        "retrieve",
        "public mapping marker",
        "--pinned-session",
        "{\"source\":\"codex\"}",
    ]);
    assert!(!malformed.status.success());
    assert!(
        !malformed.stdout.is_empty(),
        "retrieve errors should be structured JSON on stdout; stderr={}",
        String::from_utf8_lossy(&malformed.stderr)
    );
    let malformed_json = parse_stdout_json(&malformed);
    assert_eq!(malformed_json["error_kind"], "invalid_pinned_session");

    let extra_field = fixture.run_cli(&[
        "retrieve",
        "public mapping marker",
        "--pinned-session",
        r#"{"source":"codex","project_name":"/tmp/project","source_session_id":"retrieve-codex-alpha","session_id":"internal"}"#,
    ]);
    assert!(!extra_field.status.success());
    assert!(
        !extra_field.stdout.is_empty(),
        "retrieve errors should be structured JSON on stdout; stderr={}",
        String::from_utf8_lossy(&extra_field.stderr)
    );
    let extra_field_json = parse_stdout_json(&extra_field);
    assert_eq!(extra_field_json["error_kind"], "invalid_pinned_session");

    let stale = fixture.run_cli(&[
        "retrieve",
        "public mapping marker",
        "--pinned-session",
        r#"{"source":"codex","project_name":"/missing/project","source_session_id":"missing-session"}"#,
    ]);
    assert!(!stale.status.success());
    assert!(
        !stale.stdout.is_empty(),
        "retrieve errors should be structured JSON on stdout; stderr={}",
        String::from_utf8_lossy(&stale.stderr)
    );
    let stale_json = parse_stdout_json(&stale);
    assert_eq!(stale_json["error_kind"], "pinned_session_not_found");
}

// --- export ---

#[test]
fn export_with_project_returns_all_messages_for_project() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["read", "project", "--project", "Users/test/codex-proj"]);

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
        "read",
        "project",
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
        "read",
        "project",
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

    let output = fixture.run_cli_in_dir(&["read", "project"], &proj_dir);

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

    let output = fixture.run_cli_in_dir(&["--source", "grok", "read", "project"], &proj_dir);

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

    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-1","model":"test-model","choices":[{"message":{"role":"assistant","content":"continuity summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(
        stdout_json["backend"].as_str().unwrap(),
        "openai-compatible"
    );
    assert_eq!(stdout_json["model"].as_str().unwrap(), "test-model");
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
    assert!(system_message_text(&body).contains("Memory Agent"));
}

#[test]
fn summarize_project_without_selector_uses_project_history() {
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

    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-latest","model":"test-model","choices":[{"message":{"role":"assistant","content":"latest summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &["summarize", "project", "--project", "/Users/test/proj"],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
        input.contains("hello from claude"),
        "project summaries should include project history across sources"
    );
    assert!(input.matches("=== Session:").count() > 1);
}

#[test]
fn summarize_project_remote_includes_remote_messages() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-summary");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
cat > /dev/null
cat <<'JSON'
{"messages":[{"session_id":"remote-summary-session","source":"codex","project_name":"/Users/test/proj","role":"user","content":"remote summary evidence","model":"model","timestamp":"2025-01-08T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"total_messages":1,"next_page":false,"next_offset":1,"peer_results":[{"host":"local","transport":"local","command":"read/project","status":"ok","remote_mmr_version":"9.9.9","total_messages":1}]}
JSON
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-remote","model":"test-model","choices":[{"message":{"role":"assistant","content":"remote summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "--remote",
            "studio",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
            ("PATH", path.as_str()),
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
    assert!(input.contains("remote summary evidence"));
    assert!(input.contains("hello from claude"));
}

#[test]
fn summarize_fails_without_api_key_configuration() {
    let fixture = TestFixture::seeded();
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &["summarize", "project", "--project", "/Users/test/proj"],
        &[],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("summarize.apiKey"));
    assert!(stderr.contains("summarize.apiKeyEnv"));
    assert!(stderr.contains("OPENAI_API_KEY"));
}

#[test]
fn summarize_defaults_model_gpt_55_with_openai_api_key_only() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-default-model","model":"gpt-5.5","choices":[{"message":{"role":"assistant","content":"default model summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["model"].as_str().unwrap(), "gpt-5.5");
    assert_eq!(
        stdout_json["text"].as_str().unwrap(),
        "default model summary"
    );
}

#[test]
fn summarize_api_key_env_takes_precedence_over_openai_api_key() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-env-precedence","model":"gpt-5.5","choices":[{"message":{"role":"assistant","content":"env precedence summary"}}]}"#,
    );
    mmr::config::write_summarize_config_for_tests_with_api(
        &fixture.home,
        base_url.as_str(),
        "gpt-5.5",
        None,
        Some("MMR_TEST_API_KEY"),
    )
    .expect("write summarize config");
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ],
        &[
            ("MMR_TEST_API_KEY", "preferred-key"),
            ("OPENAI_API_KEY", "ignored-key"),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(
        stdout_json["text"].as_str().unwrap(),
        "env precedence summary"
    );
}

#[test]
fn status_summary_runner_configured_with_api_key_env() {
    let fixture = TestFixture::seeded();
    mmr::config::write_summarize_config_for_tests_with_api(
        &fixture.home,
        "https://api.openai.com/v1",
        "gpt-5.5",
        None,
        Some("MMR_TEST_API_KEY"),
    )
    .expect("write summarize config");
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &["status", "--pretty"],
        &[("MMR_TEST_API_KEY", "test-key")],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status_json = parse_stdout_json(&output);
    assert_eq!(
        status_json["diagnostics"]["summary_runner"]["status"],
        "configured"
    );
    assert_eq!(
        status_json["diagnostics"]["summary_runner"]["model"]
            .as_str()
            .unwrap(),
        "gpt-5.5"
    );
    assert!(
        status_json["diagnostics"]["summary_runner"]["config_file"]
            .as_str()
            .is_some_and(|path| path.contains("config.json"))
    );
}

#[test]
fn summarize_reads_api_key_env_from_config_file() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-config-env","model":"gpt-5.5","choices":[{"message":{"role":"assistant","content":"api key env summary"}}]}"#,
    );
    mmr::config::write_summarize_config_for_tests_with_api(
        &fixture.home,
        base_url.as_str(),
        "gpt-5.5",
        None,
        Some("MMR_TEST_API_KEY"),
    )
    .expect("write summarize config");
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ],
        &[("MMR_TEST_API_KEY", "test-key")],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["text"].as_str().unwrap(), "api key env summary");
}

#[test]
fn summarize_reads_api_credentials_from_config_file() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-config","model":"gpt-5.5","choices":[{"message":{"role":"assistant","content":"config summary"}}]}"#,
    );
    mmr::config::write_summarize_config_for_tests_with_api(
        &fixture.home,
        base_url.as_str(),
        "gpt-5.5",
        Some("test-key"),
        None,
    )
    .expect("write summarize config");
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ],
        &[],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["model"].as_str().unwrap(), "gpt-5.5");
    assert_eq!(stdout_json["text"].as_str().unwrap(), "config summary");
}

#[test]
fn summarize_uses_model_from_env() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-env-default","model":"env-model","choices":[{"message":{"role":"assistant","content":"model env default"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "json",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
            ("MMR_SUMMARISER_MODEL", "env-model"),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["model"].as_str().unwrap(), "env-model");
    assert_eq!(stdout_json["text"].as_str().unwrap(), "model env default");
}

#[test]
fn summarize_model_flag_overrides_model_env() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-env-override","model":"flag-model","choices":[{"message":{"role":"assistant","content":"explicit override"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "--model",
            "flag-model",
            "-O",
            "json",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
            ("MMR_SUMMARISER_MODEL", "env-model"),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["model"].as_str().unwrap(), "flag-model");
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

    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-2","model":"test-model","choices":[{"message":{"role":"assistant","content":"codex-only summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "--source",
            "codex",
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-3","model":"test-model","choices":[{"message":{"role":"assistant","content":"session summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "session",
            "sess-claude-1",
            "--project",
            "/Users/test/proj",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
        body.get("previous_interaction_id").is_none(),
        "one-shot summarize requests should not resume previous interactions"
    );
    let system_instruction = system_message_text(&body);
    assert!(system_instruction.contains("Memory Agent"));
}

#[test]
fn remember_output_format_md_transforms_json_response_to_markdown() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-md","model":"test-model","choices":[{"message":{"role":"assistant","content":"Status\n- Item one"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "-O",
            "md",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
    let (base_url, _captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"  interaction-trim  ","model":"test-model","choices":[{"message":{"role":"assistant","content":"  status line\nnext line  "}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "--output-format",
            "md",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-custom","model":"test-model","choices":[{"message":{"role":"assistant","content":"custom output"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "--instructions",
            "Return only a single keyword.",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let system = system_message_text(&body);
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
    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-default","model":"test-model","choices":[{"message":{"role":"assistant","content":"default output"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &["summarize", "project", "--project", "/Users/test/proj"],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock server thread");

    let body = captured.lock().expect("captured body").clone().unwrap();
    let system = system_message_text(&body);
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
fn summarize_session_limit_pages_newest_first() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-page","model":"test-model","choices":[{"message":{"role":"assistant","content":"paged session summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "session",
            "sess-codex-2",
            "--project",
            "/Users/test/codex-proj",
            "--limit",
            "1",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
    assert!(input.contains("second long codex answer"));
    assert!(!input.contains("start longer codex thread"));
    assert!(!input.contains("follow-up question"));
}

#[test]
fn summarize_project_limit_excludes_older_messages() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_chat_completions_server(
        r#"{"id":"interaction-proj-page","model":"test-model","choices":[{"message":{"role":"assistant","content":"paged project summary"}}]}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "--limit",
            "1",
        ],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", base_url.as_str()),
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
    assert!(input.contains("hi from assistant"));
    assert!(!input.contains("hello from claude"));
}

#[test]
fn summarize_session_requires_session_id() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_raw(&["summarize", "session"]);

    assert!(
        !output.status.success(),
        "session selector without an ID should be rejected"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided")
            || stderr.contains("a value is required"),
        "stderr should explain that the session command requires an ID: {stderr}"
    );
}

#[test]
fn compact_project_calls_morph_native_endpoint_with_history() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_compact_server(
        r#"{"id":"cmpr-project","object":"compact","model":"morph-compactor","output":"compacted project transcript","messages":[{"role":"user","content":"compacted project transcript","compacted_line_ranges":[{"start":2,"end":4}],"kept_line_ranges":[]}],"usage":{"input_tokens":100,"output_tokens":40,"compression_ratio":0.4,"processing_time_ms":12}}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "compact",
            "project",
            "--project",
            "/Users/test/proj",
            "--query",
            "active task",
            "--compression-ratio",
            "0.4",
            "--preserve-recent",
            "3",
            "--no-markers",
        ],
        &[
            ("MORPHLLM_API_KEY", "test-key"),
            ("MORPHLLM_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock compact server thread");

    let stdout_json = parse_stdout_json(&output);
    assert_eq!(stdout_json["backend"].as_str().unwrap(), "morph-compact");
    assert_eq!(stdout_json["model"].as_str().unwrap(), "morph-compactor");
    assert_eq!(stdout_json["id"].as_str().unwrap(), "cmpr-project");
    assert_eq!(
        stdout_json["output"].as_str().unwrap(),
        "compacted project transcript"
    );
    assert_eq!(stdout_json["usage"]["input_tokens"].as_u64().unwrap(), 100);
    assert_eq!(
        stdout_json["messages"][0]["compacted_line_ranges"][0]["start"]
            .as_u64()
            .unwrap(),
        2
    );

    let body = captured.lock().expect("captured body").clone().unwrap();
    assert_eq!(body["query"].as_str().unwrap(), "active task");
    assert_eq!(body["compression_ratio"].as_f64().unwrap(), 0.4);
    assert_eq!(body["preserve_recent"].as_u64().unwrap(), 3);
    assert!(!body["include_markers"].as_bool().unwrap());
    assert_eq!(body["model"].as_str().unwrap(), "morph-compactor");
    let input = body["input"].as_str().expect("compact input text");
    assert!(input.contains("hello from claude"));
    assert!(input.contains("## Session"));
}

#[test]
fn compact_session_markdown_outputs_compacted_text_only() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let (base_url, captured, handle) = start_mock_compact_server(
        r#"{"id":"cmpr-session","object":"compact","model":"morph-compactor","output":"  compacted session transcript\nnext line  ","messages":[{"role":"user","content":"compacted session transcript\nnext line","compacted_line_ranges":[],"kept_line_ranges":[]}],"usage":{"input_tokens":50,"output_tokens":25,"compression_ratio":0.5,"processing_time_ms":8}}"#,
    );
    let output = run_cli_with_home_and_env(
        &fixture.home,
        &[
            "compact",
            "session",
            "sess-claude-1",
            "--project",
            "/Users/test/proj",
            "-O",
            "md",
        ],
        &[
            ("MORPHLLM_API_KEY", "test-key"),
            ("MORPHLLM_BASE_URL", base_url.as_str()),
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    handle.join().expect("mock compact server thread");

    let stdout = stdout_text(&output);
    assert_eq!(stdout.trim_end(), "compacted session transcript\nnext line");
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "markdown compact output should not be JSON"
    );
    let body = captured.lock().expect("captured body").clone().unwrap();
    let input = body["input"].as_str().expect("compact input text");
    assert!(input.contains("hello from claude"));
}

#[test]
fn compact_source_requires_explicit_source() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_raw(&["compact", "source"]);

    assert!(
        !output.status.success(),
        "compact source without --source should be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("`mmr compact source` requires --source"),
        "stderr should explain missing source: {stderr}"
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
fn read_project_pagination_includes_next_page_and_next_command() {
    let fixture = TestFixture::seeded();
    // codex-proj has 6 messages (sess-codex-1: 2 + sess-codex-2: 4)
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "read",
        "project",
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
    assert!(next_cmd.contains("read project"), "next_command={next_cmd}");
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
fn read_source_pagination_no_next_command_when_all_results_fit() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "read", "source", "--limit", "100"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert!(!json["next_page"].as_bool().unwrap());
    assert!(json["next_command"].is_null());
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
        &["read", "session", "sess-claude-1"],
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

    let output = fixture.run_cli(&["read", "session", "sess-claude-1"]);
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

    let output = fixture.run_cli(&["--source", "claude", "read", "session", "sess-claude-1"]);
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
        "read",
        "session",
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
        "read",
        "session",
        "sess-claude-1",
        "--project",
        "/Users/test/codex-proj",
    ]);
    assert!(empty_output.status.success());
    let empty_json = parse_stdout_json(&empty_output);
    assert_eq!(empty_json["total_messages"].as_i64().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// previous stable session recall
// ---------------------------------------------------------------------------

fn assert_recall_failure(output: &Output, expected_error_kind: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["command"], "recall");
    assert_eq!(
        json["error_kind"],
        expected_error_kind,
        "stdout={}",
        stdout_text(output)
    );
    assert!(json["message"].is_string());
}

#[test]
fn recall_returns_previous_session_in_cwd_project() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);

    let output =
        fixture.run_cli_in_dir_with_env(&["recall"], &cwd, &[("MMR_AUTO_DISCOVER_PROJECT", "1")]);
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
fn recall_one_reports_age_one() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "recall",
        "--project",
        "/Users/test/codex-proj",
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
        "mmr read session sess-codex-2"
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
fn recall_zero_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "recall",
        "--project",
        "/Users/test/codex-proj",
        "0",
    ]);
    assert_recall_failure(&output, "age_zero_not_selectable");
}

#[test]
fn recall_zero_with_include_newest_returns_newest() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "recall",
        "--project",
        "/Users/test/codex-proj",
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
fn recall_out_of_range_names_counts() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "recall", "--all", "5"]);
    assert_recall_failure(&output, "session_back_out_of_range");
    let json = parse_stdout_json(&output);
    assert_eq!(json["total_sessions_in_scope"].as_i64().unwrap(), 3);
    assert_eq!(json["max_selectable_age"].as_u64().unwrap(), 2);
    assert_eq!(json["requested_age"].as_u64().unwrap(), 5);
}

#[test]
fn read_source_omits_session_selection_field() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["--source", "codex", "read", "source", "--limit", "5"]);
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
fn read_project_strawman_from_index_flags_are_rejected_by_clap() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["read", "project", "--from-index", "-1", "--to-index", "-1"]);
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
        "recall",
        "--project",
        "/Users/test/codex-proj",
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
        next_cmd.contains("read session sess-codex-2"),
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
        "recall",
        "--project",
        "/Users/test/codex-proj",
        "1",
        "--limit",
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
fn peer_status_host_uses_explicit_ssh_target() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-status");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    let ssh_log = fixture.home.join("peer-status-ssh.log");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$MMR_FAKE_SSH_LOG"
cat <<'JSON'
{"command":"peer/status","status":"ok","protocol_version":1,"mmr_version":"9.9.9","capabilities":["read-project"],"sources":["codex"]}
JSON
"#,
    );

    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &["peer", "status", "--host", "mish@studio:2222"],
        &[
            ("PATH", path.as_str()),
            ("MMR_FAKE_SSH_LOG", ssh_log.to_str().unwrap()),
        ],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "peer/status");
    assert_eq!(json["mmr_version"], "9.9.9");
    let log = fs::read_to_string(&ssh_log).expect("ssh log");
    assert!(log.contains("-p 2222"));
    assert!(log.contains("mish@studio"));
    assert!(log.contains("mmr peer status --json"));
}

#[test]
fn read_project_remote_merges_remote_messages_with_origin() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-read");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    let ssh_log = fixture.home.join("peer-read-ssh.log");
    let request_log = fixture.home.join("peer-read-request.json");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$MMR_FAKE_SSH_LOG"
cat > "$MMR_FAKE_REQUEST_LOG"
cat <<'JSON'
{"messages":[{"session_id":"remote-session","source":"codex","project_name":"/Users/test/codex-proj","role":"user","content":"remote studio context","model":"model","timestamp":"2025-01-08T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"total_messages":1,"next_page":false,"next_offset":1,"peer_results":[{"host":"local","transport":"local","command":"read/project","status":"ok","remote_mmr_version":"9.9.9","total_messages":1}]}
JSON
"#,
    );

    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "read",
            "project",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
        ],
        &[
            ("PATH", path.as_str()),
            ("MMR_FAKE_SSH_LOG", ssh_log.to_str().unwrap()),
            ("MMR_FAKE_REQUEST_LOG", request_log.to_str().unwrap()),
        ],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    let messages = json["messages"].as_array().expect("messages");
    let remote = messages
        .iter()
        .find(|message| message["content"] == "remote studio context")
        .expect("remote message");
    assert_eq!(remote["origin"]["host"], "studio");
    assert_eq!(remote["origin"]["transport"], "ssh");
    assert_eq!(remote["origin"]["remote_mmr_version"], "9.9.9");
    assert_eq!(json["peer_results"][0]["host"], "studio");
    assert_eq!(json["peer_results"][0]["remote_mmr_version"], "9.9.9");
    let request = fs::read_to_string(&request_log).expect("request log");
    assert!(request.contains("\"protocol_version\":1"));
    assert!(request.contains("\"source\":\"codex\""));
    assert!(
        fs::read_to_string(&ssh_log)
            .expect("ssh log")
            .contains("mmr peer read-project --request-json -")
    );
}

#[test]
fn read_project_remote_ssh_failure_is_structured() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-fail");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
echo "Permission denied (publickey)." >&2
exit 255
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &["read", "project", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_ssh_failed");
    assert_eq!(json["host"], "studio");
}

#[test]
fn read_project_remote_rejects_option_like_target() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["read", "project", "--remote=-oProxyCommand=sh"]);
    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_target_invalid");
    assert_eq!(json["host"], "-oProxyCommand=sh");
}

#[test]
fn read_project_remote_remote_mmr_missing_is_structured() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-missing");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
echo "mmr: command not found" >&2
exit 127
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &["read", "project", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_mmr_unavailable");
    assert_eq!(json["host"], "studio");
}

#[test]
fn read_project_remote_remote_incompatible_is_structured() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-incompatible");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
echo "unsupported peer protocol version 0; expected 1" >&2
exit 1
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &["read", "project", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_mmr_unavailable");
    assert_eq!(json["host"], "studio");
}

#[test]
fn read_project_remote_remote_peer_subcommand_missing_is_structured() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-subcommand-missing");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
echo "error: unrecognized subcommand 'peer'" >&2
echo "" >&2
echo "Usage: mmr [OPTIONS] <COMMAND>" >&2
exit 2
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &["read", "project", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_mmr_unavailable");
    assert_eq!(json["host"], "studio");
}

#[test]
fn read_project_without_remote_omits_peer_results() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "read",
        "project",
        "--project",
        "/Users/test/codex-proj",
    ]);
    assert!(output.status.success());
    let json = parse_stdout_json(&output);
    assert!(json.get("peer_results").is_none());
    assert!(
        json["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .all(|message| message.get("origin").is_none())
    );
}

#[test]
fn context_project_remote_merges_remote_context() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-context");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
cat > /dev/null
cat <<'JSON'
{"command":"context/project","scope":"project","source":"codex","project":"/Users/test/codex-proj","total_sessions":1,"total_messages":1,"sessions":[{"session_id":"remote-session","source":"codex","project_name":"/Users/test/codex-proj","project_path":"/Users/test/codex-proj","first_timestamp":"2025-01-08T00:00:00","last_timestamp":"2025-01-08T00:00:00","message_count":1,"user_messages":1,"assistant_messages":0,"preview":"remote"}],"messages":[{"session_id":"remote-session","source":"codex","project_name":"/Users/test/codex-proj","role":"user","content":"remote context message","model":"model","timestamp":"2025-01-08T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"peer_results":[{"host":"local","transport":"local","command":"context/project","status":"ok","remote_mmr_version":"9.9.9","total_messages":1,"total_sessions":1}]}
JSON
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "context",
            "project",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
        ],
        &[("PATH", path.as_str())],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "context/project");
    assert_eq!(json["peer_results"][0]["host"], "studio");
    let remote = json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["content"] == "remote context message")
        .expect("remote context message");
    assert_eq!(remote["origin"]["host"], "studio");
    assert_eq!(remote["origin"]["remote_mmr_version"], "9.9.9");
}

#[test]
fn recall_remote_merges_remote_recall_messages() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-recall");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
cat > /dev/null
cat <<'JSON'
{"messages":[{"session_id":"remote-previous","source":"codex","project_name":"/Users/test/codex-proj","role":"user","content":"remote previous session","model":"model","timestamp":"2025-01-02T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"total_messages":1,"next_page":false,"next_offset":1,"session_selection":{"scope":{"project":"/Users/test/codex-proj","all":false,"source":"codex"},"axis":"session-back","total_sessions_in_scope":2,"selected":[{"age":1,"session_id":"remote-previous","source":"codex","project_name":"/Users/test/codex-proj","first_timestamp":"2025-01-02T00:00:00","last_timestamp":"2025-01-02T00:00:00","message_count":1,"equivalent_command":"mmr read session remote-previous"}],"skipped_newest":{"age":0,"session_id":"remote-current","last_timestamp":"2025-01-03T00:00:00","assumed_live":true}},"peer_results":[{"host":"local","transport":"local","command":"recall","status":"ok","remote_mmr_version":"9.9.9","total_messages":1}]}
JSON
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "recall",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
            "1",
        ],
        &[("PATH", path.as_str())],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["peer_results"][0]["host"], "studio");
    let remote = json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["content"] == "remote previous session")
        .expect("remote recall message");
    assert_eq!(remote["origin"]["host"], "studio");
    assert_eq!(remote["origin"]["remote_mmr_version"], "9.9.9");
}

#[test]
fn remote_list_read_and_context_surfaces_use_remote_flag() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-surfaces");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
cat > /dev/null
case "$*" in
  *"mmr peer list-projects --request-json -"*)
    cat <<'JSON'
{"projects":[{"name":"/Users/test/remote-proj","source":"codex","original_path":"/Users/test/remote-proj","aliases":["remote-proj"],"session_count":1,"message_count":1,"last_activity":"2025-01-08T00:00:00"}],"total_messages":1,"total_sessions":1,"peer_results":[{"host":"local","transport":"local","command":"list/projects","status":"ok","remote_mmr_version":"9.9.9","total_messages":1,"total_sessions":1}]}
JSON
    ;;
  *"mmr peer list-sessions --request-json -"*)
    cat <<'JSON'
{"sessions":[{"session_id":"remote-session","source":"codex","project_name":"/Users/test/codex-proj","project_path":"/Users/test/codex-proj","first_timestamp":"2025-01-08T00:00:00","last_timestamp":"2025-01-08T00:00:00","message_count":1,"user_messages":1,"assistant_messages":0,"preview":"remote"}],"total_sessions":1,"peer_results":[{"host":"local","transport":"local","command":"list/sessions","status":"ok","remote_mmr_version":"9.9.9","total_sessions":1}]}
JSON
    ;;
  *"mmr peer read-session --request-json -"*)
    cat <<'JSON'
{"messages":[{"session_id":"remote-session","source":"codex","project_name":"/Users/test/codex-proj","role":"user","content":"remote explicit session","model":"model","timestamp":"2025-01-08T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"total_messages":1,"next_page":false,"next_offset":1,"peer_results":[{"host":"local","transport":"local","command":"read/session","status":"ok","remote_mmr_version":"9.9.9","total_messages":1}]}
JSON
    ;;
  *"mmr peer read-source --request-json -"*)
    cat <<'JSON'
{"messages":[{"session_id":"remote-source-session","source":"codex","project_name":"/Users/test/remote-proj","role":"user","content":"remote source message","model":"model","timestamp":"2025-01-08T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"total_messages":1,"next_page":false,"next_offset":1,"peer_results":[{"host":"local","transport":"local","command":"read/source","status":"ok","remote_mmr_version":"9.9.9","total_messages":1}]}
JSON
    ;;
  *"mmr peer context-source --request-json -"*)
    cat <<'JSON'
{"command":"context/source","scope":"source","source":"codex","project":null,"total_sessions":1,"total_messages":1,"sessions":[{"session_id":"remote-source-session","source":"codex","project_name":"/Users/test/remote-proj","project_path":"/Users/test/remote-proj","first_timestamp":"2025-01-08T00:00:00","last_timestamp":"2025-01-08T00:00:00","message_count":1,"user_messages":1,"assistant_messages":0,"preview":"remote"}],"messages":[{"session_id":"remote-source-session","source":"codex","project_name":"/Users/test/remote-proj","role":"user","content":"remote context source","model":"model","timestamp":"2025-01-08T00:00:00","is_subagent":false,"msg_type":"user","input_tokens":0,"output_tokens":0}],"peer_results":[{"host":"local","transport":"local","command":"context/source","status":"ok","remote_mmr_version":"9.9.9","total_messages":1,"total_sessions":1}]}
JSON
    ;;
  *)
    echo "unexpected peer command: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());

    let projects = fixture.run_cli_with_env(
        &["list", "projects", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert!(projects.status.success());
    let projects_json = parse_stdout_json(&projects);
    let remote_project = projects_json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["name"] == "/Users/test/remote-proj")
        .expect("remote project");
    assert_eq!(remote_project["origin"]["host"], "studio");

    let sessions = fixture.run_cli_with_env(
        &[
            "list",
            "sessions",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
        ],
        &[("PATH", path.as_str())],
    );
    assert!(sessions.status.success());
    let sessions_json = parse_stdout_json(&sessions);
    let remote_session = sessions_json["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["session_id"] == "remote-session")
        .expect("remote session");
    assert_eq!(remote_session["origin"]["remote_mmr_version"], "9.9.9");

    let read_session = fixture.run_cli_with_env(
        &["read", "session", "remote-session", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert!(read_session.status.success());
    let read_session_json = parse_stdout_json(&read_session);
    assert!(
        read_session_json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["origin"]["host"] == "studio")
    );

    let read_source = fixture.run_cli_with_env(
        &["--source", "codex", "read", "source", "--remote", "studio"],
        &[("PATH", path.as_str())],
    );
    assert!(read_source.status.success());
    let read_source_json = parse_stdout_json(&read_source);
    assert!(
        read_source_json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == "remote source message"
                && message["origin"]["host"] == "studio")
    );

    let context_source = fixture.run_cli_with_env(
        &[
            "--source", "codex", "context", "source", "--remote", "studio",
        ],
        &[("PATH", path.as_str())],
    );
    assert!(context_source.status.success());
    let context_json = parse_stdout_json(&context_source);
    assert_eq!(context_json["peer_results"][0]["host"], "studio");
    assert!(
        context_json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == "remote context source"
                && message["origin"]["remote_mmr_version"] == "9.9.9")
    );
}

fn assert_cli_failure(output: &Output, expected_exit: i32, expected_command: Option<&str>) {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Some(command) = expected_command {
        let json = parse_stdout_json(output);
        assert_eq!(json["status"], "failed");
        assert_eq!(json["command"], command);
        assert!(json["message"].is_string());
    }
}

fn share_codex_bundle_to_file(fixture: &TestFixture) -> (serde_json::Value, PathBuf) {
    let inbox = fixture.home.join("session-share-inbox");
    let to = format!("file://{}", inbox.display());
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "share",
        "session",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--to",
        &to,
    ]);
    assert!(
        output.status.success(),
        "share stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "share/session");
    assert_eq!(json["transport"], "file");
    let bundle_path =
        PathBuf::from(json["inbox_path"].as_str().expect("inbox_path")).join("bundle.mmr");
    assert!(bundle_path.is_file(), "bundle file should exist");
    (json, bundle_path)
}

#[test]
fn peer_status_host_uses_hidden_host_flag() {
    let fixture = TestFixture::seeded();
    let fake_bin = fixture.home.join("fake-bin-peer-status-hidden");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
cat <<'JSON'
{"command":"peer/status","status":"ok","protocol_version":1,"mmr_version":"9.9.9","capabilities":["read-project"],"sources":["codex"]}
JSON
"#,
    );

    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &["peer", "status", "--host", "studio"],
        &[("PATH", path.as_str())],
    );
    assert!(output.status.success());
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "peer/status");
    assert_eq!(json["mmr_version"], "9.9.9");
}

#[test]
fn public_host_flag_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_raw(&["read", "project", "--host", "studio"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--host'"));
}

#[test]
fn teleport_namespace_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_raw(&["teleport", "pull"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand 'teleport'"));
}

#[test]
fn old_top_level_import_events_is_rejected() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_raw(&[
        "import",
        "--source",
        "codex",
        "--project",
        "/Users/test/codex-proj",
    ]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn ingest_events_imports_source_history() {
    let fixture = TestFixture::seeded();
    let project = fixture.home.join("ingest-proj");
    fs::create_dir_all(&project).expect("create ingest project");
    write_file(
        &fixture
            .home
            .join(".codex")
            .join("sessions")
            .join("ingest.jsonl"),
        &format!(
            r#"{{"type":"session_meta","timestamp":"2025-01-10T00:00:00","payload":{{"id":"ingest-session","cwd":"{}","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-10T00:00:00"}}}}
{{"type":"event_msg","timestamp":"2025-01-10T00:00:01","payload":{{"type":"user_message","message":"ingest me"}}}}
{{"type":"response_item","timestamp":"2025-01-10T00:01:00","payload":{{"role":"assistant","content":[{{"type":"output_text","text":"ingested"}}]}}}}"#,
            project.display()
        ),
    );
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "ingest",
        "events",
        "--project",
        project.to_str().expect("project path"),
        "--source-root",
        fixture.home.join(".codex").to_str().expect("source root"),
    ]);
    assert!(
        output.status.success(),
        "ingest stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["source"], "codex");
    assert!(json["discovered_sessions"].as_u64().unwrap() >= 1);
}

#[test]
fn share_session_file_then_import_bundle_read_only_and_apply_round_trip() {
    let fixture = TestFixture::seeded();
    let (_share_json, bundle_path) = share_codex_bundle_to_file(&fixture);

    let read_output = fixture.run_cli(&[
        "import",
        "bundle",
        "--read-only",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        read_output.status.success(),
        "read-only stderr={}",
        String::from_utf8_lossy(&read_output.stderr)
    );
    let read_json = parse_stdout_json(&read_output);
    assert_eq!(read_json["command"], "import/bundle");
    assert_eq!(read_json["message_count"], 2);

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove seeded native session before apply");
    let apply_output = fixture.run_cli(&[
        "import",
        "bundle",
        "--apply",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        apply_output.status.success(),
        "apply stderr={}",
        String::from_utf8_lossy(&apply_output.stderr)
    );
    let apply_json = parse_stdout_json(&apply_output);
    assert_eq!(apply_json["command"], "import/bundle");
    assert_eq!(apply_json["apply"]["command"], "import/bundle/apply");
    assert_eq!(apply_json["apply"]["native"]["written"], true);
    assert!(native_path.is_file(), "native codex session should exist");
}

#[test]
fn import_bundle_rejects_read_only_and_apply_together() {
    let fixture = TestFixture::seeded();
    let (_share_json, bundle_path) = share_codex_bundle_to_file(&fixture);
    let output = fixture.run_cli(&[
        "import",
        "bundle",
        "--read-only",
        "--apply",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert_cli_failure(&output, 2, Some("import/bundle"));
}

#[test]
fn import_bundle_missing_locator_fails_with_json() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["import", "bundle", "--apply"]);
    assert_cli_failure(&output, 2, Some("import/bundle"));
}

#[test]
fn import_bundle_markdown_uses_import_wording() {
    let fixture = TestFixture::seeded();
    let (_share_json, bundle_path) = share_codex_bundle_to_file(&fixture);
    let output = fixture.run_cli(&[
        "import",
        "bundle",
        "--read-only",
        "-O",
        "md",
        bundle_path.to_str().expect("bundle path"),
    ]);
    assert!(
        output.status.success(),
        "markdown import stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    let text = json["text"].as_str().expect("markdown text");
    assert!(text.starts_with("# Import bundle"));
    assert!(!text.contains("Teleport read"));
}

#[test]
fn share_session_ssh_dry_run_reports_import_bundle_plan() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "--source",
        "codex",
        "share",
        "session",
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
        "share dry-run stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "share/session");
    assert_eq!(json["transport"], "ssh");
    assert_eq!(json["to"], "bob@macbook");
    let planned = json["planned_commands"]
        .as_object()
        .expect("planned_commands");
    assert!(
        planned["stream_apply"]
            .as_array()
            .expect("stream_apply argv")
            .iter()
            .any(|arg| arg.as_str() == Some("mmr import bundle --to - --apply")),
        "dry-run should include import bundle stream command"
    );
}

#[test]
fn share_session_auto_ignores_legacy_teleport_transport_env() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "share",
            "session",
            "--session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--to",
            "bob@macbook",
            "--dry-run",
        ],
        &[("MMR_TELEPORT_TRANSPORT", "file")],
    );
    assert!(
        output.status.success(),
        "share should ignore legacy transport env stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "share/session");
    assert_eq!(json["transport"], "ssh");
}

#[test]
fn share_session_http_starts_one_shot_locator() {
    if !loopback_bind_available() {
        return;
    }
    let fixture = TestFixture::seeded();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "--source",
            "grok",
            "share",
            "session",
            "--via",
            "http",
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
        .expect("spawn share session");

    let stdout = child.stdout.take().expect("share stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read share startup json");
    let startup: serde_json::Value =
        serde_json::from_str(line.trim()).expect("parse share startup json");
    assert_eq!(startup["command"], "share/session");
    assert_eq!(startup["transport"], "http");
    assert_eq!(startup["session"]["source"], "grok");
    let _ = child.kill();
    let _ = child.wait();
}

fn peer_pack_response_from_bundle(fixture: &TestFixture, bundle_path: &Path, response_path: &Path) {
    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bundle_path).expect("bundle JSON"))
            .expect("parse bundle");
    write_file(
        response_path,
        &serde_json::json!({
            "command": "peer/teleport-pack",
            "status": "ok",
            "bundle_id": bundle_json["manifest"]["bundle_id"],
            "bundle": bundle_json,
            "remote_mmr_version": "9.9.9"
        })
        .to_string(),
    );
    let _ = fixture;
}

#[test]
fn import_session_from_remote_read_only_reads_without_applying() {
    let fixture = TestFixture::seeded();
    let (_share_json, bundle_path) = share_codex_bundle_to_file(&fixture);
    let response_path = fixture.home.join("peer-import-read-only-response.json");
    peer_pack_response_from_bundle(&fixture, &bundle_path, &response_path);

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove native before import");

    let fake_bin = fixture.home.join("fake-bin-peer-import-read-only");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
cat > /dev/null
cat "$MMR_FAKE_PEER_RESPONSE"
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "import",
            "session",
            "--from",
            "studio",
            "--session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--read-only",
        ],
        &[
            ("PATH", path.as_str()),
            ("MMR_FAKE_PEER_RESPONSE", response_path.to_str().unwrap()),
        ],
    );
    assert!(
        output.status.success(),
        "import read-only stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "import/session");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["read"]["command"], "import/session/read");
    assert_eq!(json["read"]["message_count"], 2);
    assert!(!native_path.exists());
}

#[test]
fn import_session_from_remote_applies() {
    let fixture = TestFixture::seeded();
    let (_share_json, bundle_path) = share_codex_bundle_to_file(&fixture);
    let response_path = fixture.home.join("peer-import-apply-response.json");
    peer_pack_response_from_bundle(&fixture, &bundle_path, &response_path);

    let native_path = fixture
        .home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    fs::remove_file(&native_path).expect("remove native before import");

    let fake_bin = fixture.home.join("fake-bin-peer-import-apply");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    let ssh_log = fixture.home.join("peer-import-ssh.log");
    let request_log = fixture.home.join("peer-import-request.json");
    write_executable(
        &fake_bin.join("ssh"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$MMR_FAKE_SSH_LOG"
cat > "$MMR_FAKE_REQUEST_LOG"
cat "$MMR_FAKE_PEER_RESPONSE"
"#,
    );
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{original_path}", fake_bin.display());
    let output = fixture.run_cli_with_env(
        &[
            "--source",
            "codex",
            "import",
            "session",
            "--from",
            "studio",
            "--session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--apply",
        ],
        &[
            ("PATH", path.as_str()),
            ("MMR_FAKE_SSH_LOG", ssh_log.to_str().unwrap()),
            ("MMR_FAKE_REQUEST_LOG", request_log.to_str().unwrap()),
            ("MMR_FAKE_PEER_RESPONSE", response_path.to_str().unwrap()),
        ],
    );
    assert!(
        output.status.success(),
        "import apply stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "import/session");
    assert_eq!(json["apply"]["command"], "import/session/apply");
    assert!(
        native_path.is_file(),
        "import should apply native transcript"
    );
    assert!(
        fs::read_to_string(&ssh_log)
            .expect("ssh log")
            .contains("mmr peer teleport-pack --request-json -")
    );
    let request = fs::read_to_string(&request_log).expect("request");
    assert!(request.contains("\"session_id\":\"sess-codex-1\""));
    assert!(request.contains("\"source\":\"codex\""));
}

#[test]
fn provider_matrix_share_file_then_import_read_only() {
    let fixture = TestFixture::seeded();
    for (source, session, project) in [
        ("codex", "sess-codex-1", "/Users/test/codex-proj"),
        ("claude", "sess-claude-1", "-Users-test-proj"),
        ("cursor", "sess-cursor-1", "-Users-test-cursor-proj"),
        ("grok", "sess-grok-1", "/Users/test/grok-proj"),
        ("pi", "sess-pi-1", "/Users/test/pi-proj"),
    ] {
        let inbox = fixture.home.join(format!("provider-share-{source}"));
        let to = format!("file://{}", inbox.display());
        let project_arg = format!("--project={project}");
        let share_args = [
            "--source",
            source,
            "share",
            "session",
            "--session",
            session,
            project_arg.as_str(),
            "--to",
            to.as_str(),
        ];
        let share_output = fixture.run_cli(&share_args);
        assert!(
            share_output.status.success(),
            "{source} share stderr={}",
            String::from_utf8_lossy(&share_output.stderr)
        );
        let share_json = parse_stdout_json(&share_output);
        let bundle_path = PathBuf::from(share_json["inbox_path"].as_str().expect("inbox path"))
            .join("bundle.mmr");
        let read_output = fixture.run_cli(&[
            "import",
            "bundle",
            "--read-only",
            bundle_path.to_str().expect("bundle path"),
        ]);
        assert!(
            read_output.status.success(),
            "{source} import read-only stderr={}",
            String::from_utf8_lossy(&read_output.stderr)
        );
        let read_json = parse_stdout_json(&read_output);
        assert_eq!(read_json["command"], "import/bundle");
        assert_eq!(read_json["session"]["source"], source);
        assert!(read_json["message_count"].as_u64().unwrap() >= 1);
    }
}

#[test]
fn teleport_provider_profile_dispatch_unknown_provider() {
    match mmr::teleport::profile_for("unknown-provider") {
        Err(err) => assert!(err.message.contains("unsupported teleport provider")),
        Ok(_) => panic!("unknown provider should be rejected"),
    }
}
