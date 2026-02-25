use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
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
