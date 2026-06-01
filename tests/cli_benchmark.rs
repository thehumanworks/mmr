use std::fs;
use std::io::BufRead;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
}

fn seed_codex_source_home(home: &Path) {
    let codex_session = home
        .join(".codex")
        .join("sessions")
        .join("sess-codex-1.jsonl");
    write_file(
        &codex_session,
        r#"{"type":"session_meta","timestamp":"2025-01-02T00:00:00","payload":{"id":"sess-codex-1","cwd":"/Users/test/codex-proj","cli_version":"1.0.0","model_provider":"openai","timestamp":"2025-01-02T00:00:00","git":{"branch":"main"}}}
{"type":"event_msg","timestamp":"2025-01-02T00:00:01","payload":{"type":"user_message","message":"hello from codex"}}
{"type":"response_item","timestamp":"2025-01-02T00:05:00","payload":{"role":"assistant","content":[{"type":"output_text","text":"short codex answer"}]}}"#,
    );
}

fn create_source_home(base: &Path) -> PathBuf {
    let source_home = base.join("source-home");
    fs::create_dir_all(&source_home).expect("create source HOME");
    seed_codex_source_home(&source_home);
    source_home
}

fn parse_stdout_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout JSON")
}

fn loopback_bind_available() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
}

fn build_codex_session(session_id: &str, cwd: &str, pair_count: usize, start: usize) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{{\"type\":\"session_meta\",\"timestamp\":\"2025-01-01T00:00:00\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"{cwd}\",\"cli_version\":\"1.0.0\",\"model_provider\":\"openai\",\"timestamp\":\"2025-01-01T00:00:00\",\"git\":{{\"branch\":\"main\"}}}}}}\n"
    ));

    for i in 0..pair_count {
        let n = start + i;
        out.push_str(&format!(
            "{{\"type\":\"event_msg\",\"timestamp\":\"2025-01-01T00:{:02}:{:02}\",\"payload\":{{\"type\":\"user_message\",\"message\":\"q-{n}\"}}}}\n",
            (n / 60) % 60,
            n % 60
        ));
        out.push_str(&format!(
            "{{\"type\":\"response_item\",\"timestamp\":\"2025-01-01T00:{:02}:{:02}\",\"payload\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"a-{n}\"}}]}}}}\n",
            ((n + 1) / 60) % 60,
            (n + 1) % 60
        ));
    }

    out
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_projects_query_parses_large_fixture() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("create HOME");

    let project_path = "/Users/bench/codex-proj";
    let sessions = 50usize;
    let pairs_per_session = 20usize;
    let expected_messages = (sessions * pairs_per_session * 2) as i64;

    for i in 0..sessions {
        let file = home
            .join(".codex")
            .join("sessions")
            .join(format!("sess-bench-{i}.jsonl"));
        let contents = build_codex_session(
            &format!("sess-bench-{i}"),
            project_path,
            pairs_per_session,
            i * pairs_per_session,
        );
        write_file(&file, &contents);
    }

    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(["--source", "codex", "list", "projects"])
        .env("HOME", &home)
        .output()
        .expect("run mmr");
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout JSON");
    assert_eq!(json["total_messages"].as_i64().unwrap(), expected_messages);
    assert_eq!(json["projects"].as_array().unwrap().len(), 1);

    eprintln!(
        "BENCH mmr.projects fixture_sessions={} pairs_per_session={} elapsed_ms={}",
        sessions,
        pairs_per_session,
        elapsed.as_millis()
    );
}

fn run_mmr(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run mmr")
}

fn spawn_share_session_http(
    source_home: &Path,
    extra_args: &[&str],
) -> (std::process::Child, serde_json::Value) {
    let mut args = vec![
        "--source",
        "codex",
        "share",
        "session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
        "--via",
        "http",
        "--bind",
        "127.0.0.1:0",
        "--timeout",
        "30",
    ];
    args.extend_from_slice(extra_args);

    let mut child = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(&args)
        .env("HOME", source_home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn share session http");

    let stdout = child.stdout.take().expect("share stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read share startup JSON");
    let startup = serde_json::from_str(&line).expect("parse share startup JSON");
    (child, startup)
}

fn empty_target_home(base: &Path) -> PathBuf {
    let target_home = base.join("target-home");
    fs::create_dir_all(&target_home).expect("create target HOME");
    target_home
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_share_import_bundle_readability() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let source_home = create_source_home(tmp.path());
    let target_home = empty_target_home(tmp.path());
    let bundle_path = tmp.path().join("session-handoff.mmr");
    let to = format!("file://{}", tmp.path().join("bundle-out").display());
    let target_project = "/Users/test/bench-target-proj";
    let session_id = "sess-codex-1";

    let started = Instant::now();

    let share_output = run_mmr(
        &source_home,
        &[
            "--source",
            "codex",
            "share",
            "session",
            session_id,
            "--project",
            "/Users/test/codex-proj",
            "--to",
            &to,
        ],
    );
    assert!(
        share_output.status.success(),
        "share stderr={}",
        String::from_utf8_lossy(&share_output.stderr)
    );
    let share_json = parse_stdout_json(&share_output);
    assert_eq!(share_json["status"], "ok");
    let inbox_path = PathBuf::from(share_json["inbox_path"].as_str().expect("inbox_path"));
    fs::copy(inbox_path.join("bundle.mmr"), &bundle_path).expect("copy benchmark bundle");

    let read_output = run_mmr(
        &source_home,
        &[
            "import",
            "bundle",
            bundle_path.to_str().expect("bundle path"),
            "--read-only",
        ],
    );
    assert!(
        read_output.status.success(),
        "read stderr={}",
        String::from_utf8_lossy(&read_output.stderr)
    );
    let read_json = parse_stdout_json(&read_output);
    assert_eq!(read_json["status"], "ok");

    let apply_output = run_mmr(
        &target_home,
        &[
            "import",
            "bundle",
            bundle_path.to_str().expect("bundle path"),
            "--apply",
            "--project",
            target_project,
        ],
    );
    assert!(
        apply_output.status.success(),
        "apply stderr={}",
        String::from_utf8_lossy(&apply_output.stderr)
    );
    let apply_json = parse_stdout_json(&apply_output);
    assert_eq!(apply_json["status"], "ok");

    let messages_output = run_mmr(
        &target_home,
        &[
            "--source",
            "codex",
            "read",
            "session",
            session_id,
            "--project",
            target_project,
        ],
    );
    assert!(
        messages_output.status.success(),
        "messages stderr={}",
        String::from_utf8_lossy(&messages_output.stderr)
    );
    let messages_json = parse_stdout_json(&messages_output);
    let messages = messages_json["messages"]
        .as_array()
        .expect("messages array");
    assert!(!messages.is_empty());

    let elapsed = started.elapsed();
    eprintln!(
        "BENCH share_import.bundle_readability session={} message_count={} elapsed_ms={}",
        session_id,
        messages.len(),
        elapsed.as_millis()
    );
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_share_file_import_bundle_two_machine() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let source_home = create_source_home(tmp.path());
    let target_home = empty_target_home(tmp.path());
    let inbox = tmp.path().join("shared-inbox");
    let to = format!("file://{}", inbox.display());
    let target_project = "/Users/test/bench-file-target";

    let started = Instant::now();

    let share_output = run_mmr(
        &source_home,
        &[
            "--source",
            "codex",
            "share",
            "session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--to",
            &to,
        ],
    );
    assert!(
        share_output.status.success(),
        "share stderr={}",
        String::from_utf8_lossy(&share_output.stderr)
    );
    let share_json = parse_stdout_json(&share_output);
    assert_eq!(share_json["transport"], "file");
    let bundle_id = share_json["bundle_id"].as_str().expect("bundle_id");
    let entry = inbox.join(bundle_id);

    let import_output = run_mmr(
        &target_home,
        &[
            "import",
            "bundle",
            entry.to_str().expect("inbox entry"),
            "--apply",
            "--project",
            target_project,
        ],
    );
    assert!(
        import_output.status.success(),
        "import stderr={}",
        String::from_utf8_lossy(&import_output.stderr)
    );
    let import_json = parse_stdout_json(&import_output);
    assert_eq!(import_json["apply"]["status"], "ok");

    let elapsed = started.elapsed();
    eprintln!(
        "BENCH share_import.file_bundle bundle_id={} elapsed_ms={}",
        bundle_id,
        elapsed.as_millis()
    );
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_share_http_import_bundle_loopback() {
    if !loopback_bind_available() {
        eprintln!("BENCH share_import.http_bundle skipped: loopback bind unavailable");
        return;
    }

    let tmp = tempfile::tempdir().expect("temp dir");
    let source_home = create_source_home(tmp.path());
    let target_home = empty_target_home(tmp.path());
    let target_project = "/Users/test/bench-http-target";

    let started = Instant::now();
    let (mut share_child, startup) = spawn_share_session_http(&source_home, &[]);
    assert_eq!(startup["transport"], "http");
    let listen_url = startup["listen_url"].as_str().expect("listen_url");

    let import_output = run_mmr(
        &target_home,
        &[
            "import",
            "bundle",
            listen_url,
            "--apply",
            "--project",
            target_project,
        ],
    );
    assert!(
        import_output.status.success(),
        "import stderr={}",
        String::from_utf8_lossy(&import_output.stderr)
    );
    let import_json = parse_stdout_json(&import_output);
    assert_eq!(import_json["apply"]["status"], "ok");

    let serve_status = share_child.wait().expect("wait for share");
    assert!(
        serve_status.success(),
        "share should exit after one download"
    );

    let elapsed = started.elapsed();
    eprintln!(
        "BENCH share_import.http_bundle listen_url={} elapsed_ms={}",
        listen_url,
        elapsed.as_millis()
    );
}
