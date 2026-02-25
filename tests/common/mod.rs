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

        Self { _tmp: tmp, home }
    }

    pub fn run_cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(args)
            .env("HOME", &self.home)
            .output()
            .expect("run mmr")
    }

    pub fn run_cli_in_dir(&self, args: &[&str], cwd: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(args)
            .env("HOME", &self.home)
            .current_dir(cwd)
            .output()
            .expect("run mmr")
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
