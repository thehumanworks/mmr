# Test and Benchmark Loop

## Table of Contents
- [Fixture Strategy](#fixture-strategy)
- [Contract Integration Tests](#contract-integration-tests)
- [Benchmark Test](#benchmark-test)
- [Verification Commands](#verification-commands)

## Fixture Strategy

Seed Claude, Codex, and Cursor fixtures under a temp HOME so tests are hermetic.

```rust
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
        seed_cursor_fixture(&home);
        Self { _tmp: tmp, home }
    }
}
```

Source: `tests/common/mod.rs`

## Contract Integration Tests

Keep behavior checks at the CLI level:

- source filtering defaults and overrides
- cwd auto-discovery, `--all`, and explicit `--project` scope
- `messages --session` all-project lookup behavior and stderr hints
- sort and pagination behavior
- chronological message output

Example assertion pattern:

```rust
let output = fixture.run_cli(&["projects"]);
assert!(output.status.success());
let json = parse_stdout_json(&output);
assert_eq!(json["total_messages"].as_i64().unwrap(), 8);
```

Source: `tests/cli_contract.rs`

## Benchmark Test

Use an ignored integration test to benchmark realistic fixture size while remaining opt-in.

```rust
#[test]
#[ignore = "benchmark test: run explicitly"]
fn benchmark_projects_query_parses_large_fixture() {
    // seed many sessions, run CLI once, assert message totals, print elapsed time
}
```

Source: `tests/cli_benchmark.rs:37-84`

## Verification Commands

Run this exact loop before claiming success:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
