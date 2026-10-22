#[allow(dead_code)]
mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStderr, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
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
        stdio_text.contains("without a REST server or listener"),
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

#[test]
fn mcp_stdio_ignores_rest_settings_and_occupied_ports() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    seed_find_fixture(&fixture)?;
    // Keep the old default port occupied where available. An existing listener
    // already satisfies that condition; never connect to it or inspect it.
    let _default_port = match TcpListener::bind("127.0.0.1:8765") {
        Ok(listener) => Some(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => None,
        Err(error) => return Err(error.into()),
    };
    let occupied = TcpListener::bind("127.0.0.1:0")?;
    occupied.set_nonblocking(true)?;
    let address = occupied.local_addr()?.to_string();
    let url = format!("http://{address}");
    for bind in [address.as_str(), "not-a-socket-address"] {
        let (mut mcp, mut stderr) = StdioMcp::spawn_with_env(
            &fixture,
            &[
                ("MMR_MCP_REST_BACKING_BIND", bind),
                ("MMR_MCP_REST_BACKING_BASE_URL", &url),
            ],
        )?;
        assert_stdio_query_contract(&fixture, &mut mcp)?;
        mcp.finish()?;
        let mut diagnostics = String::new();
        stderr.read_to_string(&mut diagnostics)?;
        assert!(diagnostics.is_empty(), "{diagnostics}");
        assert_eq!(
            occupied.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "stdio must not probe the configured REST endpoint"
        );
    }
    Ok(())
}

#[test]
fn mcp_stdio_does_not_bind_a_rest_listener() -> anyhow::Result<()> {
    let fixture = TestFixture::seeded();
    seed_find_fixture(&fixture)?;
    let reserved = TcpListener::bind("127.0.0.1:0")?;
    let address = reserved.local_addr()?;
    drop(reserved);
    let (mut mcp, _) = StdioMcp::spawn_with_env(
        &fixture,
        &[
            ("MMR_MCP_REST_BACKING_BIND", &address.to_string()),
            ("MMR_MCP_REST_BACKING_BASE_URL", "not-a-url"),
        ],
    )?;
    // A live stdio process must leave even a valid configured port available.
    let _still_available = TcpListener::bind(address)?;
    assert_stdio_query_contract(&fixture, &mut mcp)?;
    mcp.finish()?;
    Ok(())
}

fn seed_find_fixture(fixture: &TestFixture) -> anyhow::Result<()> {
    let project = fixture.home.join("cwd");
    fs::create_dir_all(&project)?;
    let rows = [
        json!({"type":"session_meta", "timestamp":"2026-10-22T00:00:00Z",
            "payload":{"id":"mcp-fixture", "cwd":fs::canonicalize(&project)?}}),
        json!({"type":"event_msg", "timestamp":"2026-10-22T00:00:01Z",
            "payload":{"type":"user_message", "message":"socketmarker fixture"}}),
    ];
    fs::write(
        fixture.home.join(".codex/sessions/mcp-fixture.jsonl"),
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let init = fixture.run_cli(&["init"]);
    assert!(init.status.success(), "{}", stderr_text(&init));
    Ok(())
}

fn assert_stdio_query_contract(fixture: &TestFixture, mcp: &mut StdioMcp) -> anyhow::Result<()> {
    let response = mcp.send_request(
        json!({"jsonrpc":"2.0", "id":2,
        "method":"tools/list", "params":{}}),
        2,
    )?;
    let tools = response["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), EXPECTED_TOOLS.len());
    for expected in EXPECTED_TOOLS {
        assert!(tools.iter().any(|tool| tool["name"] == *expected));
    }
    for (id, tool, arguments, cli_args) in [
        (
            3,
            "mmr_read_session",
            json!({"source":"claude", "session_id":"sess-claude-1"}),
            vec!["--source", "claude", "read", "session", "sess-claude-1"],
        ),
        (
            4,
            "mmr_find",
            json!({"source":"codex", "query":"socketmarker"}),
            vec!["--source", "codex", "find", "socketmarker"],
        ),
    ] {
        let cli = fixture.run_cli(&cli_args);
        assert!(cli.status.success(), "{}", stderr_text(&cli));
        let response = mcp.call_tool(id, tool, arguments)?;
        let actual: Value = serde_json::from_str(&tool_text(&response))?;
        assert_eq!(actual, parse_stdout_json(&cli));
        if tool == "mmr_find" {
            assert!(!actual["results"].as_array().unwrap().is_empty());
        }
    }
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
    seed_find_fixture(&fixture)?;
    let mut child = HttpChild(
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args([
                "mcp",
                "--transport",
                "http",
                "--bind",
                "127.0.0.1:0",
                "--path",
                "/custom/mcp",
            ])
            .env("HOME", &fixture.home)
            .env_remove("MMR_CONFIG_FILE")
            .env_remove("XDG_CONFIG_HOME")
            .current_dir(fixture.home.join("cwd"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stderr = child.0.stderr.take().expect("stderr pipe");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut startup = String::new();
        let result = BufReader::new(stderr)
            .read_line(&mut startup)
            .map(|_| startup);
        let _ = sender.send(result);
    });
    let startup = receiver.recv_timeout(Duration::from_secs(5))??;
    assert!(
        startup.contains("http://") && startup.contains("/mcp"),
        "startup stderr: {startup}"
    );
    let url = startup
        .split_whitespace()
        .find(|part| part.starts_with("http://"))
        .expect("startup url")
        .to_string();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
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

    for (id, method, params) in [
        (2, "tools/list", json!({})),
        (
            3,
            "tools/call",
            json!({"name":"mmr_read_session", "arguments":{
            "source":"claude", "session_id":"sess-claude-1"}}),
        ),
        (
            4,
            "tools/call",
            json!({"name":"mmr_find", "arguments":{
            "source":"codex", "query":"socketmarker"}}),
        ),
    ] {
        let response = client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .send()
            .await?
            .error_for_status()?;
        let parsed = parse_streamable_http_body(&response.text().await?)?;
        assert!(parsed.get("error").is_none(), "{parsed}");
        if id == 2 {
            assert_eq!(
                parsed["result"]["tools"].as_array().unwrap().len(),
                EXPECTED_TOOLS.len()
            );
        } else {
            let args = if id == 3 {
                vec!["--source", "claude", "read", "session", "sess-claude-1"]
            } else {
                vec!["--source", "codex", "find", "socketmarker"]
            };
            let cli = fixture.run_cli(&args);
            assert!(cli.status.success(), "{}", stderr_text(&cli));
            let actual: Value = serde_json::from_str(&tool_text(&parsed))?;
            assert_eq!(actual, parse_stdout_json(&cli));
        }
    }

    Ok(())
}

struct HttpChild(Child);

impl Drop for HttpChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct StdioMcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<anyhow::Result<Value>>,
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
        fs::create_dir_all(fixture.home.join("cwd"))?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(["mcp", "--transport", "stdio"])
            .env("HOME", &fixture.home)
            .env_remove("MMR_MCP_REST_BACKING_BIND")
            .env_remove("MMR_MCP_REST_BACKING_BASE_URL")
            .env_remove("MMR_CONFIG_FILE")
            .env_remove("XDG_CONFIG_HOME")
            .current_dir(fixture.home.join("cwd"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().expect("stdin pipe");
        let stdout = BufReader::new(child.stdout.take().expect("stdout pipe"));
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in stdout.lines() {
                let parsed = line.map_err(anyhow::Error::from).and_then(|line| {
                    serde_json::from_str(&line)
                        .map_err(|error| anyhow::anyhow!("non-JSON stdout line: {line:?}: {error}"))
                });
                if sender.send(parsed).is_err() {
                    break;
                }
            }
        });
        let stderr = BufReader::new(child.stderr.take().expect("stderr pipe"));
        let mut mcp = Self {
            child,
            stdin: Some(stdin),
            stdout: receiver,
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
        writeln!(
            self.stdin.as_mut().expect("open stdin"),
            "{}",
            serde_json::to_string(&request)?
        )?;
        self.stdin.as_mut().expect("open stdin").flush()?;
        self.read_response(expected_id)
    }

    fn send_notification(&mut self, notification: Value) -> anyhow::Result<()> {
        writeln!(
            self.stdin.as_mut().expect("open stdin"),
            "{}",
            serde_json::to_string(&notification)?
        )?;
        self.stdin.as_mut().expect("open stdin").flush()?;
        Ok(())
    }

    fn read_response(&mut self, expected_id: i64) -> anyhow::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() > deadline {
                anyhow::bail!("timed out waiting for JSON-RPC response id {expected_id}");
            }
            let parsed = self
                .stdout
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
            if parsed["id"] == expected_id {
                return Ok(parsed);
            }
        }
    }
    fn finish(mut self) -> anyhow::Result<()> {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                assert!(status.success(), "stdio exit: {status}");
                break;
            }
            anyhow::ensure!(Instant::now() < deadline, "stdio did not exit after EOF");
            thread::sleep(Duration::from_millis(10));
        }
        // Drain through EOF so trailing diagnostics on stdout cannot escape QA.
        loop {
            match self.stdout.recv_timeout(Duration::from_secs(5)) {
                Ok(value) => {
                    value?;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
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

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
