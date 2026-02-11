use std::fs;
use std::process::Command;

fn write_file(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn cli_errors_when_cache_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let db_path = tmp.path().join("missing.duckdb");

    let out = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("projects")
        .env("HOME", &home)
        .env("MMR_DB_PATH", &db_path)
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "expected non-zero exit, stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("mmr ingest") || stderr.contains("ingest"),
        "stderr should instruct how to build cache, stderr={}",
        stderr
    );
}

#[test]
fn cli_uses_cache_and_does_not_reingest() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    // --- Create tiny Claude fixture under HOME/.claude/projects/... ---
    let claude_session = home
        .join(".claude")
        .join("projects")
        .join("-Users-test-proj")
        .join("sess-claude-1.jsonl");
    write_file(
        &claude_session,
        r#"{"type":"user","sessionId":"sess-claude-1","message":{"role":"user","content":"hello from claude"},"timestamp":"2025-01-01T00:00:00","uuid":"u1"}
{"type":"assistant","sessionId":"sess-claude-1","message":{"role":"assistant","content":"hi from assistant","model":"claude-3-opus","usage":{"input_tokens":100,"output_tokens":50}},"timestamp":"2025-01-01T00:01:00","uuid":"a1","parentUuid":"u1"}"#,
    );

    // --- Create tiny Codex fixture under HOME/.codex/sessions/... ---
    let codex_session = home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"hello from codex"}}
{"type":"response_item","timestamp":"2025-01-02T00:00:02","payload":{"role":"assistant","content":[{"type":"output_text","text":"hi from codex assistant"}]}}"#,
    );

    let db_path = tmp.path().join("cache.duckdb");

    // Build the cache from fixtures.
    let out = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("--quiet")
        .arg("ingest")
        .env("HOME", &home)
        .env("MMR_DB_PATH", &db_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ingest failed, stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Remove the raw sources; if the CLI tries to reingest, it would fail.
    fs::remove_dir_all(home.join(".claude")).unwrap();
    fs::remove_dir_all(home.join(".codex")).unwrap();

    // Query should succeed purely from the cache.
    let out = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .arg("projects")
        .env("HOME", &home)
        .env("MMR_DB_PATH", &db_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "projects failed, stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "projects should not emit stderr, stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["total_messages"].as_i64().unwrap(), 4);
    assert_eq!(json["total_sessions"].as_i64().unwrap(), 2);
    assert_eq!(json["projects"].as_array().unwrap().len(), 2);
}
