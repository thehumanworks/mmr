use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

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

/// A second machine for peer tests. An `ssh` script on `PATH` drops the
/// ssh options and host, then runs this build's `mmr` with `HOME` set to
/// `home`, so both the client and the `mmr peer …` server code run.
pub struct PeerLoopback {
    pub home: PathBuf,
    bin_dir: PathBuf,
    ssh_log: PathBuf,
}

impl PeerLoopback {
    /// Remote history for `/Users/test/codex-proj`, `/Users/test/proj` and a
    /// remote-only project. `sess-codex-1` is a byte-for-byte copy of the local
    /// fixture session, so merged reads must drop its duplicate messages.
    pub fn seeded(local: &TestFixture) -> Self {
        let root = local.home.parent().expect("fixture root").join("remote");
        let home = root.join("home");
        let bin_dir = root.join("bin");
        let ssh_log = root.join("ssh.log");
        fs::create_dir_all(home.join("cwd")).expect("remote cwd");
        fs::create_dir_all(&bin_dir).expect("remote bin dir");

        let sessions = home.join(".codex").join("sessions");
        fs::create_dir_all(&sessions).expect("remote codex sessions");
        fs::copy(
            local.home.join(".codex/sessions/sess-codex-1.jsonl"),
            sessions.join("sess-codex-1.jsonl"),
        )
        .expect("copy shared session");
        write_file(
            &sessions.join("sess-remote-a.jsonl"),
            r#"{"type":"session_meta","timestamp":"2025-01-02T00:04:00","payload":{"id":"sess-remote-a","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:04:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:04:00","payload":{"type":"user_message","message":"remote codex question"}}
{"type":"response_item","timestamp":"2025-01-02T00:04:30","payload":{"role":"assistant","content":[{"type":"output_text","text":"remote codex answer"}]}}"#,
        );
        write_file(
            &sessions.join("sess-remote-b.jsonl"),
            r#"{"type":"session_meta","timestamp":"2025-01-07T00:00:00","payload":{"id":"sess-remote-b","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-07T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-07T00:00:01","payload":{"type":"user_message","message":"newest remote question"}}
{"type":"response_item","timestamp":"2025-01-07T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"newest remote answer"}]}}"#,
        );
        write_file(
            &sessions.join("sess-remote-only.jsonl"),
            r#"{"type":"session_meta","timestamp":"2025-01-06T00:00:00","payload":{"id":"sess-remote-only","cwd":"/Users/test/remote-only","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-06T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-06T00:00:01","payload":{"type":"user_message","message":"remote only question"}}"#,
        );
        write_file(
            &home
                .join(".claude")
                .join("projects")
                .join("-Users-test-proj")
                .join("sess-claude-remote.jsonl"),
            r#"{"type":"user","sessionId":"sess-claude-remote","message":{"role":"user","content":"remote claude question"},"timestamp":"2025-01-01T00:00:30","uuid":"ru1","cwd":"/Users/test/proj"}"#,
        );

        let script = format!(
            "#!/bin/sh\n\
             while [ \"$#\" -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
             shift\n\
             printf '%s\\n' \"$*\" >> '{log}'\n\
             shift\n\
             shift\n\
             cd '{cwd}' || exit 1\n\
             HOME='{home}' exec '{bin}' \"$@\"\n",
            log = ssh_log.display(),
            cwd = home.join("cwd").display(),
            home = home.display(),
            bin = env!("CARGO_BIN_EXE_mmr"),
        );
        let ssh = bin_dir.join("ssh");
        fs::write(&ssh, script).expect("write fake ssh");
        let mut permissions = fs::metadata(&ssh).expect("ssh metadata").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&ssh, permissions).expect("chmod fake ssh");

        Self {
            home,
            bin_dir,
            ssh_log,
        }
    }

    /// `PATH` value that resolves `ssh` to the loopback script.
    pub fn path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// Remote commands the client sent, one `host mmr peer …` line per call.
    pub fn ssh_calls(&self) -> Vec<String> {
        fs::read_to_string(&self.ssh_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

/// Render each item of a JSON array as `field|field|…` using JSON pointers, so
/// tests can pin order and content in one readable assertion. Missing or null
/// values render as `-`.
pub fn rows(items: &serde_json::Value, pointers: &[&str]) -> Vec<String> {
    items
        .as_array()
        .expect("JSON array")
        .iter()
        .map(|item| {
            pointers
                .iter()
                .map(|pointer| match item.pointer(pointer) {
                    None | Some(serde_json::Value::Null) => "-".to_string(),
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(other) => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

/// One project whose path has spaces and brackets, so every printed
/// `next_command` and `read_command` must shell-quote it to run as printed.
pub struct SpacedProjectFixture {
    _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub data_home: PathBuf,
    pub project: PathBuf,
}

impl SpacedProjectFixture {
    pub fn seeded() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        let data_home = root.join("data");
        let project = root.join("retrieve project with spaces [docs]");
        fs::create_dir_all(&home).expect("create HOME");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(&project).expect("canonical project");
        let project_name = project.to_str().expect("project path UTF-8").to_string();
        seed_retrieve_provider_history(&home, &project_name);

        Self {
            _tmp: tmp,
            home,
            data_home,
            project,
        }
    }

    pub fn project_arg(&self) -> &str {
        self.project.to_str().expect("project path UTF-8")
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
            .expect("run printed command")
    }
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
