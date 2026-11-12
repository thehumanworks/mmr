//! One project, five harnesses: the project's history is every session run in
//! its directory or below it, whichever agent ran it and whichever on-disk
//! layout that agent uses today. Layouts follow what Codex 0.164, Claude Code
//! 2.1 and the Cursor agent wrote in November 2026.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let home = tmp
            .path()
            .canonicalize()
            .expect("canonical tmp")
            .join("home");
        let project = home.join("src").join("ledger-svc");
        for dir in [
            project.join("scripts"),
            project.join(".claude/worktrees/dedupe-redis"),
            home.join("src/ledger-svc-old"),
        ] {
            fs::create_dir_all(dir).expect("create dirs");
        }
        Self {
            _tmp: tmp,
            home,
            project,
        }
    }

    fn path(&self, rel: &str) -> String {
        if rel.is_empty() {
            self.project.display().to_string()
        } else {
            self.project.join(rel).display().to_string()
        }
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(args)
            .env("HOME", &self.home)
            .env_remove("SIMPLEMMR_HOME")
            .env_remove("MMR_DEFAULT_SOURCE")
            .env_remove("MMR_AUTO_DISCOVER_PROJECT")
            .current_dir(cwd)
            .output()
            .expect("run mmr");
        assert!(
            output.status.success(),
            "mmr {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("stdout JSON")
    }
}

fn write_lines(path: &Path, rows: &[Value]) -> Vec<u8> {
    let body = rows
        .iter()
        .map(|row| row.to_string() + "\n")
        .collect::<String>()
        .into_bytes();
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, &body).expect("write");
    body
}

/// Codex 0.164: the prompt is a `response_item` user message (next to the
/// injected AGENTS.md and environment context) plus an
/// `item_completed`/`UserMessage` event; there is no `user_message` event.
fn codex_rows(id: &str, cwd: &str, prompt: &str, reply: &str) -> Vec<Value> {
    vec![
        json!({"timestamp":"2026-11-03T10:12:00.000Z","type":"session_meta","payload":{"id":id,"timestamp":"2026-11-03T10:12:00.000Z","cwd":cwd,"originator":"codex-tui","cli_version":"0.164.0","model_provider":"openai"}}),
        json!({"timestamp":"2026-11-03T10:12:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("# AGENTS.md instructions for {cwd}\n\n<INSTRUCTIONS>\nKeep diffs small.\n</INSTRUCTIONS>")},{"type":"input_text","text":format!("<environment_context>\n  <cwd>{cwd}</cwd>\n</environment_context>")}]}}),
        json!({"timestamp":"2026-11-03T10:12:05.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":prompt}]}}),
        json!({"timestamp":"2026-11-03T10:12:05.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u1","client_id":"u1","content":[{"type":"text","text":prompt,"text_elements":[]}]}}}),
        json!({"timestamp":"2026-11-03T10:12:40.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":reply}]}}),
        json!({"timestamp":"2026-11-03T10:12:40.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"a1","content":[{"type":"Text","text":reply}],"phase":"final_answer"}}}),
    ]
}

fn claude_rows(id: &str, cwd: &str, prompt: &str, reply: &str, day: &str) -> Vec<Value> {
    vec![
        json!({"type":"user","sessionId":id,"cwd":cwd,"uuid":"u1","parentUuid":null,"timestamp":format!("{day}T09:00:00.000Z"),"message":{"role":"user","content":prompt}}),
        json!({"type":"assistant","sessionId":id,"cwd":cwd,"uuid":"a1","parentUuid":"u1","timestamp":format!("{day}T09:00:30.000Z"),"message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":reply}]}}),
    ]
}

fn claude_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn seed(world: &World) {
    let home = &world.home;
    let root = world.path("");

    // Codex, current format, project root.
    write_lines(
        &home.join(".codex/sessions/2026/11/03/rollout-2026-11-03T10-12-00-codex-live.jsonl"),
        &codex_rows(
            "codex-live",
            &root,
            "Never change the signature of deliver(); billing-svc imports it.",
            "Understood.",
        ),
    );
    // Codex, archived and zstd-compressed.
    let archived = codex_rows(
        "codex-archived",
        &root,
        "Scaffold the delivery loop.",
        "Delivery retries up to 5 times.",
    )
    .iter()
    .map(|row| row.to_string() + "\n")
    .collect::<String>();
    let compressed = ruzstd::encoding::compress_to_vec(
        archived.as_bytes(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    let archive =
        home.join(".codex/archived_sessions/rollout-2026-10-19T10-12-00-codex-archived.jsonl.zst");
    fs::create_dir_all(archive.parent().expect("parent")).expect("mkdir");
    fs::write(&archive, compressed).expect("write archive");

    // Claude Code, in a worktree under the project.
    let worktree = world.path(".claude/worktrees/dedupe-redis");
    write_lines(
        &home.join(format!(
            ".claude/projects/{}/claude-worktree.jsonl",
            claude_dir(&worktree)
        )),
        &claude_rows(
            "claude-worktree",
            &worktree,
            "Try Redis for dedupe.",
            "Scrapped: on-prem installs have no Redis.",
            "2026-10-27",
        ),
    );

    // Cursor: directory named without a leading "-", no timestamp fields,
    // user text wrapped in <timestamp>/<user_query>.
    let cursor_slug = root.trim_start_matches('/').replace(['/', '.', '_'], "-");
    write_lines(
        &home.join(format!(
            ".cursor/projects/{cursor_slug}/agent-transcripts/cursor-1/cursor-1.jsonl"
        )),
        &[
            json!({"role":"user","message":{"content":[{"type":"text","text":"<timestamp>Friday, Oct 30, 2026, 3:05 PM (UTC+1)</timestamp>\n<user_query>\nAdd full jitter to the backoff.\n</user_query>"}]}}),
            json!({"role":"assistant","message":{"content":[{"type":"text","text":"Added full jitter."},{"type":"tool_use","name":"Read","input":{"target_file":"ledger/delivery.py"}}]}}),
            json!({"type":"turn_ended","status":"success"}),
        ],
    );

    // Grok, started in the project's scripts/ directory.
    let scripts = world.path("scripts");
    let grok_dir = home
        .join(".grok/sessions")
        .join(scripts.replace('/', "%2F"))
        .join("grok-1");
    fs::create_dir_all(&grok_dir).expect("grok dir");
    fs::write(
        grok_dir.join("summary.json"),
        json!({"info":{"id":"grok-1","cwd":scripts},"created_at":"2026-11-04T08:00:00Z","updated_at":"2026-11-04T08:05:00Z","current_model_id":"grok-build","num_messages":2}).to_string(),
    )
    .expect("grok summary");
    write_lines(
        &grok_dir.join("updates.jsonl"),
        &[
            json!({"timestamp":1793779200,"method":"session/update","params":{"sessionId":"grok-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"Test against the staging endpoint."}},"_meta":{"agentTimestampMs":1793779200000_i64}}}),
            json!({"timestamp":1793779260,"method":"session/update","params":{"sessionId":"grok-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"48 of 50 accepted."}},"_meta":{"agentTimestampMs":1793779260000_i64}}}),
        ],
    );

    // Sibling directory whose name starts with the project's name: not part of it.
    let sibling = home.join("src/ledger-svc-old").display().to_string();
    write_lines(
        &home.join(format!(
            ".claude/projects/{}/claude-sibling.jsonl",
            claude_dir(&sibling)
        )),
        &claude_rows(
            "claude-sibling",
            &sibling,
            "old prototype",
            "retry 10 times",
            "2026-09-01",
        ),
    );
    // A session in the home directory itself.
    let home_dir = home.display().to_string();
    write_lines(
        &home.join(format!(
            ".claude/projects/{}/claude-home.jsonl",
            claude_dir(&home_dir)
        )),
        &claude_rows(
            "claude-home",
            &home_dir,
            "tidy dotfiles",
            "done",
            "2026-11-01",
        ),
    );
}

fn session_ids(response: &Value) -> Vec<String> {
    let mut ids = response["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|session| session["session_id"].as_str().expect("id").to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[test]
fn project_scope_includes_every_harness_and_nested_directories() {
    let world = World::new();
    seed(&world);

    let listed = world.run(&world.project, &["list", "sessions", "--limit", "50"]);
    assert_eq!(
        session_ids(&listed),
        [
            "claude-worktree",
            "codex-archived",
            "codex-live",
            "cursor-1",
            "grok-1",
        ],
        "sessions from the project root, its subdirectories and worktrees, and no sibling"
    );

    let explicit = world.run(
        &world.home,
        &[
            "list",
            "sessions",
            "--project",
            &world.path(""),
            "--limit",
            "50",
        ],
    );
    assert_eq!(session_ids(&explicit), session_ids(&listed));

    // From the home directory, nested sessions are not pulled in wholesale.
    let from_home = world.run(&world.home, &["list", "sessions", "--limit", "50"]);
    assert_eq!(session_ids(&from_home), ["claude-home"]);

    // `read project` from the project directory carries the nested sessions' text.
    let project = world.run(&world.project, &["read", "project"]);
    let text = project["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|message| message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "billing-svc imports it",
        "retries up to 5 times",
        "on-prem installs have no Redis",
        "Add full jitter",
        "staging endpoint",
    ] {
        assert!(
            text.contains(expected),
            "read project is missing {expected:?} in {project}"
        );
    }
}

#[test]
fn codex_current_format_user_messages_are_read_once_without_injected_context() {
    let world = World::new();
    seed(&world);

    let session = world.run(&world.project, &["read", "session", "codex-live"]);
    let messages = session["messages"].as_array().expect("messages");
    let roles_and_text = messages
        .iter()
        .map(|m| {
            format!(
                "{}: {}",
                m["role"].as_str().unwrap_or(""),
                m["content"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        roles_and_text,
        [
            "user: Never change the signature of deliver(); billing-svc imports it.",
            "assistant: Understood.",
        ]
    );

    let archived = world.run(&world.project, &["read", "session", "codex-archived"]);
    assert_eq!(archived["messages"].as_array().expect("messages").len(), 2);
}

#[test]
fn cursor_sessions_get_their_project_path_timestamps_and_plain_user_text() {
    let world = World::new();
    seed(&world);

    let projects = world.run(&world.home, &["--source", "cursor", "list", "projects"]);
    assert_eq!(projects["projects"][0]["original_path"], world.path(""));

    let session = world.run(&world.project, &["read", "session", "cursor-1"]);
    let messages = session["messages"].as_array().expect("messages");
    assert_eq!(messages[0]["content"], "Add full jitter to the backoff.");
    assert_eq!(messages[0]["timestamp"], "2026-10-30T14:05:00.000Z");
    assert_eq!(
        messages[1]["timestamp"], "2026-10-30T14:05:00.000Z",
        "inherits the turn's time"
    );
}

#[test]
fn skill_install_reaches_claude_code_and_the_shared_skills_directory() {
    let world = World::new();
    let response = world.run(&world.home, &["skill", "install"]);
    let expected = [".agents/skills/mmr", ".claude/skills/mmr"]
        .map(|rel| world.home.join(rel).display().to_string());
    assert_eq!(response["paths"], json!(expected));
    for dir in &expected {
        let skill = fs::read_to_string(Path::new(dir).join("SKILL.md")).expect("SKILL.md");
        assert!(
            skill.starts_with("---\nname: mmr\n"),
            "{dir} has the bundled skill"
        );
    }
}
