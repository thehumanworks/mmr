use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

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

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
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
