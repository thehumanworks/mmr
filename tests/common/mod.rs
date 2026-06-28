use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

use mmr::store::{NewEvent, NewLearnedMemory, Store};
use serde_json::json;

pub struct TestFixture {
    _tmp: tempfile::TempDir,
    pub home: PathBuf,
}

impl TestFixture {
    pub fn seeded() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).expect("create HOME");

        seed_claude_fixture(&home);
        seed_codex_fixture(&home);
        seed_cursor_fixture(&home);
        seed_grok_fixture(&home);
        seed_pi_fixture(&home);

        Self { _tmp: tmp, home }
    }

    pub fn run_cli(&self, args: &[&str]) -> Output {
        self.run_cli_command(args, &[])
    }

    pub fn run_cli_raw(&self, args: &[&str]) -> Output {
        self.run_cli_command(args, &[])
    }

    pub fn run_cli_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.run_cli_command(args, env)
    }

    pub fn run_cli_in_dir(&self, args: &[&str], cwd: &Path) -> Output {
        self.run_cli_command_in_dir(args, cwd, &[])
    }

    pub fn run_cli_in_dir_with_env(
        &self,
        args: &[&str],
        cwd: &Path,
        env: &[(&str, &str)],
    ) -> Output {
        self.run_cli_command_in_dir(args, cwd, env)
    }

    fn run_cli_command(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let cwd = self.home.join("cwd");
        fs::create_dir_all(&cwd).expect("cwd");
        self.run_cli_command_in_dir(args, &cwd, env)
    }

    fn run_cli_command_in_dir(&self, args: &[&str], cwd: &Path, env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(args)
            .env("HOME", &self.home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("MMR_CONFIG_FILE")
            .current_dir(cwd);
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().expect("run mmr")
    }
}

pub fn parse_stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout JSON")
}

pub struct RetrieveContractFixture {
    _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub data_home: PathBuf,
    pub project: PathBuf,
    pub other_project: PathBuf,
    pub provider_only_project: PathBuf,
}

impl RetrieveContractFixture {
    pub fn seeded() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        let data_home = root.join("data");
        let project = root.join("retrieve project with spaces [docs]");
        let other_project = root.join("retrieve other project [system]");
        let provider_only_project = root.join("retrieve provider only project [harness]");
        fs::create_dir_all(&home).expect("create HOME");
        fs::create_dir_all(&project).expect("create project");
        fs::create_dir_all(&other_project).expect("create other project");
        fs::create_dir_all(&provider_only_project).expect("create provider-only project");
        let project = fs::canonicalize(&project).expect("canonical project");
        let other_project = fs::canonicalize(&other_project).expect("canonical other project");
        let provider_only_project =
            fs::canonicalize(&provider_only_project).expect("canonical provider-only project");
        let project_name = project.to_str().expect("project path UTF-8").to_string();
        let other_project_name = other_project
            .to_str()
            .expect("other project path UTF-8")
            .to_string();
        let provider_only_project_name = provider_only_project
            .to_str()
            .expect("provider-only project path UTF-8")
            .to_string();

        link_retrieve_project(&home, &data_home, &project, "retrieve project");
        link_retrieve_project(&home, &data_home, &other_project, "retrieve other project");

        seed_retrieve_provider_history(&home, &project_name);
        seed_retrieve_store(&data_home, &project);
        seed_retrieve_other_provider_history(&home, &other_project_name);
        seed_retrieve_other_store(&data_home, &other_project);
        seed_retrieve_provider_only_history(&home, &provider_only_project_name);

        Self {
            _tmp: tmp,
            home,
            data_home,
            project,
            other_project,
            provider_only_project,
        }
    }

    pub fn project_arg(&self) -> &str {
        self.project.to_str().expect("project path UTF-8")
    }

    pub fn other_project_arg(&self) -> &str {
        self.other_project
            .to_str()
            .expect("other project path UTF-8")
    }

    pub fn provider_only_project_arg(&self) -> &str {
        self.provider_only_project
            .to_str()
            .expect("provider-only project path UTF-8")
    }

    pub fn run_cli(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("MMR_CONFIG_FILE")
            .current_dir(&self.project);
        command.output().expect("run mmr")
    }

    pub fn run_cli_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("MMR_CONFIG_FILE")
            .current_dir(&self.project);
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().expect("run mmr")
    }

    pub fn run_shell_command(&self, command_line: &str) -> Output {
        let binary_dir = Path::new(env!("CARGO_BIN_EXE_mmr"))
            .parent()
            .expect("mmr binary parent");
        let existing_path = std::env::var_os("PATH").unwrap_or_default();
        let path = format!(
            "{}:{}",
            binary_dir.display(),
            existing_path.to_string_lossy()
        );
        Command::new("zsh")
            .arg("-lc")
            .arg(command_line)
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("MMR_CONFIG_FILE")
            .current_dir(&self.project)
            .output()
            .expect("run retrieve next_command")
    }

    pub fn add_newer_matching_session(&self, query: &str) {
        let project_name = self.project_arg().to_string();
        write_codex_session(
            &self.home,
            &project_name,
            "retrieve-codex-newer",
            &[
                (
                    "event_msg",
                    "user",
                    "2026-06-28T09:00:00Z",
                    format!("{query} newer user"),
                ),
                (
                    "response_item",
                    "assistant",
                    "2026-06-28T09:00:05Z",
                    format!("{query} newer assistant"),
                ),
            ],
        );

        let mut store = Store::open(self.data_home.join("mmr").join("mmr.db")).expect("store");
        let project = store
            .project_by_path(&self.project)
            .expect("project lookup")
            .expect("project");
        insert_search_event(
            &mut store,
            &project.id,
            "codex",
            "retrieve-codex-newer",
            "newer-match",
            "message",
            "assistant",
            "2026-06-28T09:00:05Z",
            &format!("{query} newer matching session should not alter pinned page"),
        );
    }
}

fn link_retrieve_project(home: &Path, data_home: &Path, project: &Path, label: &str) {
    let link = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "__db-info",
            "--project",
            project.to_str().expect("project path UTF-8"),
        ])
        .env("HOME", home)
        .env("XDG_DATA_HOME", data_home)
        .output()
        .unwrap_or_else(|err| panic!("link {label}: {err}"));
    assert!(
        link.status.success(),
        "{label} link stderr={}",
        String::from_utf8_lossy(&link.stderr)
    );
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
}

fn write_jsonl(path: &Path, rows: &[serde_json::Value]) {
    let mut contents = rows
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    write_file(path, &contents);
}

fn seed_retrieve_provider_history(home: &Path, project_name: &str) {
    write_codex_session(
        home,
        project_name,
        "retrieve-codex-alpha",
        &[
            (
                "event_msg",
                "user",
                "2026-06-28T07:59:00Z",
                "retrieve fixture smoke public mapping prelude".to_string(),
            ),
            (
                "response_item",
                "assistant",
                "2026-06-28T07:59:30Z",
                "public mapping marker citation marker ranking tie marker".to_string(),
            ),
            (
                "event_msg",
                "user",
                "2026-06-28T08:00:00Z",
                "window marker anchor one ranking tie marker".to_string(),
            ),
            (
                "response_item",
                "assistant",
                "2026-06-28T08:00:30Z",
                "window marker anchor two next command marker".to_string(),
            ),
            (
                "event_msg",
                "user",
                "2026-06-28T08:01:00Z",
                "public mapping marker follow-up".to_string(),
            ),
            (
                "response_item",
                "assistant",
                "2026-06-28T08:01:30Z",
                "next command marker final response".to_string(),
            ),
        ],
    );
    write_codex_session(
        home,
        project_name,
        "retrieve-codex-beta",
        &[
            (
                "event_msg",
                "user",
                "2026-06-28T07:58:00Z",
                "ranking tie marker beta user".to_string(),
            ),
            (
                "response_item",
                "assistant",
                "2026-06-28T08:00:00Z",
                "ranking tie marker beta assistant".to_string(),
            ),
        ],
    );
    write_claude_session(
        home,
        project_name,
        "retrieve-claude-alpha",
        &[
            (
                "user",
                "2026-06-28T07:58:00Z",
                "ranking tie marker claude user",
            ),
            (
                "assistant",
                "2026-06-28T08:00:00Z",
                "ranking tie marker claude assistant",
            ),
        ],
    );
}

fn seed_retrieve_other_provider_history(home: &Path, project_name: &str) {
    write_codex_session(
        home,
        project_name,
        "retrieve-codex-system",
        &[
            (
                "event_msg",
                "user",
                "2026-06-28T06:59:00Z",
                "system wide marker other project prelude".to_string(),
            ),
            (
                "response_item",
                "assistant",
                "2026-06-28T06:59:30Z",
                "system wide marker other project answer".to_string(),
            ),
            (
                "event_msg",
                "user",
                "2026-06-28T07:00:00Z",
                "system wide marker follow-up".to_string(),
            ),
        ],
    );
}

fn seed_retrieve_provider_only_history(home: &Path, project_name: &str) {
    write_codex_session(
        home,
        project_name,
        "retrieve-codex-provider-only",
        &[
            (
                "event_msg",
                "user",
                "2026-06-28T06:45:00Z",
                "provider only marker harness transcript was never linked".to_string(),
            ),
            (
                "response_item",
                "assistant",
                "2026-06-28T06:45:30Z",
                "provider only marker answer from raw harness history".to_string(),
            ),
        ],
    );
}

fn seed_retrieve_store(data_home: &Path, project: &Path) {
    let mut store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project = store
        .project_by_path(project)
        .expect("project lookup")
        .expect("project");

    let public_mapping = insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-alpha",
        "public-mapping-1",
        "message",
        "assistant",
        "2026-06-28T07:59:30Z",
        "retrieve fixture smoke public mapping marker citation marker ranking tie marker",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-alpha",
        "public-mapping-2",
        "message",
        "user",
        "2026-06-28T08:01:00Z",
        "public mapping marker follow-up",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-alpha",
        "window-1",
        "message",
        "user",
        "2026-06-28T08:00:00Z",
        "window marker anchor one ranking tie marker",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-alpha",
        "window-2",
        "message",
        "assistant",
        "2026-06-28T08:00:30Z",
        "window marker anchor two next command marker",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-alpha",
        "next-command-1",
        "message",
        "assistant",
        "2026-06-28T08:01:30Z",
        "next command marker final response",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-beta",
        "ranking-beta-1",
        "message",
        "user",
        "2026-06-28T07:58:00Z",
        "ranking tie marker beta user",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-beta",
        "ranking-beta-2",
        "message",
        "assistant",
        "2026-06-28T08:00:00Z",
        "ranking tie marker beta assistant",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "claude",
        "retrieve-claude-alpha",
        "ranking-claude-1",
        "message",
        "user",
        "2026-06-28T07:58:00Z",
        "ranking tie marker claude user",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "claude",
        "retrieve-claude-alpha",
        "ranking-claude-2",
        "message",
        "assistant",
        "2026-06-28T08:00:00Z",
        "ranking tie marker claude assistant",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-missing-provider",
        "unreadable-db-only",
        "message",
        "assistant",
        "2026-06-28T08:02:00Z",
        "unreadable marker db-only provider transcript is absent",
    );

    let evidence_ref = format!("mmr://event/{}", public_mapping.id);
    let run = store
        .start_dream_run(&project.id, "mock", "mock", "sha256:retrieve")
        .expect("start retrieve dream run");
    store
        .complete_dream_run(
            &run.id,
            "sha256:retrieve-output",
            &[],
            &[NewLearnedMemory {
                kind: "retrieval".to_string(),
                claim: "unreadable marker learned memory only".to_string(),
                confidence: 0.9,
                evidence_refs: vec![evidence_ref],
                counterevidence_refs: Vec::new(),
                status: "active".to_string(),
            }],
        )
        .expect("complete retrieve dream run");
}

fn seed_retrieve_other_store(data_home: &Path, project: &Path) {
    let mut store = Store::open(data_home.join("mmr").join("mmr.db")).expect("store");
    let project = store
        .project_by_path(project)
        .expect("other project lookup")
        .expect("other project");

    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-system",
        "system-wide-1",
        "message",
        "user",
        "2026-06-28T06:59:00Z",
        "system wide marker other project prelude",
    );
    insert_search_event(
        &mut store,
        &project.id,
        "codex",
        "retrieve-codex-system",
        "system-wide-2",
        "message",
        "assistant",
        "2026-06-28T06:59:30Z",
        "system wide marker other project answer",
    );
}

fn write_codex_session(
    home: &Path,
    project_name: &str,
    session_id: &str,
    messages: &[(&str, &str, &str, String)],
) {
    let mut rows = vec![json!({
        "type": "session_meta",
        "timestamp": "2026-06-28T07:55:00Z",
        "payload": {
            "id": session_id,
            "cwd": project_name,
            "cli_version": "1.0.0",
            "model_provider": "openai",
            "timestamp": "2026-06-28T07:55:00Z",
            "git": {"branch": "main"}
        }
    })];
    for (kind, role, timestamp, text) in messages {
        if *kind == "event_msg" {
            rows.push(json!({
                "type": "event_msg",
                "timestamp": timestamp,
                "payload": {
                    "type": "user_message",
                    "message": text
                }
            }));
        } else {
            rows.push(json!({
                "type": "response_item",
                "timestamp": timestamp,
                "payload": {
                    "role": role,
                    "content": [{"type": "output_text", "text": text}]
                }
            }));
        }
    }

    write_jsonl(
        &home
            .join(".codex")
            .join("sessions")
            .join(format!("{session_id}.jsonl")),
        &rows,
    );
}

fn write_claude_session(
    home: &Path,
    project_name: &str,
    session_id: &str,
    messages: &[(&str, &str, &str)],
) {
    let project_dir = if project_name == "/" {
        "-".to_string()
    } else {
        format!(
            "-{}",
            project_name.trim_start_matches('/').replace('/', "-")
        )
    };
    let rows = messages
        .iter()
        .enumerate()
        .map(|(idx, (role, timestamp, content))| {
            json!({
                "type": role,
                "sessionId": session_id,
                "message": {
                    "role": role,
                    "content": content,
                    "model": "claude-3-opus",
                    "usage": {"input_tokens": 10, "output_tokens": 5}
                },
                "timestamp": timestamp,
                "uuid": format!("{session_id}-{idx}"),
                "cwd": project_name
            })
        })
        .collect::<Vec<_>>();
    write_jsonl(
        &home
            .join(".claude")
            .join("projects")
            .join(project_dir)
            .join(format!("{session_id}.jsonl")),
        &rows,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_search_event(
    store: &mut Store,
    project_id: &str,
    source: &str,
    source_session_id: &str,
    source_event_id: &str,
    event_type: &str,
    role: &str,
    timestamp: &str,
    content: &str,
) -> mmr::store::EventRecord {
    let event = NewEvent::new(
        source,
        source_session_id,
        event_type,
        role,
        timestamp,
        content,
        "retrieve-contract-v1",
    )
    .with_source_event_id(source_event_id)
    .with_raw_local_ref(format!(
        "tests/fixtures/retrieve/{source_session_id}.jsonl:1"
    ));
    let (event, _) = store
        .insert_event_with_search_document(project_id, &event)
        .expect("insert retrieve event");
    event
}

fn seed_claude_fixture(home: &Path) {
    let claude_session = home
        .join(".claude")
        .join("projects")
        .join("-Users-test-proj")
        .join("sess-claude-1.jsonl");

    write_file(
        &claude_session,
        r#"{"type":"user","sessionId":"sess-claude-1","message":{"role":"user","content":"hello from claude"},"timestamp":"2025-01-01T00:00:00","uuid":"u1","cwd":"/Users/test/proj"}
{"type":"assistant","sessionId":"sess-claude-1","message":{"role":"assistant","content":"hi from assistant","model":"claude-3-opus","usage":{"input_tokens":100,"output_tokens":50}},"timestamp":"2025-01-01T00:01:00","uuid":"a1","parentUuid":"u1","cwd":"/Users/test/proj"}"#,
    );
}

fn seed_cursor_fixture(home: &Path) {
    let cursor_session = home
        .join(".cursor")
        .join("projects")
        .join("-Users-test-cursor-proj")
        .join("agent-transcripts")
        .join("sess-cursor-1")
        .join("sess-cursor-1.jsonl");

    write_file(
        &cursor_session,
        r#"{"role":"user","message":{"content":[{"type":"text","text":"hello from cursor"}]}}
{"role":"assistant","message":{"content":[{"type":"text","text":"hi from cursor assistant"}]}}"#,
    );
}

fn seed_grok_fixture(home: &Path) {
    let grok_session = home
        .join(".grok")
        .join("sessions")
        .join("%2FUsers%2Ftest%2Fgrok-proj")
        .join("sess-grok-1");

    write_file(
        &grok_session.join("summary.json"),
        r#"{"info":{"id":"sess-grok-1","cwd":"/Users/test/grok-proj"},"session_summary":"Grok fixture session","created_at":"2025-01-05T00:00:00Z","updated_at":"2025-01-05T00:00:03Z","current_model_id":"grok-build","num_messages":3}"#,
    );
    write_file(
        &grok_session.join("updates.jsonl"),
        r#"{"timestamp":1736035200,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}
not-json
{"timestamp":1736035201,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035201000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi "}},"_meta":{"agentTimestampMs":1736035202000}}}
{"timestamp":1736035202,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from grok assistant"}},"_meta":{"agentTimestampMs":1736035202100}}}
{"timestamp":1736035203,"method":"session/update","params":{"sessionId":"sess-grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"follow-up from grok"},"_meta":{"modelId":"grok-build"}},"_meta":{"agentTimestampMs":1736035203000}}}"#,
    );
}

fn seed_pi_fixture(home: &Path) {
    let pi_session = home
        .join(".pi")
        .join("agent")
        .join("sessions")
        .join("--Users-test-pi-proj--")
        .join("2025-01-04T00-00-00-000Z_sess-pi-1.jsonl");

    write_file(
        &pi_session,
        r#"{"type":"session","version":3,"id":"sess-pi-1","timestamp":"2025-01-04T00:00:00.000Z","cwd":"/Users/test/pi-proj"}
{"type":"model_change","id":"model-1","parentId":null,"timestamp":"2025-01-04T00:00:00.100Z","provider":"openai-codex","modelId":"gpt-5.5"}
{"type":"message","id":"msg-pi-u1","parentId":"model-1","timestamp":"2025-01-04T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"hello from pi"}],"timestamp":1735948801000}}
{"type":"message","id":"msg-pi-a1","parentId":"msg-pi-u1","timestamp":"2025-01-04T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"internal"},{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"Cargo.toml"}},{"type":"text","text":"hi from pi assistant"}],"model":"gpt-5.5","usage":{"input":12,"output":6},"timestamp":1735948802000}}
{"type":"message","id":"msg-pi-t1","parentId":"msg-pi-a1","timestamp":"2025-01-04T00:00:03.000Z","message":{"role":"toolResult","toolCallId":"call-1","toolName":"read","content":[{"type":"text","text":"tool output should not be a chat message"}],"isError":false,"timestamp":1735948803000}}"#,
    );
}

fn seed_codex_fixture(home: &Path) {
    let codex_session_1 = home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    write_file(
        &codex_session_1,
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"hello from codex"}}
{"type":"response_item","timestamp":"2025-01-02T00:05:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"short codex answer"}]}}"#,
    );

    let codex_session_2 = home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-2.jsonl");
    write_file(
        &codex_session_2,
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-2","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:30","payload":{"type":"user_message","message":"start longer codex thread"}}
{"type":"response_item","timestamp":"2025-01-02T00:01:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"first long codex answer"}]}}
{"type":"event_msg","timestamp":"2025-01-02T00:02:00","payload":{"type":"user_message","message":"follow-up question"}}
{"type":"response_item","timestamp":"2025-01-02T00:03:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"second long codex answer"}]}}"#,
    );

    let codex_session_3 = home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-recent-1.jsonl");
    write_file(
        &codex_session_3,
        r#"{"type":"session_meta","timestamp":"2025-01-03T00:00:00","payload":{"id":"sess-codex-recent-1","cwd":"/Users/test/codex-recent","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-03T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-03T00:00:01","payload":{"type":"user_message","message":"brand new project question"}}
{"type":"response_item","timestamp":"2025-01-03T00:01:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"recent project answer"}]}}"#,
    );
}
