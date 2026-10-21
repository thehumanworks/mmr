//! Explicit-config/synthetic subprocess tests; no provider roots or network.
use mmr::assimilation_jobs::{JobConfig, JobWorkerError};
use mmr::assimilation_jobs::{JobsAction, execute};
use mmr::store::jobs::{JobDiagnostic, JobState};
use mmr::store::{NewEvent, Store};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Fixture {
    dir: tempfile::TempDir,
    db: PathBuf,
    config: PathBuf,
    project: String,
}
impl Fixture {
    fn new(script: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("store.sqlite3");
        let store = Store::open(&db).unwrap();
        let project = store.ensure_project_link(dir.path()).unwrap().id;
        let program = dir.path().join("synthetic.py");
        std::fs::write(&program, script).unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(&config, serde_json::to_vec(&json!({
            "version":1,"enabled":true,"database_path":db,"project_id":project,
            "policy_version":"synthetic-v1", "command":{"program":"/usr/bin/python3","args":[program],"version":"synthetic-v1"},
            "budgets":{"execution_timeout_ms":2000,"cleanup_timeout_ms":500,
                "invocation_timeout_ms":12000,"lease_ms":4000,"poll_interval_ms":10}
        })).unwrap()).unwrap();
        Self {
            dir,
            db,
            config,
            project,
        }
    }
    fn add(&self, session: &str, kind: &str, role: &str, text: &str) -> mmr::store::EventRecord {
        Store::open_job_store(&self.db)
            .unwrap()
            .insert_event(
                &self.project,
                &NewEvent::new(
                    "codex",
                    session,
                    kind,
                    role,
                    "2026-01-01T00:00:00Z",
                    text,
                    "synthetic-v1",
                ),
            )
            .unwrap()
    }
    fn enqueue(&self, sessions: Vec<String>) -> mmr::store::jobs::JobSummary {
        execute(
            Some(&self.config),
            JobsAction::Enqueue {
                session_ids: sessions,
                attest_complete: Some("trusted-synthetic-complete".into()),
            },
        )
        .unwrap()
        .job
        .unwrap()
    }
    fn run(&self, max_jobs: u8) -> mmr::assimilation_jobs::JobsResponse {
        execute(Some(&self.config), JobsAction::Run { max_jobs }).unwrap()
    }
    fn budget(&self, name: &str, value: u64) {
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(&self.config).unwrap()).unwrap();
        config["budgets"][name] = json!(value);
        std::fs::write(&self.config, serde_json::to_vec(&config).unwrap()).unwrap();
    }
}
const SUCCESS: &str = "import json,sys\nr=json.load(sys.stdin)\njson.dump({'observations':[{'kind':'pattern','claim':'Synthetic proposal','confidence':0.99,'status':'active','evidence_refs':[r['evidence'][0]['evidence_ref']]}]},sys.stdout)\n";

fn wait_marker(path: &std::path::Path) -> String {
    let start = Instant::now();
    loop {
        if let Ok(body) = std::fs::read_to_string(path) {
            if !body.is_empty() {
                return body;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "synthetic fixture did not start"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(unix)]
#[test]
fn job_runner_clean_environment_is_explicit_and_preserves_legacy_default() {
    use mmr::dream::{
        CommandDreamRunner, DreamEvidence, DreamRequest, DreamRunner, EvidenceAccess,
    };
    let request = DreamRequest {
        project_id: "synthetic".into(),
        provider: "command".into(),
        model: None,
        evidence_access: EvidenceAccess::SharedSafe,
        evidence_hash: "synthetic".into(),
        evidence: vec![DreamEvidence {
            evidence_ref: "mmr://event/synthetic".into(),
            project_id: "synthetic".into(),
            source: "fixture".into(),
            role: "user".into(),
            event_type: "user_turn".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            content_text: "Synthetic only".into(),
        }],
    };
    let runner = CommandDreamRunner::new(
        "/bin/sh",
        [
            "-c",
            r#"cat >/dev/null; [ -z "${HOME+x}" ] && [ "$PATH" = /usr/bin:/bin ] && printf '%s' '{"observations":[{"kind":"pattern","claim":"Synthetic only","confidence":0.5,"evidence_refs":["mmr://event/synthetic"]}]}'"#,
        ],
    );
    if std::env::var_os("HOME").is_some() {
        assert!(
            runner.run(&request).is_err(),
            "inherited HOME positive control"
        );
    }
    runner
        .with_clean_environment()
        .run(&request)
        .expect("clean environment synthetic output");
}

#[test]
fn job_config_missing_and_empty_default_disabled() {
    assert!(!JobConfig::load(None).expect("missing disabled").enabled);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, "{}").unwrap();
    assert!(!JobConfig::load(Some(&path)).unwrap().enabled);
}

#[test]
fn job_config_bounds_and_errors_do_not_echo_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    for body in [
        r#"{"private-sentinel":"unsafe-value"}"#.to_string(),
        r#"{"version":999}"#.to_string(),
        r#"{"enabled":true,"command":{"program":"private-sentinel","version":"v1","args":[]}}"#
            .to_string(),
        " ".repeat(32 * 1024 + 1),
    ] {
        std::fs::write(&path, body).unwrap();
        let err = JobConfig::load(Some(&path)).expect_err("invalid config");
        assert!(!format!("{err:?} {err}").contains("sentinel"));
        assert!(matches!(err, JobWorkerError::InvalidConfig));
    }
}

#[test]
fn job_disabled_execution_does_not_create_store_or_resolve_runner() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    let db = dir.path().join("must-not-exist.sqlite");
    std::fs::write(
        &config,
        serde_json::to_vec(&json!({"enabled":false,"database_path":db,"project_id":"synthetic"}))
            .unwrap(),
    )
    .unwrap();
    for action in [
        JobsAction::Enqueue {
            session_ids: vec![],
            attest_complete: None,
        },
        JobsAction::Run { max_jobs: 0 },
    ] {
        assert_eq!(execute(Some(&config), action).unwrap().status, "disabled");
    }
    assert!(!db.exists());
    assert!(execute(Some(&config), JobsAction::Status { limit: 20 }).is_err());
    assert!(!db.exists());
}

#[test]
fn job_projection_receipt_dedup_and_restart_are_pending_only() {
    let f = Fixture::new(&format!("import pathlib\n{SUCCESS}"));
    let safe = f.add(
        "selected",
        "user_turn",
        "user",
        "Synthetic narrative with person@example.com.",
    );
    f.add("selected", "tool_result", "tool", "private-tool-sentinel");
    f.add(
        "selected",
        "user_turn",
        "user",
        "api_key=sk-synthetic00000000000000000000000000000000000000",
    );
    f.add(
        "selected",
        "user_turn",
        "person@example.com",
        "private-role-sentinel",
    );
    f.add(
        "unselected",
        "user_turn",
        "user",
        "private-unselected-sentinel",
    );
    let capture = f.dir.path().join("request.json");
    let program = f.dir.path().join("synthetic.py");
    std::fs::write(&program,format!("import json,sys,pathlib\nr=json.load(sys.stdin)\npathlib.Path({}).write_text(json.dumps(r))\njson.dump({{'observations':[{{'kind':'pattern','claim':'Synthetic proposal','confidence':0.99,'status':'active','evidence_refs':[r['evidence'][0]['evidence_ref']]}}]}},sys.stdout)",serde_json::to_string(capture.to_str().unwrap()).unwrap())).unwrap();
    let queued = f.enqueue(vec![safe.session_id.clone()]);
    let response = f.run(1);
    assert_eq!(response.jobs.len(), 1);
    let job = &response.jobs[0];
    assert_eq!(job.state, JobState::Proposed);
    assert!(job.may_have_executed && job.cleanup_confirmed);
    let outbound = std::fs::read_to_string(&capture).unwrap();
    for excluded in [
        "person@example.com",
        "private-tool-sentinel",
        "private-role-sentinel",
        "private-unselected-sentinel",
        "sk-synthetic",
    ] {
        assert!(!outbound.contains(excluded));
    }
    let request: Value = serde_json::from_str(&outbound).unwrap();
    assert_eq!(request["evidence"].as_array().unwrap().len(), 1);
    let show = execute(
        Some(&f.config),
        JobsAction::Show {
            job_id: queued.id.clone(),
        },
    )
    .unwrap();
    let receipt = show.receipt.unwrap();
    let proposal: Value = serde_json::from_str(&receipt.proposal_json).unwrap();
    assert_eq!(proposal["observations"][0]["status"], "pending");
    let record = Store::open_job_store(&f.db)
        .unwrap()
        .job(&f.project, &queued.id)
        .unwrap();
    assert_eq!(record.projection.unwrap().omissions.len(), 3);
    assert_eq!(f.enqueue(vec![safe.session_id]).id, queued.id);
    assert!(f.run(1).jobs.is_empty());
    assert_eq!(
        rusqlite::Connection::open(&f.db)
            .unwrap()
            .query_row("SELECT count(*) FROM learned_memory", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[cfg(unix)]
#[test]
fn job_durable_cancel_joins_nonreading_stdin_child() {
    let f = Fixture::new(
        "import os,pathlib,time\npathlib.Path(__file__).with_suffix('.pid').write_text(str(os.getpid()))\ntime.sleep(30)\n",
    );
    let safe = f.add("blocked", "user_turn", "user", &"q".repeat(512 * 1024));
    let queued = f.enqueue(vec![safe.session_id]);
    let config = f.config.clone();
    let handle = std::thread::spawn(move || {
        execute(Some(&config), JobsAction::Run { max_jobs: 1 }).unwrap()
    });
    let pid = wait_marker(&f.dir.path().join("synthetic.pid"));
    let started = Instant::now();
    let requested = execute(
        Some(&f.config),
        JobsAction::Cancel {
            job_id: queued.id.clone(),
        },
    )
    .unwrap()
    .job
    .unwrap();
    assert!(requested.cancel_requested);
    let completed = handle.join().unwrap();
    assert_eq!(completed.jobs[0].state, JobState::Cancelled);
    assert!(completed.jobs[0].cleanup_confirmed);
    assert!(started.elapsed() < Duration::from_secs(2));
    let status = std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .unwrap();
    assert!(
        !status.status.success(),
        "owned child remains alive after acknowledgement"
    );
    assert!(f.run(1).jobs.is_empty());
}

#[test]
fn job_known_execution_failures_are_bounded_and_never_retried() {
    for (script, diagnostic, output_cap) in [
        (
            "import sys;sys.stdin.read();print('private-output-sentinel')",
            JobDiagnostic::InvalidOutput,
            None,
        ),
        (
            "import sys;sys.stdin.read();print('q'*4096)",
            JobDiagnostic::BudgetExceeded,
            Some(128),
        ),
        (
            "import time;time.sleep(30)",
            JobDiagnostic::RunnerTimedOut,
            None,
        ),
    ] {
        let f = Fixture::new(script);
        if let Some(cap) = output_cap {
            f.budget("max_output_bytes", cap);
        }
        if diagnostic == JobDiagnostic::RunnerTimedOut {
            f.budget("execution_timeout_ms", 200);
        }
        let safe = f.add("failed", "user_turn", "user", "Synthetic narrative");
        f.enqueue(vec![safe.session_id]);
        let result = f.run(1);
        assert_eq!(result.jobs[0].state, JobState::Failed);
        assert_eq!(result.jobs[0].diagnostic, Some(diagnostic));
        assert!(result.jobs[0].may_have_executed && result.jobs[0].cleanup_confirmed);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("private-output-sentinel")
        );
        assert!(f.run(1).jobs.is_empty());
    }
}

#[test]
fn job_invocation_deadline_and_projection_budget_prevent_launch() {
    for (budget, value) in [("invocation_timeout_ms", 100), ("max_request_bytes", 128)] {
        let f = Fixture::new(SUCCESS);
        f.budget(budget, value);
        let safe = f.add("budget", "user_turn", "user", &"q".repeat(4096));
        let queued = f.enqueue(vec![safe.session_id]);
        let result = f.run(1);
        let actual = Store::open_job_store(&f.db)
            .unwrap()
            .job(&f.project, &queued.id)
            .unwrap()
            .summary;
        assert!(!actual.may_have_executed);
        if budget == "invocation_timeout_ms" {
            assert_eq!(result.status, "budget_exhausted");
            assert_eq!(actual.state, JobState::Queued);
            assert_eq!(actual.attempts, 0);
        } else {
            assert_eq!(actual.state, JobState::Failed);
            assert_eq!(actual.diagnostic, Some(JobDiagnostic::ProjectionFailed));
        }
    }
}

#[test]
fn job_finite_drain_leaves_excess_work_queued() {
    let f = Fixture::new(SUCCESS);
    for session in ["a", "b", "c"] {
        let event = f.add(
            session,
            "user_turn",
            "user",
            "Synthetic independent narrative",
        );
        f.enqueue(vec![event.session_id]);
    }
    let first = f.run(2);
    assert_eq!(first.jobs.len(), 2);
    assert!(first.jobs.iter().all(|j| j.state == JobState::Proposed));
    assert_eq!(f.run(8).jobs.len(), 1);
    assert!(f.run(1).jobs.is_empty());
}
