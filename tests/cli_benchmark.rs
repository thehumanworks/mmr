use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Instant;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn append_file(path: &Path, contents: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
}

fn run_cli(args: &[&str], home: &Path, db_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args(args)
        .env("HOME", home)
        .env("MMR_DB_PATH", db_path)
        .output()
        .unwrap()
}

fn build_large_claude_session(pair_count: usize, start_idx: usize) -> String {
    let mut out = String::with_capacity(pair_count * 300);
    for i in 0..pair_count {
        let n = start_idx + i;
        out.push_str(&format!(
            "{{\"type\":\"user\",\"sessionId\":\"sess-bench-1\",\"message\":{{\"role\":\"user\",\"content\":\"hello {n}\"}},\"timestamp\":\"2025-01-01T00:{:02}:{:02}\",\"uuid\":\"u-{n}\"}}\n",
            (n / 60) % 60,
            n % 60,
        ));
        out.push_str(&format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"sess-bench-1\",\"message\":{{\"role\":\"assistant\",\"content\":\"world {n}\",\"model\":\"claude-3-opus\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":40}}}},\"timestamp\":\"2025-01-01T00:{:02}:{:02}\",\"uuid\":\"a-{n}\",\"parentUuid\":\"u-{n}\"}}\n",
            ((n + 1) / 60) % 60,
            (n + 1) % 60,
        ));
    }
    out
}

#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_incremental_refresh_is_faster_than_full_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let claude_session = home
        .join(".claude")
        .join("projects")
        .join("-Users-test-bench")
        .join("sess-bench-1.jsonl");

    let base_pairs = 10_000;
    write_file(&claude_session, &build_large_claude_session(base_pairs, 0));

    let db_path = tmp.path().join("cache.duckdb");

    let full_start = Instant::now();
    let full_out = run_cli(&["--quiet", "ingest"], &home, &db_path);
    let full_elapsed = full_start.elapsed();
    assert!(
        full_out.status.success(),
        "full ingest failed: {}",
        String::from_utf8_lossy(&full_out.stderr)
    );

    append_file(&claude_session, &build_large_claude_session(1, base_pairs));

    let inc_start = Instant::now();
    let inc_out = run_cli(&["--quiet", "projects"], &home, &db_path);
    let inc_elapsed = inc_start.elapsed();
    assert!(
        inc_out.status.success(),
        "incremental ingest failed: {}",
        String::from_utf8_lossy(&inc_out.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&inc_out.stdout).unwrap();
    assert_eq!(
        json["total_messages"].as_i64().unwrap(),
        (base_pairs * 2 + 2) as i64
    );

    eprintln!(
        "benchmark_full_ms={} benchmark_incremental_ms={}",
        full_elapsed.as_millis(),
        inc_elapsed.as_millis()
    );

    assert!(
        inc_elapsed < full_elapsed,
        "expected incremental refresh ({:?}) to be faster than full rebuild ({:?})",
        inc_elapsed,
        full_elapsed
    );
}
