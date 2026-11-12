//! End-to-end `--remote` contracts. A loopback `ssh` runs this build's
//! `mmr peer …` against a second seeded HOME, so these tests cover the client
//! fan-out and merge code and the peer server code together.

#[allow(dead_code)]
mod common;

use common::{PeerLoopback, TestFixture, parse_stdout_json, rows};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn run(fixture: &TestFixture, remote: &PeerLoopback, args: &[&str]) -> serde_json::Value {
    let path = remote.path_env();
    let output = fixture.run_cli_with_env(args, &[("PATH", path.as_str())]);
    assert!(
        output.status.success(),
        "args={args:?} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_stdout_json(&output)
}

/// Follow a printed `next_command` exactly as a user would paste it.
fn run_next(
    fixture: &TestFixture,
    remote: &PeerLoopback,
    json: &serde_json::Value,
) -> serde_json::Value {
    let command = json["next_command"].as_str().expect("next_command");
    let args = command.split_whitespace().skip(1).collect::<Vec<_>>();
    run(fixture, remote, &args)
}

fn peer_result(
    command: &str,
    total_messages: Option<i64>,
    total_sessions: Option<i64>,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "host": "studio",
        "transport": "ssh",
        "command": command,
        "status": "ok",
        "remote_mmr_version": VERSION,
    });
    if let Some(total) = total_messages {
        value["total_messages"] = total.into();
    }
    if let Some(total) = total_sessions {
        value["total_sessions"] = total.into();
    }
    serde_json::json!([value])
}

const MESSAGE_ROW: &[&str] = &["/timestamp", "/session_id", "/role", "/origin/host"];

#[test]
fn list_projects_remote_merges_both_machines_in_sort_order_then_pages() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);

    let json = run(
        &fixture,
        &remote,
        &["list", "projects", "--remote", "studio"],
    );
    assert_eq!(
        rows(&json["projects"], &["/name", "/source", "/origin/host"]),
        [
            "/Users/test/codex-proj|codex|studio",
            "/Users/test/remote-only|codex|studio",
            "/Users/test/grok-proj|grok|-",
            "--Users-test-pi-proj--|pi|-",
            "/Users/test/codex-recent|codex|-",
            "/Users/test/codex-proj|codex|-",
            "-Users-test-proj|claude|-",
            "-Users-test-proj|claude|studio",
            "-Users-test-cursor-proj|cursor|-",
        ]
    );
    assert_eq!(json["projects"][0]["origin"]["remote_mmr_version"], VERSION);
    assert_eq!(json["total_messages"], 25, "17 local + 8 remote");
    assert_eq!(json["total_sessions"], 12, "7 local + 5 remote");
    assert_eq!(
        json["peer_results"],
        peer_result("list/projects", Some(8), Some(5))
    );

    let page = run(
        &fixture,
        &remote,
        &[
            "list", "projects", "--remote", "studio", "--limit", "3", "--offset", "2",
        ],
    );
    assert_eq!(
        rows(&page["projects"], &["/name", "/origin/host"]),
        [
            "/Users/test/grok-proj|-",
            "--Users-test-pi-proj--|-",
            "/Users/test/codex-recent|-",
        ]
    );
    assert_eq!(page["total_messages"], 25);
    assert_eq!(
        remote.ssh_calls(),
        ["studio mmr peer list-projects --request-json -"; 2]
    );
}

#[test]
fn list_sessions_remote_keeps_both_copies_of_a_shared_session() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);

    let json = run(
        &fixture,
        &remote,
        &[
            "list",
            "sessions",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
        ],
    );
    assert_eq!(
        rows(
            &json["sessions"],
            &["/session_id", "/last_timestamp", "/origin/host"]
        ),
        [
            "sess-remote-b|2025-01-07T00:00:02|studio",
            "sess-codex-1|2025-01-02T00:05:00|-",
            "sess-codex-1|2025-01-02T00:05:00|studio",
            "sess-remote-a|2025-01-02T00:04:30|studio",
            "sess-codex-2|2025-01-02T00:03:00|-",
        ]
    );
    assert_eq!(json["total_sessions"], 5);
    assert_eq!(
        json["peer_results"],
        peer_result("list/sessions", None, Some(3))
    );

    let page = run(
        &fixture,
        &remote,
        &[
            "list",
            "sessions",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
            "--sort-by",
            "message-count",
            "--order",
            "asc",
            "--limit",
            "2",
            "--offset",
            "3",
        ],
    );
    assert_eq!(
        rows(&page["sessions"], &["/session_id", "/origin/host"]),
        ["sess-remote-b|studio", "sess-codex-2|-"]
    );
    assert_eq!(page["total_sessions"], 5);
}

#[test]
fn read_project_remote_dedups_orders_and_pages_forward_from_oldest() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);
    let base = [
        "--source",
        "codex",
        "read",
        "project",
        "--project",
        "/Users/test/codex-proj",
        "--remote",
        "studio",
    ];

    let all = run(&fixture, &remote, &base);
    assert_eq!(
        rows(&all["messages"], MESSAGE_ROW),
        [
            "2025-01-02T00:00:01|sess-codex-1|user|-",
            "2025-01-02T00:00:30|sess-codex-2|user|-",
            "2025-01-02T00:01:00|sess-codex-2|assistant|-",
            "2025-01-02T00:02:00|sess-codex-2|user|-",
            "2025-01-02T00:03:00|sess-codex-2|assistant|-",
            "2025-01-02T00:04:00|sess-remote-a|user|studio",
            "2025-01-02T00:04:30|sess-remote-a|assistant|studio",
            "2025-01-02T00:05:00|sess-codex-1|assistant|-",
            "2025-01-07T00:00:01|sess-remote-b|user|studio",
            "2025-01-07T00:00:02|sess-remote-b|assistant|studio",
        ],
        "the remote copy of sess-codex-1 is dropped as a duplicate"
    );
    assert_eq!(all["total_messages"], 10);
    assert_eq!(all["next_page"], false);
    assert_eq!(all["next_offset"], 10);
    assert!(all.get("next_command").is_none());
    assert_eq!(
        all["peer_results"],
        peer_result("read/project", Some(6), None)
    );

    let mut args = base.to_vec();
    args.extend(["--limit", "3", "--offset", "2"]);
    let page = run(&fixture, &remote, &args);
    assert_eq!(
        rows(&page["messages"], &["/timestamp"]),
        [
            "2025-01-02T00:01:00",
            "2025-01-02T00:02:00",
            "2025-01-02T00:03:00"
        ]
    );
    assert_eq!(page["total_messages"], 10);
    assert_eq!(page["next_page"], true);
    assert_eq!(page["next_offset"], 5);
    assert_eq!(
        page["next_command"],
        "mmr --source codex read project --project /Users/test/codex-proj --remote studio --limit 3 --offset 5"
    );

    let next = run_next(&fixture, &remote, &page);
    assert_eq!(
        rows(&next["messages"], MESSAGE_ROW),
        [
            "2025-01-02T00:04:00|sess-remote-a|user|studio",
            "2025-01-02T00:04:30|sess-remote-a|assistant|studio",
            "2025-01-02T00:05:00|sess-codex-1|assistant|-",
        ]
    );
    let last = run_next(&fixture, &remote, &next);
    assert_eq!(rows(&last["messages"], &["/session_id"]).len(), 2);
    assert_eq!(last["next_page"], false);
    assert!(last.get("next_command").is_none());
}

#[test]
fn read_session_remote_merges_the_same_session_from_both_machines() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);

    let shared = run(
        &fixture,
        &remote,
        &[
            "read",
            "session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
        ],
    );
    assert_eq!(
        rows(&shared["messages"], MESSAGE_ROW),
        [
            "2025-01-02T00:00:01|sess-codex-1|user|-",
            "2025-01-02T00:05:00|sess-codex-1|assistant|-",
        ]
    );
    assert_eq!(shared["total_messages"], 2);
    assert_eq!(
        shared["peer_results"],
        peer_result("read/session", Some(2), None)
    );

    let remote_only = run(
        &fixture,
        &remote,
        &[
            "read",
            "session",
            "sess-remote-a",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
            "--limit",
            "1",
        ],
    );
    assert_eq!(
        rows(&remote_only["messages"], MESSAGE_ROW),
        ["2025-01-02T00:04:00|sess-remote-a|user|studio"]
    );
    assert_eq!(remote_only["total_messages"], 2);
    assert_eq!(remote_only["next_page"], true);
    assert_eq!(remote_only["next_offset"], 1);
    assert!(remote_only.get("next_command").is_none());
}

#[test]
fn read_source_remote_merges_every_project_of_the_source() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);

    let all = run(
        &fixture,
        &remote,
        &["--source", "codex", "read", "source", "--remote", "studio"],
    );
    assert_eq!(
        rows(&all["messages"], MESSAGE_ROW),
        [
            "2025-01-02T00:00:01|sess-codex-1|user|-",
            "2025-01-02T00:00:30|sess-codex-2|user|-",
            "2025-01-02T00:01:00|sess-codex-2|assistant|-",
            "2025-01-02T00:02:00|sess-codex-2|user|-",
            "2025-01-02T00:03:00|sess-codex-2|assistant|-",
            "2025-01-02T00:04:00|sess-remote-a|user|studio",
            "2025-01-02T00:04:30|sess-remote-a|assistant|studio",
            "2025-01-02T00:05:00|sess-codex-1|assistant|-",
            "2025-01-03T00:00:01|sess-codex-recent-1|user|-",
            "2025-01-03T00:01:00|sess-codex-recent-1|assistant|-",
            "2025-01-06T00:00:01|sess-remote-only|user|studio",
            "2025-01-07T00:00:01|sess-remote-b|user|studio",
            "2025-01-07T00:00:02|sess-remote-b|assistant|studio",
        ]
    );
    assert_eq!(all["total_messages"], 13);
    assert_eq!(
        all["peer_results"],
        peer_result("read/source", Some(7), None)
    );

    let page = run(
        &fixture,
        &remote,
        &[
            "--source", "codex", "read", "source", "--remote", "studio", "--limit", "4",
        ],
    );
    assert_eq!(
        rows(&page["messages"], &["/timestamp"]),
        [
            "2025-01-02T00:00:01",
            "2025-01-02T00:00:30",
            "2025-01-02T00:01:00",
            "2025-01-02T00:02:00",
        ]
    );
    assert_eq!(page["next_page"], true);
    assert_eq!(page["next_offset"], 4);
    assert!(page.get("next_command").is_none());
}

#[test]
fn context_project_remote_dedups_sessions_and_keeps_newest_message_window() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);

    let json = run(
        &fixture,
        &remote,
        &[
            "--source",
            "codex",
            "context",
            "project",
            "--project",
            "/Users/test/codex-proj",
            "--remote",
            "studio",
            "--limit",
            "3",
        ],
    );
    assert_eq!(json["command"], "context/project");
    assert_eq!(json["scope"], "project");
    assert_eq!(json["source"], "codex");
    assert_eq!(json["project"], "/Users/test/codex-proj");
    assert_eq!(
        rows(&json["sessions"], &["/session_id", "/origin/host"]),
        [
            "sess-remote-b|studio",
            "sess-codex-1|-",
            "sess-remote-a|studio",
            "sess-codex-2|-",
        ]
    );
    assert_eq!(
        rows(&json["messages"], MESSAGE_ROW),
        [
            "2025-01-02T00:05:00|sess-codex-1|assistant|-",
            "2025-01-07T00:00:01|sess-remote-b|user|studio",
            "2025-01-07T00:00:02|sess-remote-b|assistant|studio",
        ]
    );
    assert_eq!(json["total_sessions"], 4);
    assert_eq!(
        json["total_messages"], 12,
        "6 local + 6 remote, before dedup"
    );
    assert_eq!(
        json["peer_results"],
        peer_result("context/project", Some(6), Some(3))
    );
}

#[test]
fn context_source_remote_lists_sessions_from_both_machines() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);

    let json = run(
        &fixture,
        &remote,
        &[
            "--source", "codex", "context", "source", "--remote", "studio", "--limit", "3",
        ],
    );
    assert_eq!(json["command"], "context/source");
    assert_eq!(json["scope"], "source");
    assert_eq!(json["source"], "codex");
    assert!(json["project"].is_null());
    assert_eq!(
        rows(&json["sessions"], &["/session_id", "/origin/host"]),
        [
            "sess-remote-b|studio",
            "sess-remote-only|studio",
            "sess-codex-recent-1|-",
            "sess-codex-1|-",
            "sess-codex-1|studio",
            "sess-remote-a|studio",
            "sess-codex-2|-",
        ],
        "source context does not dedup sessions (unlike project context)"
    );
    assert_eq!(
        rows(&json["messages"], MESSAGE_ROW),
        [
            "2025-01-06T00:00:01|sess-remote-only|user|studio",
            "2025-01-07T00:00:01|sess-remote-b|user|studio",
            "2025-01-07T00:00:02|sess-remote-b|assistant|studio",
        ]
    );
    assert_eq!(json["total_sessions"], 7);
    assert_eq!(
        json["total_messages"], 15,
        "8 local + 7 remote, before dedup"
    );
    assert_eq!(
        json["peer_results"],
        peer_result("context/source", Some(7), Some(4))
    );
}

#[test]
fn recall_remote_merges_each_machines_previous_session_and_pages() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);
    let base = [
        "--source",
        "codex",
        "recall",
        "--project",
        "/Users/test/codex-proj",
        "--remote",
        "studio",
        "1",
    ];

    let all = run(&fixture, &remote, &base);
    assert_eq!(
        rows(&all["messages"], MESSAGE_ROW),
        [
            "2025-01-02T00:00:01|sess-codex-1|user|studio",
            "2025-01-02T00:00:30|sess-codex-2|user|-",
            "2025-01-02T00:01:00|sess-codex-2|assistant|-",
            "2025-01-02T00:02:00|sess-codex-2|user|-",
            "2025-01-02T00:03:00|sess-codex-2|assistant|-",
            "2025-01-02T00:05:00|sess-codex-1|assistant|studio",
        ],
        "local age 1 is sess-codex-2; remote age 1 is its copy of sess-codex-1"
    );
    assert_eq!(all["total_messages"], 6);
    assert_eq!(all["next_page"], false);
    assert_eq!(
        all["session_selection"]["selected"][0]["session_id"],
        "sess-codex-2"
    );
    assert_eq!(
        all["session_selection"]["skipped_newest"]["session_id"],
        "sess-codex-1"
    );
    assert_eq!(all["peer_results"], peer_result("recall", Some(2), None));

    let mut args = base.to_vec();
    args.extend(["--limit", "1"]);
    let page = run(&fixture, &remote, &args);
    assert_eq!(
        rows(&page["messages"], MESSAGE_ROW),
        ["2025-01-02T00:00:01|sess-codex-1|user|studio"]
    );
    assert_eq!(page["next_offset"], 1);
    assert_eq!(
        page["next_command"],
        "mmr --source codex recall --project /Users/test/codex-proj --remote studio 1 --limit 1 --offset 1"
    );
    let next = run_next(&fixture, &remote, &page);
    assert_eq!(
        rows(&next["messages"], MESSAGE_ROW),
        ["2025-01-02T00:00:30|sess-codex-2|user|-"]
    );
    assert_eq!(
        next["next_command"],
        "mmr --source codex recall --project /Users/test/codex-proj --remote studio 1 --limit 1 --offset 2"
    );
}

#[test]
fn peer_server_rejects_unknown_protocol_versions() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["peer", "read-source", "--request-json", "-"]);
    assert!(!output.status.success(), "empty stdin is not a request");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["peer", "list-projects", "--request-json", "-"])
        .env("HOME", &fixture.home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn peer");
    std::io::Write::write_all(
        child.stdin.as_mut().expect("stdin"),
        br#"{"protocol_version":99,"limits":{"limit":null,"offset":0},"sort_by":"timestamp","order":"desc"}"#,
    )
    .expect("write request");
    let output = child.wait_with_output().expect("peer output");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsupported peer protocol version 99"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}
