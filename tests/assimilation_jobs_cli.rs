//! Explicit synthetic Store/config/process fixtures; no live provider history or models.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mmr::store::{NewEvent, Store};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

const SENTINEL: &str = "synthetic-private-argument-marker";
const TIME: &str = "2026-01-01T00:00:00Z";

struct Fixture {
    dir: TempDir,
    home: PathBuf,
    database: PathBuf,
    config: PathBuf,
    marker: PathBuf,
    project: String,
    sessions: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let database = dir.path().join("explicit.sqlite3");
        let mut store = Store::open(&database).unwrap();
        let project = store.ensure_project_link(dir.path()).unwrap().id;
        let mut sessions = Vec::new();
        for name in ["session-a", "session-b", "session-c"] {
            let event = store
                .insert_event(
                    &project,
                    &NewEvent::new(
                        "codex",
                        name,
                        "user_turn",
                        "user",
                        TIME,
                        "Synthetic fact for a pending proposal.",
                        "synthetic-cli-v1",
                    ),
                )
                .unwrap();
            sessions.push(event.session_id);
        }
        let config = dir.path().join("jobs.json");
        let marker = dir.path().join("launches.jsonl");
        let script = dir.path().join("fake-command.py");
        fs::write(&script, r#"import json,os,sys
request=json.load(sys.stdin)
with open(sys.argv[1], 'a') as f:
    f.write(json.dumps({'request':request,'env':dict(os.environ)})+'\n')
ref=request['evidence'][0]['evidence_ref']
proposal={'kind':'observation','claim':'Synthetic proposal pending review','confidence':0.99,'scope':'fixture only','evidence_refs':[ref],'status':'active'}
print(json.dumps({'observations':[proposal],'claims':[proposal],'learned_memory_updates':[proposal],'counterevidence':[proposal]}))
"#).unwrap();
        let fixture = Self {
            dir,
            home,
            database,
            config,
            marker,
            project,
            sessions,
        };
        fs::write(
            &fixture.config,
            serde_json::to_vec(&fixture.config_json(&script)).unwrap(),
        )
        .unwrap();
        fixture.poison_ambient_history();
        fixture
    }
    fn config_json(&self, script: &Path) -> Value {
        json!({"version":1,"enabled":true,"database_path":self.database,"project_id":self.project,"policy_version":"synthetic-cli-v1",
            "command":{"program":"/usr/bin/python3","args":[script,self.marker,SENTINEL],"version":"fake-v1"},
            "budgets":{"max_sessions":8,"max_events":2000,"max_evidence_bytes":2097152,"max_request_bytes":2097152,"max_output_bytes":262144,"max_stderr_bytes":65536,
                "execution_timeout_ms":2000,"cleanup_timeout_ms":300,"invocation_timeout_ms":8000,"poll_interval_ms":10,"lease_ms":4000,"max_claim_attempts":3}})
    }
    fn edit_config(&self, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&fs::read(&self.config).unwrap()).unwrap();
        edit(&mut value);
        fs::write(&self.config, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn poison_ambient_history(&self) {
        let root = self.home.join(".codex/sessions/2026/01/01");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("poison.jsonl"), [0xff, 0xfe]).unwrap();
        let config_root = self.home.join(".config/mmr");
        fs::create_dir_all(&config_root).unwrap();
        fs::write(config_root.join("config.json"), "not JSON").unwrap();
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
        command
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("MMR_DEFAULT_SOURCE", "codex")
            .env("MMR_DREAM_COMMAND", "must-never-be-selected")
            .env("MMR_DEFAULT_DREAM_RUNNER", "command")
            .env("MMR_DREAM_MOCK_OUTPUT", "not JSON")
            .env("OPENAI_API_KEY", "synthetic-ambient-key")
            .env("OPENAI_BASE_URL", "http://127.0.0.1:1/must-not-connect")
            .env("MORPHLLM_API_KEY", "synthetic-ambient-key")
            .env("MMR_CONFIG_FILE", self.home.join(".config/mmr/config.json"));
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn jobs(&self, args: &[&str]) -> Output {
        let mut argv = vec![
            "assimilate",
            "jobs",
            "--config",
            self.config.to_str().unwrap(),
        ];
        argv.extend_from_slice(args);
        self.run(&argv)
    }
    fn success(&self, args: &[&str]) -> Value {
        success(self.jobs(args))
    }
    fn launches(&self) -> Vec<Value> {
        fs::read_to_string(&self.marker)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn safe_failure(output: Output) {
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains(SENTINEL),
        "diagnostics exposed a synthetic private value"
    );
    assert!(!combined.contains("synthetic-ambient-key"));
}

#[test]
fn missing_and_disabled_config_short_circuit_before_store_attestation_or_ambient_launch() {
    let f = Fixture::new();
    for action in ["enqueue", "run"] {
        let value = success(f.run(&["assimilate", "jobs", action]));
        assert_eq!(value["status"], "disabled");
    }
    let absent = f.dir.path().join("must-not-create/store.sqlite3");
    f.edit_config(|config| {
        config["enabled"] = json!(false);
        config["database_path"] = json!(absent);
    });
    for args in [
        vec!["enqueue"],
        vec![
            "enqueue",
            "--session",
            "does-not-exist",
            "--attest-complete",
            "synthetic-attestation",
        ],
        vec!["run"],
    ] {
        assert_eq!(f.success(&args)["status"], "disabled");
    }
    assert!(!absent.exists());
    assert!(!f.home.join(".local/share/mmr").exists());
    assert!(f.launches().is_empty());
    // Negative control: the ordinary source reader does encounter this poisoned history.
    assert!(
        !f.run(&["--source", "codex", "read", "source"])
            .status
            .success()
    );
}

#[test]
fn jobs_reject_inapplicable_scope_flags_and_bound_drain_and_inspection_arguments() {
    let f = Fixture::new();
    for args in [
        vec!["--source", "codex", "assimilate", "jobs", "run"],
        vec!["assimilate", "jobs", "status", "--remote", "fixture"],
        vec!["assimilate", "jobs", "run", "--max-jobs", "0"],
        vec!["assimilate", "jobs", "run", "--max-jobs", "9"],
        vec!["assimilate", "jobs", "status", "--limit", "101"],
    ] {
        assert!(!f.run(&args).status.success());
    }
    for args in [
        vec!["status"],
        vec!["show", "missing"],
        vec!["cancel", "missing"],
    ] {
        let mut argv = vec!["assimilate", "jobs"];
        argv.extend(args);
        safe_failure(f.run(&argv));
    }
    assert!(f.launches().is_empty());
}

#[test]
fn explicit_config_validation_diagnostics_never_echo_unknown_keys_paths_or_arguments() {
    let f = Fixture::new();
    let original = fs::read(&f.config).unwrap();
    for case in ["unknown", "nested", "version", "budget", "program", "huge"] {
        fs::write(&f.config, &original).unwrap();
        f.edit_config(|config| match case {
            "unknown" => config[SENTINEL] = json!(SENTINEL),
            "nested" => config["command"][SENTINEL] = json!(SENTINEL),
            "version" => config["version"] = json!(77),
            "budget" => config["budgets"]["max_output_bytes"] = json!(0),
            "program" => config["command"]["program"] = json!(SENTINEL),
            _ => config["command"]["args"] = json!(["x".repeat(100_000)]),
        });
        safe_failure(f.jobs(&["enqueue", "--session", &f.sessions[0]]));
    }
    assert!(f.launches().is_empty());
}

#[test]
fn explicit_order_attestation_dedup_and_receipt_survive_separate_cli_restarts() {
    let f = Fixture::new();
    let ids = [&f.sessions[1], &f.sessions[0]];
    // Enqueue does not infer completion from ordinary events.
    safe_failure(f.jobs(&["enqueue", "--session", ids[0], "--session", ids[1]]));
    let enqueue = f.success(&[
        "enqueue",
        "--session",
        ids[0],
        "--session",
        ids[1],
        "--attest-complete",
        "trusted-cli-fixture",
    ]);
    assert_eq!(enqueue["command"], "enqueue");
    assert_eq!(enqueue["job"]["state"], "queued");
    let id = enqueue["job"]["id"].as_str().unwrap();
    let snapshot = enqueue["job"]["snapshot_id"].as_str().unwrap();
    let mut store = Store::open(&f.database).unwrap();
    let current = store
        .read_current_evidence_snapshot(snapshot, Default::default())
        .unwrap();
    assert_eq!(
        current
            .snapshot
            .metadata
            .sessions
            .iter()
            .map(|s| &s.metadata.id)
            .collect::<Vec<_>>(),
        ids
    );
    drop(store);
    assert_eq!(
        f.success(&["enqueue", "--session", ids[0], "--session", ids[1]])["job"]["id"],
        id
    );
    let run = f.success(&["run"]);
    assert_eq!(run["command"], "run");
    assert_eq!(run["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(run["jobs"][0]["state"], "proposed", "{run}");
    let shown = f.success(&["show", id]);
    let receipt = shown["receipt"]["id"].as_str().unwrap();
    assert_eq!(shown["job"]["receipt_id"], receipt);
    assert_eq!(shown["detail"]["summary"], shown["job"]);
    assert!(shown["detail"]["policy"]["runner_fingerprint"].is_string());
    assert_eq!(shown["detail"]["limits"]["max_events"], 2000);
    assert_eq!(
        shown["detail"]["projection"]["included_event_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        shown["detail"]["projection"]["omissions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let proposal: Value =
        serde_json::from_str(shown["receipt"]["proposal_json"].as_str().unwrap()).unwrap();
    for key in [
        "observations",
        "claims",
        "learned_memory_updates",
        "counterevidence",
    ] {
        assert_eq!(proposal[key][0]["status"], "pending");
    }
    assert_eq!(f.success(&["show", id]), shown);
    assert_eq!(
        f.success(&["enqueue", "--session", ids[0], "--session", ids[1]])["job"]["receipt_id"],
        receipt
    );
    let again = f.success(&["run"]);
    assert!(
        again
            .get("jobs")
            .is_none_or(|jobs| jobs.as_array().unwrap().is_empty())
    );
    assert_eq!(
        Connection::open(&f.database)
            .unwrap()
            .query_row("SELECT count(*) FROM learned_memory", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0,
        "proposal receipt must not activate learned memory"
    );
    let launches = f.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0]["request"]["evidence_access"], "shared_safe");
    assert!(launches[0]["env"].get("OPENAI_API_KEY").is_none());
    assert!(launches[0]["env"].get("MMR_DREAM_COMMAND").is_none());
    let status = f.success(&["status"]);
    assert_eq!(status["jobs"].as_array().unwrap().len(), 1);
    assert!(status.get("receipt").is_none());
    assert!(status.get("detail").is_none());
    assert!(
        !status
            .to_string()
            .contains("Synthetic proposal pending review")
    );
    assert!(!status.to_string().contains(SENTINEL));
    assert!(!String::from_utf8_lossy(&fs::read(&f.database).unwrap()).contains(SENTINEL));
}

#[test]
fn disabled_execution_keeps_inspection_and_sticky_queued_cancellation_available() {
    let f = Fixture::new();
    let enqueued = f.success(&[
        "enqueue",
        "--session",
        &f.sessions[0],
        "--attest-complete",
        "trusted-cli-fixture",
    ]);
    let id = enqueued["job"]["id"].as_str().unwrap();
    f.edit_config(|config| config["enabled"] = json!(false));
    let shown = f.success(&["show", id]);
    assert_eq!(shown["job"]["id"], id);
    assert_eq!(f.success(&["status"])["jobs"][0]["state"], "queued");
    let cancelled = f.success(&["cancel", id]);
    assert_eq!(cancelled["job"]["state"], "cancelled");
    assert_eq!(cancelled["job"]["cancel_requested"], true);
    assert_eq!(f.success(&["run"])["status"], "disabled");
    assert_eq!(f.success(&["cancel", id])["job"]["state"], "cancelled");
    f.edit_config(|config| config["enabled"] = json!(true));
    assert_eq!(
        f.success(&["enqueue", "--session", &f.sessions[0]])["job"]["state"],
        "cancelled"
    );
    f.success(&["run"]);
    assert!(f.launches().is_empty());
}

#[test]
fn run_defaults_to_one_and_explicit_batch_is_finite_and_status_is_bounded() {
    let f = Fixture::new();
    for id in &f.sessions {
        f.success(&[
            "enqueue",
            "--session",
            id,
            "--attest-complete",
            "trusted-cli-fixture",
        ]);
    }
    assert_eq!(
        f.success(&["status", "--limit", "1"])["jobs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.success(&["run"])["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(f.launches().len(), 1);
    assert_eq!(
        f.success(&["run", "--max-jobs", "2"])["jobs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(f.launches().len(), 3);
    assert!(
        f.success(&["status"])["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|job| job["state"] == "proposed")
    );
    // Jobs-local global config works after the action, and JSON pretty output stays parseable.
    let pretty = success(f.run(&[
        "assimilate",
        "jobs",
        "status",
        "--config",
        f.config.to_str().unwrap(),
        "--pretty",
    ]));
    assert_eq!(pretty["jobs"].as_array().unwrap().len(), 3);
}

#[test]
fn bounded_runner_failure_is_terminal_and_never_exposes_command_diagnostics() {
    let f = Fixture::new();
    fs::write(
        f.dir.path().join("fake-command.py"),
        format!(
            r#"import json,sys
json.load(sys.stdin)
with open(sys.argv[1], 'a') as f: f.write('{{}}\n')
sys.stderr.write('{SENTINEL}')
sys.stderr.flush()
sys.stdout.write('x'*300000)
sys.stdout.flush()
"#
        ),
    )
    .unwrap();
    f.edit_config(|config| config["budgets"]["max_output_bytes"] = json!(1024));
    let enqueued = f.success(&[
        "enqueue",
        "--session",
        &f.sessions[0],
        "--attest-complete",
        "trusted-cli-fixture",
    ]);
    let id = enqueued["job"]["id"].as_str().unwrap();
    let run = f.success(&["run"]);
    assert_eq!(run["jobs"][0]["state"], "failed");
    assert!(run["jobs"][0]["diagnostic"].is_string());
    assert!(!run.to_string().contains(SENTINEL));
    let shown = f.success(&["show", id]);
    assert!(shown.get("receipt").is_none());
    assert!(!shown.to_string().contains(SENTINEL));
    assert_eq!(
        f.success(&["enqueue", "--session", &f.sessions[0]])["job"]["state"],
        "failed"
    );
    f.success(&["run"]);
    assert_eq!(f.launches().len(), 1);
}

#[test]
fn inspection_and_cancellation_are_scoped_to_the_explicit_project() {
    let f = Fixture::new();
    let enqueued = f.success(&[
        "enqueue",
        "--session",
        &f.sessions[0],
        "--attest-complete",
        "trusted-cli-fixture",
    ]);
    let id = enqueued["job"]["id"].as_str().unwrap();
    let other_path = f.dir.path().join("other-project");
    fs::create_dir(&other_path).unwrap();
    let other = Store::open(&f.database)
        .unwrap()
        .ensure_project_link(&other_path)
        .unwrap()
        .id;
    f.edit_config(|config| {
        config["project_id"] = json!(other);
        config["enabled"] = json!(false);
    });
    let status = f.success(&["status"]);
    assert!(
        status
            .get("jobs")
            .is_none_or(|jobs| jobs.as_array().unwrap().is_empty())
    );
    safe_failure(f.jobs(&["show", id]));
    safe_failure(f.jobs(&["cancel", id]));
    f.edit_config(|config| config["project_id"] = json!(f.project));
    assert_eq!(f.success(&["show", id])["job"]["state"], "queued");
    assert!(f.launches().is_empty());
}
