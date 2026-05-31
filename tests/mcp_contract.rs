#[allow(dead_code)]
mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(["mcp", "--transport", "stdio"])
            .env("HOME", &fixture.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin pipe");
        let stdout = BufReader::new(child.stdout.take().expect("stdout pipe"));
        let mut mcp = Self {
            child,
            stdin,
            stdout,
        };
        let init = mcp.send_request(
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
        mcp.send_notification(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))?;
        Ok(mcp)
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

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
