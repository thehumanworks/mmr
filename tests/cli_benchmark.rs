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
        .args(["--source", "codex", "projects"])
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

fn spawn_teleport_serve(
    source_home: &Path,
    extra_args: &[&str],
) -> (std::process::Child, serde_json::Value) {
    let mut args = vec![
        "teleport",
        "serve",
        "--source",
        "codex",
        "--session",
        "sess-codex-1",
        "--project",
        "/Users/test/codex-proj",
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
        .expect("spawn teleport serve");

    let stdout = child.stdout.take().expect("serve stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read teleport serve startup JSON");
    let startup = serde_json::from_str(&line).expect("parse serve startup JSON");
    (child, startup)
}

fn empty_target_home(base: &Path) -> PathBuf {
    let target_home = base.join("target-home");
    fs::create_dir_all(&target_home).expect("create target HOME");
    target_home
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_teleport_pack_inspect_apply_readability() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let source_home = create_source_home(tmp.path());
    let target_home = empty_target_home(tmp.path());
    let bundle_path = tmp.path().join("teleport-handoff.mmr");
    let target_project = "/Users/test/bench-target-proj";
    let session_id = "sess-codex-1";

    let started = Instant::now();

    let pack_output = run_mmr(
        &source_home,
        &[
            "teleport",
            "pack",
            "--source",
            "codex",
            "--session",
            session_id,
            "--project",
            "/Users/test/codex-proj",
            "--to",
            bundle_path.to_str().expect("bundle path"),
        ],
    );
    assert!(
        pack_output.status.success(),
        "pack stderr={}",
        String::from_utf8_lossy(&pack_output.stderr)
    );
    let pack_json = parse_stdout_json(&pack_output);
    assert_eq!(pack_json["status"], "ok");

    let inspect_output = run_mmr(
        &source_home,
        &[
            "teleport",
            "inspect",
            bundle_path.to_str().expect("bundle path"),
        ],
    );
    assert!(
        inspect_output.status.success(),
        "inspect stderr={}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspect_json = parse_stdout_json(&inspect_output);
    assert_eq!(inspect_json["apply_ready"], true);

    let apply_output = run_mmr(
        &target_home,
        &[
            "teleport",
            "apply",
            "--to",
            bundle_path.to_str().expect("bundle path"),
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
            "messages",
            "--source",
            "codex",
            "--project",
            target_project,
            "--session",
            session_id,
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
        "BENCH teleport.pack_inspect_apply_readability session={} message_count={} elapsed_ms={}",
        session_id,
        messages.len(),
        elapsed.as_millis()
    );
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_teleport_file_send_receive_two_machine() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let source_home = create_source_home(tmp.path());
    let target_home = empty_target_home(tmp.path());
    let inbox = tmp.path().join("shared-inbox");
    let to = format!("file://{}", inbox.display());
    let target_project = "/Users/test/bench-file-target";

    let started = Instant::now();

    let send_output = run_mmr(
        &source_home,
        &[
            "teleport",
            "send",
            "--source",
            "codex",
            "--session",
            "sess-codex-1",
            "--project",
            "/Users/test/codex-proj",
            "--to",
            &to,
        ],
    );
    assert!(
        send_output.status.success(),
        "send stderr={}",
        String::from_utf8_lossy(&send_output.stderr)
    );
    let send_json = parse_stdout_json(&send_output);
    assert_eq!(send_json["transport"], "file");
    let bundle_id = send_json["bundle_id"].as_str().expect("bundle_id");
    let entry = inbox.join(bundle_id);

    let receive_output = run_mmr(
        &target_home,
        &[
            "teleport",
            "receive",
            "--to",
            entry.to_str().expect("inbox entry"),
            "--project",
            target_project,
        ],
    );
    assert!(
        receive_output.status.success(),
        "receive stderr={}",
        String::from_utf8_lossy(&receive_output.stderr)
    );
    let receive_json = parse_stdout_json(&receive_output);
    assert_eq!(receive_json["apply"]["status"], "ok");

    let elapsed = started.elapsed();
    eprintln!(
        "BENCH teleport.file_send_receive bundle_id={} elapsed_ms={}",
        bundle_id,
        elapsed.as_millis()
    );
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_teleport_http_loopback_receive() {
    if !loopback_bind_available() {
        eprintln!("BENCH teleport.http_loopback_receive skipped: loopback bind unavailable");
        return;
    }

    let tmp = tempfile::tempdir().expect("temp dir");
    let source_home = create_source_home(tmp.path());
    let target_home = empty_target_home(tmp.path());
    let target_project = "/Users/test/bench-http-target";

    let started = Instant::now();
    let (mut serve_child, startup) = spawn_teleport_serve(&source_home, &[]);
    assert_eq!(startup["transport"], "http");
    let listen_url = startup["listen_url"].as_str().expect("listen_url");

    let receive_output = run_mmr(
        &target_home,
        &[
            "teleport",
            "receive",
            listen_url,
            "--project",
            target_project,
        ],
    );
    assert!(
        receive_output.status.success(),
        "receive stderr={}",
        String::from_utf8_lossy(&receive_output.stderr)
    );
    let receive_json = parse_stdout_json(&receive_output);
    assert_eq!(receive_json["apply"]["status"], "ok");

    let serve_status = serve_child.wait().expect("wait for serve");
    assert!(
        serve_status.success(),
        "serve should exit after one download"
    );

    let elapsed = started.elapsed();
    eprintln!(
        "BENCH teleport.http_loopback_receive listen_url={} elapsed_ms={}",
        listen_url,
        elapsed.as_millis()
    );
}
