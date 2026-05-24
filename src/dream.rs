use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::redaction::{DeterministicPrivacyDetector, PiiCoverageStatus, scan_text_with_detector};
use crate::store::{EventRecord, ProjectRecord, Store, content_hash};

pub const ENV_DEFAULT_DREAM_RUNNER: &str = "MMR_DEFAULT_DREAM_RUNNER";
pub const ENV_DREAM_COMMAND: &str = "MMR_DREAM_COMMAND";
pub const DEFAULT_DREAM_RUNNER: &str = "mock";
const ACTIVE_CONFIDENCE_MIN: f64 = 0.5;
const COMMAND_RUNNER_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamRunnerKind {
    Mock,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAccess {
    SharedSafe,
    LocalRaw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamEvidenceMode {
    SharedSafe,
    LocalRaw,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DreamConfigOverride {
    pub runner: Option<String>,
    pub model: Option<String>,
    pub evidence_mode: Option<DreamEvidenceMode>,
    pub allow_raw_evidence: bool,
    pub best_of: Option<usize>,
    pub retries: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamRunnerConfig {
    pub runner: String,
    pub model: Option<String>,
    pub evidence_access: EvidenceAccess,
    pub allow_raw_evidence: bool,
    pub best_of: usize,
    pub retries: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamRequest {
    pub project_id: String,
    pub provider: String,
    pub model: Option<String>,
    pub evidence_access: EvidenceAccess,
    pub evidence_hash: String,
    pub evidence: Vec<DreamEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamEvidence {
    pub evidence_ref: String,
    pub source: String,
    pub role: String,
    pub event_type: String,
    pub timestamp: String,
    pub content_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DreamEvidenceBundle {
    pub events: Vec<DreamEvidence>,
    pub omitted_events: Vec<OmittedDreamEvidence>,
    pub evidence_hash: String,
    pub pii_coverage: PiiCoverageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OmittedDreamEvidence {
    pub evidence_ref: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamRunnerOutput {
    pub observations: Vec<DreamObservation>,
    #[serde(default)]
    pub claims: Vec<DreamObservation>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub open_loops: Vec<String>,
    #[serde(default)]
    pub learned_memory_updates: Vec<DreamObservation>,
    #[serde(default)]
    pub counterevidence: Vec<DreamObservation>,
    #[serde(default)]
    pub recommended_actions: Vec<String>,
    #[serde(default)]
    pub diagnostics: DreamDiagnostics,
    #[serde(default)]
    pub usage: Option<DreamUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamObservation {
    pub kind: String,
    pub claim: String,
    pub confidence: f64,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub recommended_action: Option<String>,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub counterevidence_refs: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub open_loops: Vec<String>,
    #[serde(default)]
    pub status: DreamObservationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DreamObservationStatus {
    Active,
    #[default]
    Pending,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DreamDiagnostics {
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatedDreamOutput {
    pub learned_memory: Vec<ValidatedLearnedMemory>,
    pub observations: Vec<DreamObservation>,
    pub diagnostics: DreamDiagnostics,
    pub usage: Option<DreamUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatedLearnedMemory {
    pub kind: String,
    pub claim: String,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub status: ValidatedLearnedMemoryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatedLearnedMemoryStatus {
    Active,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DreamRunnerResolution {
    pub runner: String,
    pub source: String,
}

pub trait DreamRunner {
    fn run(&self, request: &DreamRequest) -> Result<DreamRunnerOutput>;
}

#[derive(Debug, Clone)]
pub struct MockDreamRunner {
    output: Result<String, String>,
}

#[derive(Debug, Clone)]
pub struct CommandDreamRunner {
    program: String,
    args: Vec<String>,
}

impl DreamRunnerConfig {
    pub fn resolve_runner(
        cli_runner: Option<&str>,
        project_runner: Option<&str>,
        user_runner: Option<&str>,
    ) -> DreamRunnerResolution {
        if let Some(runner) = non_empty(cli_runner) {
            return DreamRunnerResolution {
                runner: runner.to_string(),
                source: "cli".to_string(),
            };
        }
        if let Some(runner) = non_empty(project_runner) {
            return DreamRunnerResolution {
                runner: runner.to_string(),
                source: "project".to_string(),
            };
        }
        if let Some(runner) = non_empty(user_runner) {
            return DreamRunnerResolution {
                runner: runner.to_string(),
                source: "user".to_string(),
            };
        }
        DreamRunnerResolution {
            runner: DEFAULT_DREAM_RUNNER.to_string(),
            source: "default".to_string(),
        }
    }

    pub fn resolve(
        overrides: DreamConfigOverride,
        project_runner: Option<&str>,
        user_runner: Option<&str>,
    ) -> Self {
        let resolution =
            Self::resolve_runner(overrides.runner.as_deref(), project_runner, user_runner);
        Self {
            runner: resolution.runner,
            model: overrides.model,
            evidence_access: EvidenceAccess::from(
                overrides
                    .evidence_mode
                    .unwrap_or(DreamEvidenceMode::SharedSafe),
            ),
            allow_raw_evidence: overrides.allow_raw_evidence,
            best_of: overrides.best_of.unwrap_or(1),
            retries: overrides.retries.unwrap_or(0),
        }
    }

    pub fn resolve_from_env(cli_runner: Option<&str>) -> Self {
        let user_runner = std::env::var(ENV_DEFAULT_DREAM_RUNNER).ok();
        Self::resolve(
            DreamConfigOverride {
                runner: cli_runner.map(str::to_string),
                ..DreamConfigOverride::default()
            },
            None,
            user_runner.as_deref(),
        )
    }

    pub fn runner_kind(&self) -> Result<DreamRunnerKind> {
        match self.runner.as_str() {
            "mock" => Ok(DreamRunnerKind::Mock),
            "command" => Ok(DreamRunnerKind::Command),
            other => bail!("unsupported dream runner: {other}"),
        }
    }

    pub fn validate_privacy_boundary(&self) -> Result<()> {
        if self.best_of != 1 {
            bail!("dream best-of execution is reserved and not implemented yet");
        }
        if self.retries != 0 {
            bail!("dream retry execution is reserved and not implemented yet");
        }
        if self.evidence_access == EvidenceAccess::LocalRaw && !self.allow_raw_evidence {
            bail!("raw dream evidence requires explicit local-only opt-in");
        }
        if self.runner_kind()? != DreamRunnerKind::Mock
            && self.evidence_access == EvidenceAccess::LocalRaw
        {
            bail!("remote/API dream runners require shared-safe evidence by default");
        }
        Ok(())
    }
}

impl From<DreamEvidenceMode> for EvidenceAccess {
    fn from(value: DreamEvidenceMode) -> Self {
        match value {
            DreamEvidenceMode::SharedSafe => EvidenceAccess::SharedSafe,
            DreamEvidenceMode::LocalRaw => EvidenceAccess::LocalRaw,
        }
    }
}

impl MockDreamRunner {
    pub fn new(json: impl Into<String>) -> Self {
        Self::returning_json(json)
    }

    pub fn returning_json(json: impl Into<String>) -> Self {
        Self {
            output: Ok(json.into()),
        }
    }

    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            output: Err(message.into()),
        }
    }
}

impl DreamRunner for MockDreamRunner {
    fn run(&self, request: &DreamRequest) -> Result<DreamRunnerOutput> {
        let output = self
            .output
            .as_ref()
            .map_err(|message| anyhow::anyhow!("mock dream runner failed: {message}"))?;
        parse_runner_output(output, request.evidence_refs())
    }
}

impl CommandDreamRunner {
    pub fn from_env() -> Result<Self> {
        let command = std::env::var(ENV_DREAM_COMMAND)
            .ok()
            .and_then(|value| non_empty(Some(&value)).map(str::to_string))
            .ok_or_else(|| anyhow::anyhow!("{ENV_DREAM_COMMAND} is not set"))?;
        let (program, args) = parse_command_parts(&command)?;
        Ok(Self { program, args })
    }
}

impl DreamRunner for CommandDreamRunner {
    fn run(&self, request: &DreamRequest) -> Result<DreamRunnerOutput> {
        if request.evidence_access != EvidenceAccess::SharedSafe {
            bail!("command dream runner requires shared-safe evidence");
        }
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn dream command {}", self.display_command()))?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow::anyhow!("dream command stdin unavailable"))?;
            serde_json::to_writer(&mut stdin, request).context("write dream request")?;
            stdin.write_all(b"\n").context("terminate dream request")?;
        }
        let status = wait_for_child(&mut child, COMMAND_RUNNER_TIMEOUT)?;
        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .context("read dream command stdout")?;
        }
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)
                .context("read dream command stderr")?;
        }
        if !status.success() {
            bail!("dream command failed: {}", stderr.trim());
        }
        parse_runner_output(&stdout, request.evidence_refs())
    }
}

impl CommandDreamRunner {
    fn display_command(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn parse_command_parts(command: &str) -> Result<(String, Vec<String>)> {
    let parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let Some((program, args)) = parts.split_first() else {
        bail!("{ENV_DREAM_COMMAND} is empty");
    };
    Ok((program.clone(), args.to_vec()))
}

fn wait_for_child(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("poll dream command")? {
            return Ok(status);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "dream command timed out after {} seconds",
                timeout.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn build_evidence_request(
    store: &Store,
    project: &ProjectRecord,
    config: &DreamRunnerConfig,
) -> Result<DreamRequest> {
    config.validate_privacy_boundary()?;
    let mode = match config.evidence_access {
        EvidenceAccess::SharedSafe => DreamEvidenceMode::SharedSafe,
        EvidenceAccess::LocalRaw => DreamEvidenceMode::LocalRaw,
    };
    let bundle = build_evidence_bundle(store, project, mode)?;
    Ok(DreamRequest {
        project_id: project.id.clone(),
        provider: config.runner.clone(),
        model: config.model.clone(),
        evidence_access: config.evidence_access.clone(),
        evidence_hash: bundle.evidence_hash,
        evidence: bundle.events,
    })
}

pub fn build_evidence_bundle(
    store: &Store,
    project: &ProjectRecord,
    mode: DreamEvidenceMode,
) -> Result<DreamEvidenceBundle> {
    let access = EvidenceAccess::from(mode);
    let events = store.events_for_project(&project.id, None, None)?;
    let mut evidence = Vec::with_capacity(events.len());
    let mut omitted_events = Vec::new();
    let mut pii_coverage = PiiCoverageStatus::Available;
    for event in events {
        match event_to_evidence(&event, &access)? {
            EvidenceProjection::Included(item, coverage) => {
                pii_coverage = coverage;
                evidence.push(item);
            }
            EvidenceProjection::Omitted(omitted, coverage) => {
                pii_coverage = coverage;
                omitted_events.push(omitted);
            }
        }
    }
    let evidence_hash = evidence_hash(&evidence);
    Ok(DreamEvidenceBundle {
        events: evidence,
        omitted_events,
        evidence_hash,
        pii_coverage,
    })
}

pub fn parse_dream_output_json(json: &str) -> Result<DreamRunnerOutput> {
    let value: Value = serde_json::from_str(json).context("parse dream runner JSON")?;
    reject_unknown_top_level_fields(&value)?;
    serde_json::from_value(value).context("parse dream runner schema")
}

pub fn parse_runner_output(
    json: &str,
    valid_evidence_refs: BTreeSet<String>,
) -> Result<DreamRunnerOutput> {
    let mut output = parse_dream_output_json(json)?;
    validate_output(&mut output, &valid_evidence_refs)?;
    Ok(output)
}

pub fn validate_dream_output(
    valid_evidence_refs: &BTreeSet<String>,
    mut output: DreamRunnerOutput,
) -> Result<ValidatedDreamOutput> {
    validate_output(&mut output, valid_evidence_refs)
        .map_err(|err| anyhow::anyhow!("resolve dream evidence ref failed: {err}"))?;
    let learned_source = if !output.learned_memory_updates.is_empty() {
        &output.learned_memory_updates
    } else if !output.claims.is_empty() {
        &output.claims
    } else {
        &output.observations
    };
    let learned_memory = learned_source
        .iter()
        .map(|observation| ValidatedLearnedMemory {
            kind: observation.kind.clone(),
            claim: observation.claim.clone(),
            confidence: observation.confidence,
            evidence_refs: observation.evidence_refs.clone(),
            status: if observation.confidence >= ACTIVE_CONFIDENCE_MIN {
                ValidatedLearnedMemoryStatus::Active
            } else {
                ValidatedLearnedMemoryStatus::Pending
            },
        })
        .collect();
    Ok(ValidatedDreamOutput {
        learned_memory,
        observations: output.observations,
        diagnostics: output.diagnostics,
        usage: output.usage,
    })
}

pub fn run_and_validate_dream(
    store: &Store,
    project: &ProjectRecord,
    config: DreamRunnerConfig,
    runner: &dyn DreamRunner,
) -> Result<ValidatedDreamOutput> {
    let request = build_evidence_request(store, project, &config)?;
    let output = runner.run(&request)?;
    validate_dream_output(&request.evidence_refs(), output)
}

pub fn validate_output(
    output: &mut DreamRunnerOutput,
    valid_evidence_refs: &BTreeSet<String>,
) -> Result<()> {
    if output.observations.is_empty() {
        bail!("dream output must include at least one observation");
    }
    for observation in output
        .observations
        .iter_mut()
        .chain(output.claims.iter_mut())
        .chain(output.learned_memory_updates.iter_mut())
        .chain(output.counterevidence.iter_mut())
    {
        validate_observation(observation, valid_evidence_refs)?;
        observation.status = if observation.confidence >= ACTIVE_CONFIDENCE_MIN {
            DreamObservationStatus::Active
        } else {
            DreamObservationStatus::Pending
        };
    }
    Ok(())
}

fn validate_observation(
    observation: &DreamObservation,
    valid_evidence_refs: &BTreeSet<String>,
) -> Result<()> {
    if observation.kind.trim().is_empty() {
        bail!("dream observation kind is empty");
    }
    if observation.claim.trim().is_empty() {
        bail!("dream observation claim is empty");
    }
    if !(0.0..=1.0).contains(&observation.confidence) {
        bail!("dream observation confidence must be between 0 and 1");
    }
    if observation.evidence_refs.is_empty() {
        bail!("dream observation requires at least one evidence ref");
    }
    for evidence_ref in observation
        .evidence_refs
        .iter()
        .chain(observation.counterevidence_refs.iter())
    {
        if !evidence_ref.starts_with("mmr://event/") {
            bail!("invalid evidence ref scheme: {evidence_ref}");
        }
        if !valid_evidence_refs.contains(evidence_ref) {
            bail!("dream observation references missing evidence: {evidence_ref}");
        }
    }
    Ok(())
}

fn reject_unknown_top_level_fields(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("dream runner output must be a JSON object"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "observations"
                | "claims"
                | "patterns"
                | "open_loops"
                | "learned_memory_updates"
                | "counterevidence"
                | "recommended_actions"
                | "diagnostics"
                | "usage"
        ) {
            bail!("unknown dream runner output field: {key}");
        }
    }
    Ok(())
}

enum EvidenceProjection {
    Included(DreamEvidence, PiiCoverageStatus),
    Omitted(OmittedDreamEvidence, PiiCoverageStatus),
}

fn event_to_evidence(event: &EventRecord, access: &EvidenceAccess) -> Result<EvidenceProjection> {
    let evidence_ref = format!("mmr://event/{}", event.id);
    let content_text = match access {
        EvidenceAccess::LocalRaw => event.content_text.clone(),
        EvidenceAccess::SharedSafe => {
            let outcome =
                scan_text_with_detector(&event.content_text, &DeterministicPrivacyDetector);
            if outcome.blocks_sync || outcome.pii_coverage.status != PiiCoverageStatus::Available {
                return Ok(EvidenceProjection::Omitted(
                    OmittedDreamEvidence {
                        evidence_ref,
                        reason: "unsafe evidence blocked by redaction policy".to_string(),
                    },
                    outcome.pii_coverage.status,
                ));
            } else {
                outcome.redacted_text
            }
        }
    };
    Ok(EvidenceProjection::Included(
        DreamEvidence {
            evidence_ref,
            source: event.source.clone(),
            role: event.role.clone(),
            event_type: event.event_type.clone(),
            timestamp: event.timestamp.clone(),
            content_text,
        },
        PiiCoverageStatus::Available,
    ))
}

fn evidence_hash(evidence: &[DreamEvidence]) -> String {
    let mut material = String::new();
    for item in evidence {
        material.push_str(&item.evidence_ref);
        material.push('\t');
        material.push_str(&item.content_text);
        material.push('\n');
    }
    content_hash(&material)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

impl DreamRequest {
    pub fn evidence_refs(&self) -> BTreeSet<String> {
        self.evidence
            .iter()
            .map(|evidence| evidence.evidence_ref.clone())
            .collect()
    }
}

pub fn observation_status_counts(
    output: &DreamRunnerOutput,
) -> BTreeMap<DreamObservationStatus, usize> {
    let mut counts = BTreeMap::new();
    for observation in &output.observations {
        *counts.entry(observation.status.clone()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewEvent, Store};

    #[derive(Debug, Clone)]
    struct BypassValidationRunner(DreamRunnerOutput);

    impl DreamRunner for BypassValidationRunner {
        fn run(&self, _request: &DreamRequest) -> Result<DreamRunnerOutput> {
            Ok(self.0.clone())
        }
    }

    fn seeded_request() -> (tempfile::TempDir, Store, ProjectRecord, DreamRequest) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let mut store = Store::open(tmp.path().join("mmr.db")).expect("store");
        let project = store.ensure_project_link(tmp.path()).expect("project");
        let event = NewEvent::new(
            "note",
            "notes",
            "note",
            "user",
            "2026-05-24T15:00:00Z",
            "decision: keep dream output evidence-linked",
            "note-v1",
        )
        .with_source_event_id("dream-note-1");
        store
            .insert_event_with_search_document(&project.id, &event)
            .expect("insert event");
        let config = DreamRunnerConfig {
            runner: "mock".to_string(),
            model: Some("mock-v1".to_string()),
            evidence_access: EvidenceAccess::SharedSafe,
            allow_raw_evidence: false,
            best_of: 1,
            retries: 0,
        };
        let request = build_evidence_request(&store, &project, &config).expect("evidence request");
        (tmp, store, project, request)
    }

    #[test]
    fn mock_runner_valid_output_is_schema_checked() {
        let (_tmp, _store, _project, request) = seeded_request();
        let evidence_ref = &request.evidence[0].evidence_ref;
        let runner = MockDreamRunner::returning_json(format!(
            r#"{{
                "observations": [{{
                    "kind": "preference",
                    "claim": "Keep dream output evidence-linked.",
                    "confidence": 0.82,
                    "scope": "project",
                    "recommended_action": "Preserve mmr:// citations.",
                    "evidence_refs": ["{evidence_ref}"],
                    "patterns": ["evidence-first memory"],
                    "open_loops": [],
                    "counterevidence_refs": []
                }}],
                "diagnostics": {{"warnings": []}},
                "usage": {{"input_tokens": 12, "output_tokens": 8, "cost_usd": 0.0}}
            }}"#
        ));
        let output = runner.run(&request).expect("mock runner output");
        assert_eq!(output.observations.len(), 1);
        assert_eq!(
            output.observations[0].status,
            DreamObservationStatus::Active
        );
    }

    #[test]
    fn invalid_schema_and_hallucinated_refs_are_rejected() {
        let (_tmp, _store, _project, request) = seeded_request();
        let invalid =
            MockDreamRunner::returning_json(r#"{"observations":[{"kind":"preference"}]}"#);
        assert!(invalid.run(&request).is_err());

        let hallucinated = MockDreamRunner::returning_json(
            r#"{"observations":[{"kind":"preference","claim":"No evidence.","confidence":0.8,"evidence_refs":["mmr://event/evt:v1:missing"]}]}"#,
        );
        let err = hallucinated
            .run(&request)
            .expect_err("hallucinated evidence ref should fail");
        assert!(err.to_string().contains("missing evidence"));
    }

    #[test]
    fn low_confidence_observations_are_pending() {
        let (_tmp, _store, _project, request) = seeded_request();
        let evidence_ref = &request.evidence[0].evidence_ref;
        let runner = MockDreamRunner::returning_json(format!(
            r#"{{"observations":[{{"kind":"pattern","claim":"Maybe useful.","confidence":0.31,"evidence_refs":["{evidence_ref}"]}}]}}"#
        ));
        let output = runner.run(&request).expect("runner output");
        assert_eq!(
            output.observations[0].status,
            DreamObservationStatus::Pending
        );
    }

    #[test]
    fn provider_failure_returns_no_output() {
        let (_tmp, _store, _project, request) = seeded_request();
        let runner = MockDreamRunner::failing("provider unavailable");
        let err = runner.run(&request).expect_err("provider failure");
        assert!(err.to_string().contains("provider unavailable"));
    }

    #[test]
    fn runner_config_precedence_prefers_cli_project_user_then_default() {
        let resolution =
            DreamRunnerConfig::resolve_runner(Some("command"), Some("project"), Some("user"));
        assert_eq!(resolution.runner, "command");
        assert_eq!(resolution.source, "cli");

        let resolution = DreamRunnerConfig::resolve_runner(None, Some("project"), Some("user"));
        assert_eq!(resolution.runner, "project");
        assert_eq!(resolution.source, "project");

        let resolution = DreamRunnerConfig::resolve_runner(None, None, Some("user"));
        assert_eq!(resolution.runner, "user");
        assert_eq!(resolution.source, "user");

        let resolution = DreamRunnerConfig::resolve_runner(None, None, None);
        assert_eq!(resolution.runner, DEFAULT_DREAM_RUNNER);
        assert_eq!(resolution.source, "default");

        let config = DreamRunnerConfig {
            runner: "mock".to_string(),
            model: None,
            evidence_access: EvidenceAccess::SharedSafe,
            allow_raw_evidence: false,
            best_of: 2,
            retries: 0,
        };
        let err = config
            .validate_privacy_boundary()
            .expect_err("best-of hook is reserved");
        assert!(err.to_string().contains("best-of"));
    }

    #[test]
    fn command_runner_env_parses_program_and_args() {
        let (program, args) =
            parse_command_parts("python /tmp/runner.py --mode dream").expect("command parts");
        assert_eq!(program, "python");
        assert_eq!(args, vec!["/tmp/runner.py", "--mode", "dream"]);
    }

    #[test]
    fn privacy_boundary_redacts_shared_safe_evidence_and_blocks_raw_remote_access() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let mut store = Store::open(tmp.path().join("mmr.db")).expect("store");
        let project = store.ensure_project_link(tmp.path()).expect("project");
        let event = NewEvent::new(
            "note",
            "notes",
            "note",
            "user",
            "2026-05-24T15:01:00Z",
            "Email person@example.com about privacy boundaries.",
            "note-v1",
        )
        .with_source_event_id("dream-note-secret");
        store
            .insert_event_with_search_document(&project.id, &event)
            .expect("insert event");
        let config = DreamRunnerConfig {
            runner: "mock".to_string(),
            model: None,
            evidence_access: EvidenceAccess::SharedSafe,
            allow_raw_evidence: false,
            best_of: 1,
            retries: 0,
        };
        let request = build_evidence_request(&store, &project, &config).expect("request");
        assert!(
            !request.evidence[0]
                .content_text
                .contains("person@example.com")
        );
        assert!(
            request.evidence[0]
                .content_text
                .contains("[REDACTED:private_email]")
        );

        let raw_remote = DreamRunnerConfig {
            runner: "command".to_string(),
            model: None,
            evidence_access: EvidenceAccess::LocalRaw,
            allow_raw_evidence: true,
            best_of: 1,
            retries: 0,
        };
        let err = raw_remote
            .validate_privacy_boundary()
            .expect_err("remote raw evidence blocked");
        assert!(err.to_string().contains("shared-safe"));
    }

    #[test]
    fn final_validation_rejects_refs_omitted_from_shared_safe_request() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let mut store = Store::open(tmp.path().join("mmr.db")).expect("store");
        let project = store.ensure_project_link(tmp.path()).expect("project");
        let safe = NewEvent::new(
            "note",
            "notes",
            "note",
            "user",
            "2026-05-24T15:03:00Z",
            "safe evidence",
            "note-v1",
        )
        .with_source_event_id("safe-evidence");
        store
            .insert_event_with_search_document(&project.id, &safe)
            .expect("insert safe event");
        let secret = NewEvent::new(
            "note",
            "notes",
            "note",
            "user",
            "2026-05-24T15:04:00Z",
            "api_key=sk-test-secret",
            "note-v1",
        )
        .with_source_event_id("secret-evidence");
        let (secret_event, _) = store
            .insert_event_with_search_document(&project.id, &secret)
            .expect("insert secret event");
        let output = parse_dream_output_json(&format!(
            r#"{{"observations":[{{"kind":"preference","claim":"Cite omitted evidence.","confidence":0.8,"evidence_refs":["mmr://event/{}"]}}]}}"#,
            secret_event.id
        ))
        .expect("parse bypass output");
        let config = DreamRunnerConfig {
            runner: "mock".to_string(),
            model: None,
            evidence_access: EvidenceAccess::SharedSafe,
            allow_raw_evidence: false,
            best_of: 1,
            retries: 0,
        };
        let err = run_and_validate_dream(&store, &project, config, &BypassValidationRunner(output))
            .expect_err("omitted evidence ref must fail final validation");
        assert!(err.to_string().contains("missing evidence"));
    }

    #[test]
    fn claims_are_not_silently_discarded_when_validating_output() {
        let (_tmp, _store, _project, request) = seeded_request();
        let evidence_ref = &request.evidence[0].evidence_ref;
        let mut output = parse_dream_output_json(&format!(
            r#"{{
                "observations": [{{"kind":"observation","claim":"Observed evidence.","confidence":0.8,"evidence_refs":["{evidence_ref}"]}}],
                "claims": [{{"kind":"preference","claim":"Promote explicit claims.","confidence":0.9,"evidence_refs":["{evidence_ref}"]}}]
            }}"#
        ))
        .expect("parse claims output");
        validate_output(&mut output, &request.evidence_refs()).expect("validate claims output");
        let validated =
            validate_dream_output(&request.evidence_refs(), output).expect("validated output");
        assert_eq!(
            validated.learned_memory[0].claim,
            "Promote explicit claims."
        );
    }
}
