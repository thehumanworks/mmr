use mmr::store::{LATEST_SCHEMA_VERSION, Store};

const FIXTURES: &[(&str, &str)] = &[
    (
        "codex_session",
        include_str!("fixtures/memory_fabric/codex_session.jsonl"),
    ),
    (
        "claude_like_session",
        include_str!("fixtures/memory_fabric/claude_like_session.jsonl"),
    ),
    (
        "human_note",
        include_str!("fixtures/memory_fabric/human_note.jsonl"),
    ),
    (
        "tool_output_fake_secret",
        include_str!("fixtures/memory_fabric/tool_output_fake_secret.jsonl"),
    ),
    (
        "pii_heavy_sample",
        include_str!("fixtures/memory_fabric/pii_heavy_sample.jsonl"),
    ),
];

const NHL_269_REQUIRED_TABLES: &[&str] = &[
    "blobs",
    "dream_candidates",
    "dream_runs",
    "events",
    "learned_memory",
    "project_aliases",
    "project_links",
    "projects",
    "redaction_policies",
    "redaction_runs",
    "redaction_spans",
    "schema_migrations",
    "search_documents",
    "sessions",
    "source_cursors",
    "sources",
    "summaries",
    "sync_manifest_entries",
    "sync_manifests",
];

const MALFORMED_MIXED_FIXTURE: &str =
    include_str!("fixtures/memory_fabric/malformed_mixed_session.jsonl");

const MVP_NON_GOAL_COMMANDS: &[&str] = &[
    "init",
    "store",
    "learn",
    "context",
    "candidates",
    "knowledge",
    "promote",
    "reject",
];

#[test]
fn memory_fabric_golden_fixtures_are_valid_jsonl() {
    for (name, contents) in FIXTURES {
        let mut parsed_records = 0;

        for line in contents.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|err| panic!("{name}: {err}"));
            assert!(
                value.is_object(),
                "{name}: fixture rows must be JSON objects"
            );
            parsed_records += 1;
        }

        assert!(parsed_records > 0, "{name}: fixture must not be empty");
    }
}

#[test]
fn memory_fabric_malformed_fixture_contains_valid_and_invalid_lines() {
    let mut valid_records = 0;
    let mut invalid_records = 0;

    for line in MALFORMED_MIXED_FIXTURE
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                assert!(
                    value.is_object(),
                    "valid malformed-fixture rows must be objects"
                );
                valid_records += 1;
            }
            Err(_) => invalid_records += 1,
        }
    }

    assert!(
        valid_records > 0,
        "malformed fixture must include valid rows to preserve defensive parsing"
    );
    assert!(
        invalid_records > 0,
        "malformed fixture must include invalid rows to exercise skip-and-continue parsing"
    );
}

#[test]
fn memory_fabric_non_goal_commands_are_not_public() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).expect("create HOME");

    for command_name in MVP_NON_GOAL_COMMANDS {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mmr"))
            .arg(command_name)
            .env("HOME", &home)
            .output()
            .expect("run mmr");

        assert!(
            !output.status.success(),
            "{command_name} must remain outside the public MVP command surface"
        );
        assert!(
            output.stdout.is_empty(),
            "{command_name} should not write machine output when rejected"
        );
    }
}

#[test]
fn db_info_smoke_links_non_git_project_and_round_trips_synthetic_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join("home");
    let data_home = tmp.path().join("data");
    let project = tmp.path().join("plain-project");
    std::fs::create_dir_all(&home).expect("create HOME");
    std::fs::create_dir_all(&project).expect("create project");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mmr"))
        .args([
            "__db-info",
            "--project",
            project.to_str().expect("project path UTF-8"),
            "--smoke-event",
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("run mmr");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout JSON");
    assert_eq!(json["schema_version"].as_i64().unwrap(), 1);
    assert_eq!(json["event_count"].as_i64().unwrap(), 1);
    assert!(json["project_id"].as_str().unwrap().starts_with("proj:v1:"));
    assert_eq!(
        json["db_path"].as_str().unwrap(),
        data_home.join("mmr").join("mmr.db").to_str().unwrap()
    );
}

#[test]
#[ignore = "pending NHL-277: implement link command"]
fn link_cli_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "mmr link sets up local state from a non-Git cwd, performs safe reconciliation, and prints status JSON",
    );
}

#[test]
#[ignore = "pending NHL-277: implement sync command"]
fn sync_cli_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "mmr sync is idempotent, redaction-gated, non-destructive, and prints sync/status JSON",
    );
}

#[test]
#[ignore = "pending NHL-277: implement status command"]
fn status_cli_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "mmr status reports store, project, source, redaction, and sync state as JSON",
    );
}

#[test]
#[ignore = "pending NHL-271: implement note command"]
fn note_cli_contract_is_implemented() {
    pending_contract(
        "NHL-271",
        "mmr note records argv and stdin text as first-class note events scoped to the current project",
    );
}

#[test]
#[ignore = "pending NHL-273: implement rg command"]
fn rg_cli_contract_is_implemented() {
    pending_contract(
        "NHL-273",
        "mmr rg performs POSIX-friendly exact search over generated documents with citations",
    );
}

#[test]
#[ignore = "pending NHL-273: implement search command"]
fn search_cli_contract_is_implemented() {
    pending_contract(
        "NHL-273",
        "mmr search performs structured exact search with source, role, project, session, event-type, and citation fields",
    );
}

#[test]
#[ignore = "pending NHL-282: implement summary command"]
fn summary_cli_contract_is_implemented() {
    pending_contract(
        "NHL-282",
        "mmr summary mirrors remember selection/provider semantics and keeps remember as a compatibility alias",
    );
}

#[test]
#[ignore = "pending NHL-279: implement dream command"]
fn dream_cli_contract_is_implemented() {
    pending_contract(
        "NHL-279",
        "mmr dream analyzes the current project and writes learned memory only with valid evidence refs",
    );
}

#[test]
fn schema_validation_contract_is_implemented() {
    let store = Store::open_in_memory().expect("store");
    assert_eq!(
        store.schema_version().expect("schema"),
        LATEST_SCHEMA_VERSION
    );

    let tables = store.schema_table_names().expect("tables");
    for table in NHL_269_REQUIRED_TABLES {
        assert!(
            tables.iter().any(|name| name == table),
            "missing NHL-269 table {table}"
        );
    }
}

#[test]
fn migration_replay_contract_is_implemented() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db_path = tmp.path().join("mmr.db");

    let first = Store::open(&db_path).expect("first migration replay");
    assert_eq!(
        first.schema_version().expect("schema"),
        LATEST_SCHEMA_VERSION
    );
    drop(first);

    let second = Store::open(&db_path).expect("idempotent migration replay");
    assert_eq!(
        second.schema_version().expect("schema"),
        LATEST_SCHEMA_VERSION
    );
}

#[test]
#[ignore = "pending NHL-270: implement source adapter normalization"]
fn source_adapter_normalization_contract_is_implemented() {
    pending_contract(
        "NHL-270",
        "normalize Codex, Claude/Cursor-like, human note, tool-output, and malformed mixed fixtures into stable event records",
    );
}

#[test]
#[ignore = "pending NHL-272: implement redaction policy application"]
fn redaction_policy_contract_is_implemented() {
    pending_contract(
        "NHL-272",
        "block sync for deterministic secrets and produce redacted spans for PII-heavy samples",
    );
}

#[test]
#[ignore = "pending NHL-273: implement search document generation"]
fn search_document_contract_is_implemented() {
    pending_contract(
        "NHL-273",
        "generate exact-search documents with event citations for every normalized event",
    );
}

#[test]
#[ignore = "pending NHL-282: implement summary command contract"]
fn summary_generation_contract_is_implemented() {
    pending_contract(
        "NHL-282",
        "route summary through the existing remember runner semantics while keeping remember as a compatibility alias",
    );
}

#[test]
#[ignore = "pending NHL-278/NHL-279: implement dream assimilation validation"]
fn dream_assimilation_contract_is_implemented() {
    pending_contract(
        "NHL-278/NHL-279",
        "validate structured dream output and write learned memory only when every evidence ref resolves",
    );
}

#[test]
#[ignore = "pending NHL-277: implement sync manifest generation"]
fn sync_manifest_contract_is_implemented() {
    pending_contract(
        "NHL-277",
        "generate replayable redacted sync manifests for github:<user>/mmr-store hydration",
    );
}

fn pending_contract(ticket: &str, behavior: &str) -> ! {
    panic!("{ticket} pending contract: {behavior}. Remove #[ignore] when implemented.");
}
