//! End-to-end contracts for the SSH peer paths: `import session --from` and
//! `peer status --host`. A loopback `ssh` runs this build's `mmr peer …`
//! against a second seeded HOME, so client and peer server code run together.

#[allow(dead_code)]
mod common;

use common::{PeerLoopback, TestFixture, parse_stdout_json};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn import_args(extra: &[&'static str]) -> Vec<&'static str> {
    let mut args = vec![
        "--source",
        "codex",
        "import",
        "session",
        "--from",
        "studio",
        "--session",
        "sess-remote-a",
        "--project",
        "/Users/test/codex-proj",
    ];
    args.extend_from_slice(extra);
    args
}

#[test]
fn import_session_from_peer_reads_the_remote_session_without_writing_it() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);
    let path = remote.path_env();
    let native = fixture.home.join(".codex/sessions/sess-remote-a.jsonl");

    let output = fixture.run_cli_with_env(&import_args(&[]), &[("PATH", path.as_str())]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "import/session");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["read"]["command"], "import/session/read");
    assert_eq!(json["read"]["message_count"], 2);
    assert!(!native.exists(), "read-only import must not restore files");
    assert_eq!(
        remote.ssh_calls(),
        vec!["studio mmr peer teleport-pack --request-json -".to_string()]
    );
}

#[test]
fn import_session_from_peer_with_apply_restores_the_remote_session() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);
    let path = remote.path_env();
    let native = fixture.home.join(".codex/sessions/sess-remote-a.jsonl");

    let output = fixture.run_cli_with_env(&import_args(&["--apply"]), &[("PATH", path.as_str())]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "import/session");
    assert_eq!(json["read_only"], false);
    let restored = std::fs::read_to_string(&native).expect("restored native session");
    assert!(
        restored.contains("remote codex question"),
        "restored={restored}"
    );

    // The restored session is now readable locally.
    let output = fixture.run_cli(&["--source", "codex", "read", "session", "sess-remote-a"]);
    assert!(output.status.success());
    let json = parse_stdout_json(&output);
    assert_eq!(json["total_messages"], 2);
    assert_eq!(json["messages"][0]["content"], "remote codex question");
}

#[test]
fn peer_status_host_reports_the_remote_teleport_capability() {
    let fixture = TestFixture::seeded();
    let remote = PeerLoopback::seeded(&fixture);
    let path = remote.path_env();

    let output = fixture.run_cli_with_env(
        &["peer", "status", "--host", "studio"],
        &[("PATH", path.as_str())],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["command"], "peer/status");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["protocol_version"], 1);
    assert_eq!(json["mmr_version"], VERSION);
    let capabilities = json["capabilities"].as_array().expect("capabilities");
    assert!(
        capabilities.iter().any(|item| item == "teleport-pack"),
        "capabilities={capabilities:?}"
    );
}

#[test]
fn peer_teleport_pack_rejects_unknown_protocol_versions() {
    let fixture = TestFixture::seeded();
    let output = fixture.run_cli(&["peer", "teleport-pack", "--request-json", "-"]);
    assert!(!output.status.success(), "empty stdin is not a request");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["peer", "teleport-pack", "--request-json", "-"])
        .env("HOME", &fixture.home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn peer");
    std::io::Write::write_all(
        child.stdin.as_mut().expect("stdin"),
        br#"{"protocol_version":99,"project":{"local_path":"/Users/test/codex-proj","display_name":"codex-proj"}}"#,
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
