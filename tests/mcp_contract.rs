#[allow(dead_code)]
mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use common::{TestFixture, parse_stdout_json};
use rmcp::model::{GetPromptRequestParams, PromptMessageContent};
use rmcp::{ServiceExt, model::ClientInfo};
use serde_json::{Value, json};

const EXPECTED_TOOLS: &[&str] = &[
    "mmr_list_projects",
    "mmr_list_sessions",
    "mmr_read_session",
    "mmr_read_project",
    "mmr_read_source",
    "mmr_recall",
    "mmr_find",
    "mmr_context_project",
    "mmr_context_source",
    "mmr_assimilate_project",
    "mmr_assimilate_source",
    "mmr_summarize_project",
    "mmr_summarize_session",
    "mmr_summarize_source",
    "mmr_compact_project",
    "mmr_compact_session",
    "mmr_compact_source",
    "mmr_status",
    "mmr_skill_load",
];

const EXPECTED_PROMPTS: &[&str] = &[
    "mmr_recall_previous_session",
    "mmr_project_context_brief",
    "mmr_session_handoff",
    "mmr_memory_assimilation",
    "mmr_find_then_read",
];

#[test]
fn mcp_python_bootstrap_subcommand_is_removed() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();

    let help = fixture.run_cli(&["mcp", "--help"]);
    assert!(help.status.success(), "help stderr: {}", stderr_text(&help));
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(!help_text.contains("python-bootstrap"), "{help_text}");
    assert!(help_text.contains("--transport"), "{help_text}");

    let removed = fixture.run_cli(&["mcp", "python-bootstrap"]);
    assert!(
        !removed.status.success(),
        "removed subcommand should fail; stdout: {}",
        String::from_utf8_lossy(&removed.stdout)
    );
    assert!(
        stderr_text(&removed).contains("unexpected argument 'python-bootstrap'"),
        "{}",
        stderr_text(&removed)
    );

    let stdio_help = fixture.run_cli(&["mcp", "--transport", "stdio", "--help"]);
    assert!(
        stdio_help.status.success(),
        "stdio help stderr: {}",
        stderr_text(&stdio_help)
    );
    let stdio_text = String::from_utf8_lossy(&stdio_help.stdout);
    assert!(
        stdio_text.contains("local loopback REST backing endpoint"),
        "{stdio_text}"
    );
    Ok(())
}

#[tokio::test]
async fn mcp_server_initializes_with_tools_and_prompts() -> anyhow::Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        mmr::mcp::MmrMcpServer::new()
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        anyhow::Ok(())
    });

    let client = ClientInfo::default().serve(client_transport).await?;
    let info = client.peer_info().expect("server initialize info");
    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.prompts.is_some());

    client.cancel().await?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn mcp_lists_expected_tools() -> anyhow::Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        mmr::mcp::MmrMcpServer::new()
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        anyhow::Ok(())
    });

    let client = ClientInfo::default().serve(client_transport).await?;
    let result = client.list_tools(None).await?;
    let tool_names = result
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    for expected in EXPECTED_TOOLS {
        assert!(
            tool_names.contains(expected),
            "missing expected tool {expected}; got {tool_names:?}"
        );
    }

    client.cancel().await?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn mcp_lists_expected_prompts() -> anyhow::Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        mmr::mcp::MmrMcpServer::new()
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        anyhow::Ok(())
    });

    let client = ClientInfo::default().serve(client_transport).await?;
    let result = client.list_prompts(None).await?;
    let prompt_names = result
        .prompts
        .iter()
        .map(|prompt| prompt.name.as_ref())
        .collect::<Vec<_>>();
    for expected in EXPECTED_PROMPTS {
        assert!(
            prompt_names.contains(expected),
            "missing expected prompt {expected}; got {prompt_names:?}"
        );
    }

    client.cancel().await?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn mcp_prompt_accepts_string_numeric_args() -> anyhow::Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        mmr::mcp::MmrMcpServer::new()
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        anyhow::Ok(())
    });

    let client = ClientInfo::default().serve(client_transport).await?;
    let result = client
        .get_prompt(
            GetPromptRequestParams::new("mmr_recall_previous_session").with_arguments(
                json!({
                    "project": "/Users/test/codex-proj",
                    "source": "codex",
                    "n": "2",
                    "limit": "25"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let text = prompt_text(&result.messages[0].content);
    assert!(text.contains("n=2"), "{text}");
    assert!(text.contains("limit=25"), "{text}");

    client.cancel().await?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn mcp_list_projects_matches_cli_fixture() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let cli = fixture.run_cli(&["--source", "codex", "list", "projects", "--limit", "10"]);
    assert!(cli.status.success(), "cli stderr: {}", stderr_text(&cli));
    let cli_json = parse_stdout_json(&cli);

    let mut mcp = StdioMcp::spawn(&fixture)?;
    let response = mcp.call_tool(
        2,
        "mmr_list_projects",
        json!({
            "source": "codex",
            "limit": 10
        }),
    )?;
    let mcp_json: Value = serde_json::from_str(tool_text(&response).as_str())?;
    assert_eq!(mcp_json["projects"], cli_json["projects"]);
    assert_eq!(mcp_json["total_messages"], cli_json["total_messages"]);
    Ok(())
}

#[tokio::test]
async fn mcp_read_session_matches_cli_fixture() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let cli = fixture.run_cli(&["--source", "claude", "read", "session", "sess-claude-1"]);
    assert!(cli.status.success(), "cli stderr: {}", stderr_text(&cli));
    let cli_json = parse_stdout_json(&cli);

    let mut mcp = StdioMcp::spawn(&fixture)?;
    let response = mcp.call_tool(
        2,
        "mmr_read_session",
        json!({
            "source": "claude",
            "session_id": "sess-claude-1"
        }),
    )?;
    let mcp_json: Value = serde_json::from_str(tool_text(&response).as_str())?;
    assert_eq!(mcp_json["messages"], cli_json["messages"]);
    assert_eq!(mcp_json["total_messages"], cli_json["total_messages"]);
    Ok(())
}

#[tokio::test]
async fn mcp_read_source_requires_explicit_source() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let mut mcp = StdioMcp::spawn(&fixture)?;
    let response = mcp.send_request(
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "mmr_read_source",
                "arguments": {}
            }
        }),
        2,
    )?;
    assert_eq!(response["error"]["code"], -32602, "{response}");
    Ok(())
}

#[tokio::test]
async fn mcp_stdio_autostarts_rest_backing_when_missing() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let (mut mcp, mut stderr) =
        StdioMcp::spawn_with_env(&fixture, &[("MMR_MCP_REST_BACKING_BIND", "127.0.0.1:0")])?;
    let startup = read_stderr_line(&mut stderr)?;
    assert!(
        startup.contains("mmr REST backing server listening on http://127.0.0.1:"),
        "startup stderr: {startup}"
    );
    let api_base_url = first_http_url(&startup)?;

    let client = reqwest::Client::new();
    let openapi: Value = client
        .get(format!("{api_base_url}/openapi.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(openapi["openapi"], "3.1.0");

    let status: Value = client
        .get(format!("{api_base_url}/v1/status"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(status["command"], "status", "{status}");

    let response = mcp.send_request(
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
        2,
    )?;
    assert!(
        response["result"]["tools"].as_array().is_some(),
        "{response}"
    );
    Ok(())
}

#[tokio::test]
async fn mcp_stdio_reuses_existing_rest_backing() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let mock = start_mock_rest_status_server()?;
    let base_url = mock.base_url.clone();
    let (mut mcp, mut stderr) = StdioMcp::spawn_with_env(
        &fixture,
        &[
            ("MMR_MCP_REST_BACKING_BIND", "127.0.0.1:0"),
            ("MMR_MCP_REST_BACKING_BASE_URL", &base_url),
        ],
    )?;
    let startup = read_stderr_line(&mut stderr)?;
    assert!(
        startup.contains(&format!(
            "mmr REST backing server already available at {base_url}"
        )),
        "startup stderr: {startup}"
    );
    assert!(
        mock.requests.load(Ordering::SeqCst) >= 1,
        "mock status endpoint was not probed"
    );

    let response = mcp.send_request(
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
        2,
    )?;
    assert!(
        response["result"]["tools"].as_array().is_some(),
        "{response}"
    );
    Ok(())
}

#[tokio::test]
async fn mcp_stdio_subprocess_protocol_smoke() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let mut mcp = StdioMcp::spawn(&fixture)?;
    let response = mcp.send_request(
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
        2,
    )?;
    assert!(
        response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| { tool["name"] == "mmr_list_projects" })
    );
    Ok(())
}

#[tokio::test]
async fn mcp_http_streamable_smoke() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["mcp", "--transport", "http", "--bind", "127.0.0.1:0"])
        .env("HOME", &fixture.home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().expect("stderr pipe");
    let mut stderr = BufReader::new(stderr);
    let mut startup = String::new();
    stderr.read_line(&mut startup)?;
    assert!(
        startup.contains("http://") && startup.contains("/mcp"),
        "startup stderr: {startup}"
    );
    let url = startup
        .split_whitespace()
        .find(|part| part.starts_with("http://"))
        .expect("startup url")
        .to_string();

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "mmr-test", "version": "1.0"}
            }
        }))
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    let parsed = parse_streamable_http_body(&body)?;
    assert!(
        parsed["result"]["capabilities"]["tools"].is_object(),
        "{parsed}"
    );
    assert!(
        parsed["result"]["capabilities"]["prompts"].is_object(),
        "{parsed}"
    );

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

struct StdioMcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioMcp {
    fn spawn(fixture: &TestFixture) -> anyhow::Result<Self> {
        let (mcp, _stderr) = Self::spawn_with_env(fixture, &[])?;
        Ok(mcp)
    }

    fn spawn_with_env(
        fixture: &TestFixture,
        envs: &[(&str, &str)],
    ) -> anyhow::Result<(Self, BufReader<ChildStderr>)> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(["mcp", "--transport", "stdio"])
            .env("HOME", &fixture.home)
            .env("MMR_MCP_REST_BACKING_BIND", "127.0.0.1:0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().expect("stdin pipe");
        let stdout = BufReader::new(child.stdout.take().expect("stdout pipe"));
        let stderr = BufReader::new(child.stderr.take().expect("stderr pipe"));
        let mut mcp = Self {
            child,
            stdin,
            stdout,
        };
        mcp.initialize()?;
        Ok((mcp, stderr))
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        let init = self.send_request(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "mmr-test", "version": "1.0"}
                }
            }),
            1,
        )?;
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        assert!(
            init["result"]["capabilities"]["prompts"].is_object(),
            "{init}"
        );
        self.send_notification(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))?;
        Ok(())
    }

    fn call_tool(&mut self, id: i64, name: &str, arguments: Value) -> anyhow::Result<Value> {
        self.send_request(
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": name,
                    "arguments": arguments
                }
            }),
            id,
        )
    }

    fn send_request(&mut self, request: Value, expected_id: i64) -> anyhow::Result<Value> {
        writeln!(self.stdin, "{}", serde_json::to_string(&request)?)?;
        self.stdin.flush()?;
        self.read_response(expected_id)
    }

    fn send_notification(&mut self, notification: Value) -> anyhow::Result<()> {
        writeln!(self.stdin, "{}", serde_json::to_string(&notification)?)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_response(&mut self, expected_id: i64) -> anyhow::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() > deadline {
                anyhow::bail!("timed out waiting for JSON-RPC response id {expected_id}");
            }
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line)?;
            if read == 0 {
                anyhow::bail!("MCP subprocess closed stdout");
            }
            let parsed: Value = serde_json::from_str(line.trim_end())
                .map_err(|error| anyhow::anyhow!("non-JSON stdout line: {line:?}: {error}"))?;
            if parsed["id"] == expected_id {
                return Ok(parsed);
            }
        }
    }
}

impl Drop for StdioMcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_text(response: &Value) -> String {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text content")
        .to_string()
}

fn prompt_text(content: &PromptMessageContent) -> &str {
    match content {
        PromptMessageContent::Text { text } => text,
        other => panic!("expected prompt text content, got {other:?}"),
    }
}

fn parse_streamable_http_body(body: &str) -> anyhow::Result<Value> {
    if let Ok(value) = serde_json::from_str(body) {
        return Ok(value);
    }
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            return Ok(serde_json::from_str(data.trim())?);
        }
    }
    anyhow::bail!("streamable HTTP body did not contain JSON result: {body}");
}

fn read_stderr_line(stderr: &mut BufReader<ChildStderr>) -> anyhow::Result<String> {
    let mut line = String::new();
    stderr.read_line(&mut line)?;
    if line.trim().is_empty() {
        anyhow::bail!("stderr closed before startup line");
    }
    Ok(line)
}

fn first_http_url(line: &str) -> anyhow::Result<String> {
    line.split_whitespace()
        .find(|part| part.starts_with("http://"))
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("no http URL in line: {line}"))
}

struct MockRestServer {
    base_url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for MockRestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn start_mock_rest_status_server() -> anyhow::Result<MockRestServer> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let requests = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let thread_requests = Arc::clone(&requests);
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    thread_requests.fetch_add(1, Ordering::SeqCst);
                    let mut buffer = [0_u8; 1024];
                    let _ = stream.read(&mut buffer);
                    let request = String::from_utf8_lossy(&buffer);
                    let (status, body) = if request.starts_with("GET /v1/status ") {
                        ("HTTP/1.1 200 OK", r#"{"command":"status","store":{}}"#)
                    } else {
                        ("HTTP/1.1 404 Not Found", r#"{"error":"not found"}"#)
                    };
                    let response = format!(
                        "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    Ok(MockRestServer {
        base_url,
        requests,
        stop,
        handle: Some(handle),
    })
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
