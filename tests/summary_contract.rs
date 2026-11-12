//! Contract for `summarize`: help surface, request shape sent to the
//! provider, API-key configuration, and the gated live-provider smoke test.

use std::path::Path;
use std::process::Command;

fn loopback_bind_available() -> bool {
    std::net::TcpListener::bind("127.0.0.1:0").is_ok()
}

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
