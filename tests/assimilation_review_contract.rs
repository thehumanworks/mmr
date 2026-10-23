use mmr::store::{NewEvent, Store};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    project_id: String,
    refs: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = home.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut store = Store::open(home.join("data/mmr/mmr.db")).unwrap();
        let linked = store.ensure_project_link(&project).unwrap();
        let refs = [
            "Retry cap is three after a transient failure.",
            "A second incident confirms the retry cap is three.",
            "Choose wording with the audience in mind.",
        ]
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let event = store
                .insert_event(
                    &linked.id,
                    &NewEvent::new(
                        "codex",
                        "synthetic",
                        "user_turn",
                        "user",
                        "2026-01-01T00:00:00Z",
                        text,
                        "fixture-v1",
                    )
                    .with_source_event_id(format!("example-{i}")),
                )
                .unwrap();
            format!("mmr://event/{}", event.id)
        })
        .collect();
        Self {
            _temp: temp,
            home,
            project,
            project_id: linked.id,
            refs,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mmr"))
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join("data"))
            .current_dir(&self.project)
            .args(args)
            .output()
            .unwrap()
    }

    fn plan(&self) -> Value {
        json!({"observations": [
            {"kind":"workflow", "claim":"Cap retries at three.", "confidence":0.95, "scope":self.project_id, "evidence_refs":self.refs[..2],
             "codification":{"state":"proposed", "target":"tests/retry.rs", "expected_behavior":"Reject a fourth retry.", "validation":"touch SHOULD_NOT_EXIST; exit 0"}},
            {"kind":"judgment", "claim":"Adapt wording to the audience.", "confidence":0.85, "scope":self.project_id, "evidence_refs":[self.refs[2]], "recommended_action":"Audience needs require contextual judgement, not a blanket rule."}
        ]})
    }

    fn review(&self, plan: &Value) -> Output {
        let path = self.home.join("proposal.json");
        std::fs::write(&path, serde_json::to_vec(plan).unwrap()).unwrap();
        self.run(&["assimilate", "project", "--review", path.to_str().unwrap()])
    }

    fn reject(&self, plan: &Value, reason: &str) {
        let output = self.review(plan);
        assert!(!output.status.success(), "unexpected acceptance: {plan}");
        assert!(output.stdout.is_empty(), "no partial accepted output");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn review_preserves_lessons_but_never_runs_or_installs_proposals() {
    let f = Fixture::new();
    let normal = f.run(&["assimilate", "project"]);
    assert!(normal.status.success());
    let normal: Value = serde_json::from_slice(&normal.stdout).unwrap();
    assert!(normal.get("reviewed_output").is_none());
    assert!(
        normal["runbook"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["step"] == "codify-or-retain")
    );
    let plan = f.plan();
    let output = f.review(&plan);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    let observations = &output["reviewed_output"]["observations"];
    assert_eq!(observations[0]["claim"], plan["observations"][0]["claim"]);
    assert_eq!(
        observations[0]["evidence_refs"],
        plan["observations"][0]["evidence_refs"]
    );
    assert_eq!(observations[0]["status"], "active");
    assert_eq!(observations[0]["codification"]["state"], "proposed");
    assert!(observations[1].get("codification").is_none());
    assert_eq!(
        observations[1]["recommended_action"],
        plan["observations"][1]["recommended_action"]
    );
    assert!(!f.project.join("SHOULD_NOT_EXIST").exists());
    assert!(!f.project.join("tests/retry.rs").exists());
    let store = Store::open(f.home.join("data/mmr/mmr.db")).unwrap();
    assert!(
        store
            .learned_memory_for_project(&f.project_id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .events_for_project(&f.project_id, None, None)
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn review_rejects_false_attestation_invalid_scope_and_targets() {
    let f = Fixture::new();
    for state in ["verified", "installed"] {
        let mut plan = f.plan();
        plan["observations"][0]["codification"]["state"] = json!(state);
        f.reject(&plan, "unknown variant");
    }
    for target in [
        "/tmp/check",
        "../check",
        "checks/../escape",
        "C:\\check",
        "./check",
    ] {
        let mut plan = f.plan();
        plan["observations"][0]["codification"]["target"] = json!(target);
        f.reject(&plan, "project-relative");
    }
    let mut plan = f.plan();
    plan["observations"][0]["scope"] = json!("all-projects");
    f.reject(&plan, "scope must match");
    plan = f.plan();
    plan["observations"][0]["evidence_refs"] = json!([f.refs[0], f.refs[0]]);
    f.reject(&plan, "two distinct");
    plan = f.plan();
    plan["observations"][0]["codification"]["expected_behavior"] = json!("");
    f.reject(&plan, "expected_behavior");
    plan = f.plan();
    plan["observations"][1]["recommended_action"] = json!("");
    f.reject(&plan, "recommended_action");
}

#[test]
fn review_rejects_unknown_or_omitted_evidence_and_unbounded_input() {
    let f = Fixture::new();
    let mut plan = f.plan();
    plan["observations"][0]["evidence_refs"][1] = json!("mmr://event/not-in-bundle");
    f.reject(&plan, "missing evidence");
    let mut store = Store::open(f.home.join("data/mmr/mmr.db")).unwrap();
    let secret = store
        .insert_event(
            &f.project_id,
            &NewEvent::new(
                "codex",
                "synthetic",
                "user_turn",
                "user",
                "2026-01-01T00:00:00Z",
                "api_key=sk-test1234567890abcdefghijklmnopqrstuvwxyz",
                "fixture-v1",
            ),
        )
        .unwrap();
    drop(store);
    plan = f.plan();
    plan["observations"][0]["evidence_refs"][1] = json!(format!("mmr://event/{}", secret.id));
    f.reject(&plan, "missing evidence");
    plan = f.plan();
    plan["observations"] = json!(vec![plan["observations"][1].clone(); 17]);
    f.reject(&plan, "at most 16");
    plan = f.plan();
    plan["observations"][0]["claim"] = json!("x".repeat(2001));
    f.reject(&plan, "review claim");
    plan["observations"][0]["claim"] = json!("x".repeat(65536));
    f.reject(&plan, "exceeds 65536");
}

#[test]
fn source_review_keeps_project_boundaries_and_retirement_is_only_a_recommendation() {
    let f = Fixture::new();
    let mut plan = f.plan();
    plan["observations"][1]["kind"] = json!("superseded");
    f.reject(&plan, "replacement counterevidence");
    plan["observations"][1]["counterevidence_refs"] = json!([f.refs[0]]);
    let output = f.review(&plan);
    assert!(output.status.success());
    let path = f.home.join("proposal.json");
    let output = f.run(&[
        "--source",
        "codex",
        "assimilate",
        "source",
        "--review",
        path.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        output["reviewed_output"]["observations"][1]["kind"],
        "superseded"
    );
    let limited = f.run(&[
        "--source",
        "codex",
        "assimilate",
        "source",
        "--per-project-limit",
        "1",
        "--review",
        path.to_str().unwrap(),
    ]);
    assert!(!limited.status.success());
    assert!(String::from_utf8_lossy(&limited.stderr).contains("missing evidence"));
    // A valid ref from a second project is still invalid for this candidate's scope.
    let other_path = f.home.join("other");
    std::fs::create_dir(&other_path).unwrap();
    let mut store = Store::open(f.home.join("data/mmr/mmr.db")).unwrap();
    let other = store.ensure_project_link(&other_path).unwrap();
    let event = store
        .insert_event(
            &other.id,
            &NewEvent::new(
                "codex",
                "other",
                "user_turn",
                "user",
                "2026-01-01T00:00:00Z",
                "Same lesson in another project.",
                "fixture-v1",
            ),
        )
        .unwrap();
    drop(store);
    plan["observations"][0]["evidence_refs"][1] = json!(format!("mmr://event/{}", event.id));
    std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let output = f.run(&[
        "--source",
        "codex",
        "assimilate",
        "source",
        "--review",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("scope must match"));
}
