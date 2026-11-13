#[allow(dead_code)]
mod common;

use std::fs;
use std::io::BufRead;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{SpacedProjectFixture, TestFixture, parse_stdout_json, rows};

const DEV_SUBSKILLS: [&str; 4] = [
    "goal-closeout",
    "review-remediation",
    "docs-first-contract-change",
    "command-surface-removal",
];

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

fn assert_dev_subskills_absent_from_text(text: &str) {
    for subskill in DEV_SUBSKILLS {
        assert!(
            !text.contains(subskill),
            "dev subskill {subskill} should not be bundled"
        );
    }
}

fn assert_dev_subskills_absent_from_dir(target: &Path) {
    for subskill in DEV_SUBSKILLS {
        assert!(
            !target.join(subskill).exists(),
            "dev subskill {subskill} should not be installed"
        );
    }
}

fn assert_dev_subskills_absent_from_install_response(json: &serde_json::Value) {
    let files = json["files"].as_array().expect("files array");
    for subskill in DEV_SUBSKILLS {
        assert!(
            !files
                .iter()
                .any(|file| file.as_str().is_some_and(|path| path.starts_with(subskill))),
            "dev subskill {subskill} should not be listed in installed files"
        );
    }
}

fn replace_claude_projects_with_file(home: &Path) {
    let projects = home.join(".claude").join("projects");
    fs::remove_dir_all(&projects).expect("remove claude projects dir");
    fs::write(&projects, "not a directory").expect("replace claude projects dir");
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
    assert!(stdout.contains("# mmr: the project's earlier agent sessions"));
    assert!(stdout.contains("## mmr/session-mining/SKILL.md"));
    assert!(stdout.contains("session-mining"));
    assert_dev_subskills_absent_from_text(&stdout);
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
    assert_dev_subskills_absent_from_install_response(&json);
    assert!(target.join("SKILL.md").is_file());
    assert!(target.join("session-mining").join("SKILL.md").is_file());
    assert!(
        target
            .join("session-mining")
            .join("references")
            .join("session-retrieval-patterns.md")
            .is_file()
    );
    assert_dev_subskills_absent_from_dir(&target);
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
    assert_dev_subskills_absent_from_install_response(&json);
    assert!(target.join("SKILL.md").is_file());
    assert!(target.join("session-mining").join("SKILL.md").is_file());
    assert_dev_subskills_absent_from_dir(&target);
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
fn source_filtered_commands_ignore_unselected_provider_load_errors() {
    let fixture = TestFixture::seeded();
    replace_claude_projects_with_file(&fixture.home);

    let codex = fixture.run_cli(&["--source", "codex", "list", "projects"]);
    assert!(
        codex.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&codex.stderr)
    );
    let codex_json = parse_stdout_json(&codex);
    assert!(
        codex_json["projects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|project| project["source"] == "codex")
    );

    let claude = fixture.run_cli(&["--source", "claude", "list", "projects"]);
    assert!(!claude.status.success());
    let stderr = String::from_utf8_lossy(&claude.stderr);
    assert!(
        stderr.contains("failed to read") && stderr.contains(".claude/projects"),
        "stderr={stderr}"
    );
}

#[test]
fn all_source_commands_report_provider_load_errors() {
    let fixture = TestFixture::seeded();
    replace_claude_projects_with_file(&fixture.home);

    let output = fixture.run_cli(&["list", "projects"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to read") && stderr.contains(".claude/projects"),
        "stderr={stderr}"
    );
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
    let output = fixture.run_cli_with_env(
        &["--source", "cursor", "read", "session", "sess-cursor-1"],
        &[],
    );

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
    let output =
        fixture.run_cli_with_env(&["--source", "grok", "read", "session", "sess-grok-1"], &[]);

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
    let output = fixture.run_cli_with_env(&["--source", "pi", "read", "session", "sess-pi-1"], &[]);

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
fn read_project_in_cwd_pages_like_explicit_project() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let cwd_str = fs::canonicalize(&cwd)
        .expect("canonicalize cwd project")
        .to_string_lossy()
        .into_owned();

    for offset in ["0", "1", "3"] {
        let implicit = fixture.run_cli_in_dir(
            &["read", "project", "--limit", "1", "--offset", offset],
            &cwd,
        );
        let explicit = fixture.run_cli_in_dir(
            &[
                "read",
                "project",
                "--project",
                &cwd_str,
                "--limit",
                "1",
                "--offset",
                offset,
            ],
            &cwd,
        );
        for output in [&implicit, &explicit] {
            assert!(
                output.status.success(),
                "offset={offset} stderr={}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let implicit = parse_stdout_json(&implicit);
        let explicit = parse_stdout_json(&explicit);
        for field in ["messages", "total_messages", "next_page", "next_offset"] {
            assert_eq!(implicit[field], explicit[field], "offset={offset} {field}");
        }
        if let Some(command) = implicit["next_command"].as_str() {
            assert!(!command.contains("--project"), "{command}");
        }
    }

    // The first page is the newest message, as with --project.
    let first =
        parse_stdout_json(&fixture.run_cli_in_dir(&["read", "project", "--limit", "1"], &cwd));
    assert_eq!(first["messages"][0]["content"], "cwd claude answer");
}

#[test]
fn read_project_and_find_in_cwd_include_subdirectory_sessions() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let subdir = cwd.join("scripts");
    fs::create_dir_all(&subdir).expect("create subdirectory");
    let subdir_str = fs::canonicalize(&subdir)
        .expect("canonicalize subdirectory")
        .to_string_lossy()
        .into_owned();
    write_file(
        &fixture
            .home
            .join(".codex")
            .join("sessions")
            .join("sess-cwd-subdir.jsonl"),
        &format!(
            r#"{{"type":"session_meta","timestamp":"2025-01-09T00:00:00","payload":{{"id":"sess-cwd-subdir","cwd":"{subdir_str}","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-09T00:00:00"}}}}
{{"type":"event_msg","timestamp":"2025-01-09T00:00:01","payload":{{"type":"user_message","message":"subdirmarker question"}}}}"#
        ),
    );

    let read = fixture.run_cli_in_dir(&["read", "project"], &cwd);
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    let read = parse_stdout_json(&read);
    assert!(
        read["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["session_id"] == "sess-cwd-subdir"),
        "{read}"
    );

    let found = fixture.run_cli_in_dir(&["find", "subdirmarker"], &cwd);
    assert!(
        found.status.success(),
        "{}",
        String::from_utf8_lossy(&found.stderr)
    );
    let found = parse_stdout_json(&found);
    let matches = found["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1, "{found}");
    assert_eq!(matches[0]["session_id"], "sess-cwd-subdir");
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
    let output = fixture.run_cli_with_env(
        &["--source", "claude", "read", "session", "sess-claude-1"],
        &[],
    );

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

// --- retrieve ---

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
fn context_project_returns_sessions_newest_first_and_newest_message_window() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "context",
        "project",
        "--project",
        "/Users/test/codex-proj",
        "--limit",
        "3",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "context/project");
    assert_eq!(json["scope"], "project");
    assert!(json["source"].is_null());
    assert_eq!(json["project"], "/Users/test/codex-proj");
    assert_eq!(
        rows(&json["sessions"], &["/session_id", "/last_timestamp"]),
        [
            "sess-codex-1|2025-01-02T00:05:00",
            "sess-codex-2|2025-01-02T00:03:00",
        ]
    );
    assert_eq!(
        rows(&json["messages"], &["/timestamp", "/session_id"]),
        [
            "2025-01-02T00:02:00|sess-codex-2",
            "2025-01-02T00:03:00|sess-codex-2",
            "2025-01-02T00:05:00|sess-codex-1",
        ]
    );
    assert_eq!(json["total_sessions"], 2);
    assert_eq!(json["total_messages"], 6);
    assert!(json.get("peer_results").is_none());
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
fn read_project_next_command_quotes_project_paths_with_spaces() {
    let fixture = SpacedProjectFixture::seeded();
    let page1 = fixture.run_cli(&[
        "--source",
        "codex",
        "read",
        "project",
        "--project",
        fixture.project_arg(),
        "--limit",
        "1",
    ]);
    assert!(
        page1.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page1.stderr)
    );

    let page1_json = parse_stdout_json(&page1);
    assert!(page1_json["next_page"].as_bool().unwrap());
    let next_command = page1_json["next_command"].as_str().unwrap();
    assert!(
        next_command.contains("--project '"),
        "next_command={next_command}"
    );
    assert!(
        next_command.contains(fixture.project_arg()),
        "next_command={next_command}"
    );

    let page2 = fixture.run_shell_command(next_command);
    assert!(
        page2.status.success(),
        "next_command={next_command}\nstderr={}",
        String::from_utf8_lossy(&page2.stderr)
    );
    let page2_json = parse_stdout_json(&page2);
    assert_eq!(page2_json["messages"].as_array().unwrap().len(), 1);
    assert!(
        page2_json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["project_name"] == fixture.project_arg())
    );
}

#[test]
fn read_project_pagination_no_next_command_when_all_results_fit() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "read",
        "project",
        "--project",
        "/Users/test/codex-proj",
        "--limit",
        "100",
    ]);

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
fn messages_session_without_project_or_source_searches_every_source_quietly() {
    let fixture = TestFixture::seeded();

    let output = fixture.run_cli(&["read", "session", "sess-claude-1"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(parse_stdout_json(&output)["total_messages"], 2);

    // Omitting --source is the recommended form, so it gets no hint pushing toward it.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("--source"),
        "no --source hint expected, got: {stderr}"
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

    // Asking for a project that doesn't contain the session is an error, not an empty session.
    let missing_output = fixture.run_cli(&[
        "read",
        "session",
        "sess-claude-1",
        "--project",
        "/Users/test/codex-proj",
    ]);
    assert!(!missing_output.status.success());
    let stderr = String::from_utf8_lossy(&missing_output.stderr);
    assert!(
        stderr.contains("no session sess-claude-1 found in /Users/test/codex-proj"),
        "{stderr}"
    );
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
fn recall_next_command_quotes_project_paths_with_spaces() {
    let fixture = SpacedProjectFixture::seeded();
    let page1 = fixture.run_cli(&[
        "--source",
        "codex",
        "recall",
        "--project",
        fixture.project_arg(),
        "1",
        "--limit",
        "1",
    ]);
    assert!(
        page1.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&page1.stderr)
    );

    let page1_json = parse_stdout_json(&page1);
    assert!(page1_json["next_page"].as_bool().unwrap());
    let next_command = page1_json["next_command"].as_str().unwrap();
    assert!(
        next_command.contains("--project '"),
        "next_command={next_command}"
    );
    assert!(
        next_command.contains(fixture.project_arg()),
        "next_command={next_command}"
    );

    let page2 = fixture.run_shell_command(next_command);
    assert!(
        page2.status.success(),
        "next_command={next_command}\nstderr={}",
        String::from_utf8_lossy(&page2.stderr)
    );
    let page2_json = parse_stdout_json(&page2);
    let messages = page2_json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages
            .iter()
            .all(|message| message["project_name"] == fixture.project_arg())
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
fn read_project_omits_session_selection_field() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&[
        "read",
        "project",
        "--project",
        "/Users/test/codex-proj",
        "--limit",
        "5",
    ]);
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
fn peer_status_host_ssh_failure_is_structured() {
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
        &["peer", "status", "--host", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_ssh_failed");
    assert_eq!(json["host"], "studio");
}

#[test]
fn peer_status_host_rejects_option_like_target() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["peer", "status", "--host=-oProxyCommand=sh"]);
    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_target_invalid");
    assert_eq!(json["host"], "-oProxyCommand=sh");
}

#[test]
fn peer_status_host_remote_mmr_missing_is_structured() {
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
        &["peer", "status", "--host", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_mmr_unavailable");
    assert_eq!(json["host"], "studio");
}

#[test]
fn peer_status_host_remote_incompatible_is_structured() {
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
        &["peer", "status", "--host", "studio"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error_kind"], "peer_mmr_unavailable");
    assert_eq!(json["host"], "studio");
}

#[test]
fn peer_status_host_remote_peer_subcommand_missing_is_structured() {
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
        &["peer", "status", "--host", "studio"],
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
fn share_session_ssh_rejects_option_like_target() {
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
        "--to=-oProxyCommand=sh",
        "--dry-run",
    ]);

    assert_cli_failure(&output, 2, Some("share/session"));
    let json = parse_stdout_json(&output);
    assert!(json["planned_commands"].is_null());
    assert!(
        json["message"]
            .as_str()
            .expect("failure message")
            .contains("must not start with '-'")
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
fn import_session_from_remote_defaults_to_read_only() {
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

#[test]
fn import_session_rejects_invalid_peer_bundles_before_cache_writes() {
    let fixture = TestFixture::seeded();
    let (_, bundle_path) = share_codex_bundle_to_file(&fixture);
    let response_path = fixture.home.join("invalid-peer-response.json");
    peer_pack_response_from_bundle(&fixture, &bundle_path, &response_path);
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(&response_path).unwrap()).unwrap();
    let fake_bin = fixture.home.join("fake-bin-invalid-peer");
    write_executable(
        &fake_bin.join("ssh"),
        "#!/bin/sh\ncat > /dev/null\ncat \"$MMR_FAKE_PEER_RESPONSE\"\n",
    );
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    for (kind, expected) in [
        ("outer-id", "peer bundle_id does not match manifest"),
        ("manifest-id", "bundle_id does not match content-addressed"),
        ("hash", "artifact hash mismatch"),
    ] {
        let mut response = original.clone();
        match kind {
            "outer-id" => response["bundle_id"] = serde_json::json!("../../outside"),
            "manifest-id" => {
                response["bundle"]["manifest"]["bundle_id"] = serde_json::json!("../../outside")
            }
            _ => {
                let key = response["bundle"]["files"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .next()
                    .unwrap()
                    .clone();
                response["bundle"]["files"][&key] = serde_json::json!("corrupted");
            }
        }
        fs::write(&response_path, response.to_string()).unwrap();
        let output = fixture.run_cli_with_env(
            &[
                "import",
                "session",
                "--from",
                "fixture-peer",
                "--session",
                "sess-codex-1",
            ],
            &[
                ("PATH", &path),
                ("MMR_FAKE_PEER_RESPONSE", response_path.to_str().unwrap()),
            ],
        );
        assert_cli_failure(&output, 3, Some("import/session"));
        assert!(
            parse_stdout_json(&output)["message"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
        assert!(!fixture.home.join(".mmr/teleport/cache").exists());
        assert!(!fixture.home.join(".mmr/outside").exists());
    }
}

#[test]
fn import_session_force_without_apply_is_usage_error_before_peer_access() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["import", "session", "--from", "unused-peer", "--force"]);
    assert_cli_failure(&output, 2, Some("import/session"));
    assert_eq!(
        parse_stdout_json(&output)["message"],
        "--force requires --apply"
    );
}

#[test]
fn remote_flag_is_rejected_on_every_command() {
    let fixture = TestFixture::seeded();
    let commands: &[&[&str]] = &[
        &["list", "projects"],
        &["list", "sessions"],
        &["read", "project"],
        &["read", "session", "sess-codex-1"],
        &["context", "project"],
        &["recall"],
    ];
    for command in commands {
        let mut args = command.to_vec();
        args.extend(["--remote", "studio"]);
        let output = fixture.run_cli(&args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unexpected argument '--remote'"),
            "args={args:?} stderr={stderr}"
        );
    }
}

#[test]
fn source_scoped_subcommands_are_rejected() {
    let fixture = TestFixture::seeded();
    for command in ["read", "context"] {
        let output = fixture.run_cli(&["--source", "codex", command, "source"]);
        assert_eq!(output.status.code(), Some(2), "{command} source");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unrecognized subcommand 'source'"),
            "{command} source stderr={stderr}"
        );
        assert!(output.stdout.is_empty(), "{command} source stdout");
    }
}

#[test]
fn assimilate_command_is_rejected() {
    let fixture = TestFixture::seeded();
    for args in [
        &["assimilate", "project"][..],
        &["--source", "codex", "assimilate", "source"][..],
    ] {
        let output = fixture.run_cli(args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unrecognized subcommand 'assimilate'"),
            "args={args:?} stderr={stderr}"
        );
    }
}

#[test]
fn store_commands_are_rejected() {
    let fixture = TestFixture::seeded();
    for (args, unknown) in [
        (&["init"][..], "unrecognized subcommand 'init'"),
        (
            &["ingest", "events", "--project", "/tmp/p"][..],
            "unrecognized subcommand 'ingest'",
        ),
        (&["note", "decision"][..], "unrecognized subcommand 'note'"),
        (
            &["retrieve", "retry"][..],
            "unrecognized subcommand 'retrieve'",
        ),
        (&["status"][..], "unrecognized subcommand 'status'"),
        (&["sync", "--dry-run"][..], "unrecognized subcommand 'sync'"),
        (&["redact", "scan"][..], "unrecognized subcommand 'redact'"),
        (&["__db-info"][..], "unrecognized subcommand '__db-info'"),
        (
            &["read", "session", "sess-codex-1", "--format", "tree"][..],
            "unexpected argument '--format'",
        ),
        (
            &["read", "project", "--output-dir", "/tmp/tree"][..],
            "unexpected argument '--output-dir'",
        ),
    ] {
        let output = fixture.run_cli(args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(unknown), "args={args:?} stderr={stderr}");
        assert!(output.stdout.is_empty(), "args={args:?}");
    }
}

#[test]
fn model_commands_are_rejected() {
    let fixture = TestFixture::seeded();
    for (args, unknown) in [
        (
            &["summarize", "project"][..],
            "unrecognized subcommand 'summarize'",
        ),
        (
            &["summarize", "session", "sess-codex-1"][..],
            "unrecognized subcommand 'summarize'",
        ),
        (
            &["compact", "project"][..],
            "unrecognized subcommand 'compact'",
        ),
        (
            &["compact", "session", "sess-codex-1"][..],
            "unrecognized subcommand 'compact'",
        ),
    ] {
        let output = fixture.run_cli(args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(unknown), "args={args:?} stderr={stderr}");
        assert!(output.stdout.is_empty(), "args={args:?}");
    }
}

#[test]
fn closed_stdout_reader_exits_quietly() {
    // `mmr ... | head` closes the pipe before mmr finishes writing. mmr must
    // stop without a panic, a stack trace or a failing exit code.
    let fixture = TestFixture::seeded();
    let cwd = fixture.home.join("big-project");
    fs::create_dir_all(&cwd).expect("create project dir");
    let cwd_str = fs::canonicalize(&cwd)
        .expect("canonicalize project dir")
        .to_string_lossy()
        .into_owned();
    // Larger than a pipe buffer, so the write cannot finish without a reader.
    let big = "x".repeat(256 * 1024);
    write_file(
        &fixture
            .home
            .join(".claude")
            .join("projects")
            .join(encode_claude_project_name(&cwd_str))
            .join("sess-big.jsonl"),
        &format!(
            r#"{{"type":"user","sessionId":"sess-big","message":{{"role":"user","content":"{big}"}},"timestamp":"2025-01-07T00:01:00","uuid":"u-big-1","cwd":"{cwd_str}"}}"#
        ),
    );

    for args in [
        &["context", "project", "--limit", "5"][..],
        &["read", "project", "--limit", "5"][..],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
            .args(args)
            .env("HOME", &fixture.home)
            .env("RUST_BACKTRACE", "1")
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn mmr");
        drop(child.stdout.take());
        let output = child.wait_with_output().expect("wait for mmr");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(0),
            "args={args:?} stderr={stderr}"
        );
        assert!(stderr.is_empty(), "args={args:?} stderr={stderr}");
    }
}

#[test]
fn default_source_env_is_ignored() {
    let fixture = TestFixture::seeded();
    let sources = |args: &[&str]| {
        let output = fixture.run_cli_with_env(args, &[("MMR_DEFAULT_SOURCE", "codex")]);
        assert!(
            output.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut sources = rows(&parse_stdout_json(&output)["projects"], &["/source"]);
        sources.sort();
        sources.dedup();
        sources
    };
    assert_eq!(
        sources(&["list", "projects", "--limit", "20"]),
        ["claude", "codex", "cursor", "grok", "pi"]
    );
    assert_eq!(
        sources(&["--source", "claude", "list", "projects", "--limit", "20"]),
        ["claude"]
    );
}

// --- find: raw history search, no setup ---

fn run_read_command(fixture: &TestFixture, read_command: &str) -> serde_json::Value {
    let words = read_command.split_whitespace().collect::<Vec<_>>();
    assert_eq!(words[0], "mmr", "read_command={read_command}");
    let output = fixture.run_cli(&words[1..]);
    assert!(
        output.status.success(),
        "read_command={read_command} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_stdout_json(&output)
}

#[test]
fn find_searches_the_cwd_project_across_harnesses_without_setup() {
    let fixture = TestFixture::seeded();
    let cwd = seed_cwd_project_with_history(&fixture);
    let output = fixture.run_cli_in_dir_with_env(
        &["find", "CWD"],
        &cwd,
        &[("MMR_AUTO_DISCOVER_PROJECT", "1")],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "find");
    assert_eq!(json["query"], "CWD");
    assert_eq!(json["total_matches"], 4);
    assert_eq!(
        rows(
            &json["matches"],
            &["/source", "/session_id", "/role", "/snippet"]
        ),
        [
            "claude|sess-cwd-claude|assistant|cwd claude answer",
            "claude|sess-cwd-claude|user|cwd claude question",
            "codex|sess-cwd-codex|assistant|cwd codex answer",
            "codex|sess-cwd-codex|user|cwd codex question",
        ]
    );
    assert_eq!(json["next_page"], false);
    assert!(json["next_command"].is_null());

    for found in json["matches"].as_array().unwrap() {
        let read = run_read_command(&fixture, found["read_command"].as_str().unwrap());
        assert!(
            read["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["content"] == found["snippet"]),
            "read_command should return the matched message: {found}"
        );
    }
    assert!(
        !fixture.home.join(".local/share/mmr").exists(),
        "find must not create a store"
    );
}

#[test]
fn find_scopes_to_a_project_or_every_project() {
    let fixture = TestFixture::seeded();
    let project = fixture.run_cli(&["find", "hello from", "--project", "/Users/test/grok-proj"]);
    assert!(project.status.success());
    let project = parse_stdout_json(&project);
    assert_eq!(project["scope"]["project"], "/Users/test/grok-proj");
    assert_eq!(rows(&project["matches"], &["/source"]), ["grok"]);

    let all = fixture.run_cli(&["find", "hello from", "--all"]);
    assert!(all.status.success());
    let all = parse_stdout_json(&all);
    assert_eq!(all["scope"]["all"], true);
    let mut sources = rows(&all["matches"], &["/source"]);
    sources.sort();
    sources.dedup();
    assert_eq!(sources, ["claude", "codex", "cursor", "grok", "pi"]);

    let session = fixture.run_cli(&["find", "hello", "--all", "--session", "sess-pi-1"]);
    assert!(session.status.success());
    assert_eq!(
        rows(&parse_stdout_json(&session)["matches"], &["/session_id"]),
        ["sess-pi-1"]
    );
}

#[test]
fn find_pages_with_a_next_command() {
    let fixture = TestFixture::seeded();
    let first = fixture.run_cli(&["find", "hello from", "--all", "--limit", "2"]);
    assert!(first.status.success());
    let first = parse_stdout_json(&first);
    assert_eq!(first["matches"].as_array().unwrap().len(), 2);
    assert_eq!(first["next_page"], true);
    assert_eq!(first["next_offset"], 2);
    let next_command = first["next_command"].as_str().unwrap();
    assert_eq!(
        next_command,
        "mmr find 'hello from' --all --limit 2 --offset 2"
    );
    let second = fixture.run_cli(&[
        "find",
        "hello from",
        "--all",
        "--limit",
        "2",
        "--offset",
        "2",
    ]);
    let second = parse_stdout_json(&second);
    assert_ne!(first["matches"][0], second["matches"][0]);
    assert_eq!(first["total_matches"], second["total_matches"]);
}

#[test]
fn find_rejects_an_empty_query() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["find", ""]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn find_printed_commands_run_as_printed_for_a_path_with_spaces() {
    let fixture = SpacedProjectFixture::seeded();
    let output = fixture.run_cli(&[
        "find",
        "RANKING TIE",
        "--project",
        fixture.project_arg(),
        "--limit",
        "2",
    ]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let first = parse_stdout_json(&output);
    assert_eq!(first["total_matches"], 6, "{first}");
    let next_command = first["next_command"].as_str().unwrap();
    assert!(next_command.contains("--project '"), "{next_command}");

    let second = fixture.run_shell_command(next_command);
    assert!(
        second.status.success(),
        "next_command={next_command} stderr={}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second = parse_stdout_json(&second);
    assert_eq!(second["matches"].as_array().unwrap().len(), 2);
    assert_eq!(second["next_offset"], 4);

    let mut sources = std::collections::BTreeSet::new();
    for page in [&first, &second] {
        for found in page["matches"].as_array().unwrap() {
            sources.insert(found["source"].as_str().unwrap().to_string());
            let read_command = found["read_command"].as_str().unwrap();
            let read = fixture.run_shell_command(read_command);
            assert!(
                read.status.success(),
                "read_command={read_command} stderr={}",
                String::from_utf8_lossy(&read.stderr)
            );
            let read = parse_stdout_json(&read);
            let snippet = found["snippet"].as_str().unwrap();
            assert!(
                read["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["content"] == snippet),
                "read_command={read_command} snippet={snippet}"
            );
        }
    }
    assert_eq!(
        sources.into_iter().collect::<Vec<_>>(),
        ["claude", "codex"],
        "find should match both harnesses"
    );
}
