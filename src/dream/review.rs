//! Stateless proposal review. Validation text is data, never an executable job.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use super::{DreamEvidence, DreamRunnerOutput, parse_runner_output};

pub const MAX_REVIEW_BYTES: usize = 64 * 1024;
const MAX_OBSERVATIONS: usize = 16;
const MAX_TEXT_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodificationProposal {
    pub state: ProposalState,
    pub target: String,
    pub expected_behavior: String,
    pub validation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Proposed,
}

fn bounded_text(name: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        bail!("review {name} must contain 1..={MAX_TEXT_CHARS} characters");
    }
    Ok(())
}

pub fn review_output(json: &str, evidence: &[DreamEvidence]) -> Result<DreamRunnerOutput> {
    if json.len() > MAX_REVIEW_BYTES {
        bail!("review input exceeds {MAX_REVIEW_BYTES} bytes");
    }
    let projects = evidence
        .iter()
        .map(|event| (event.evidence_ref.clone(), event.project_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let output = parse_runner_output(json, projects.keys().cloned().collect())?;
    let observations = output
        .observations
        .iter()
        .chain(&output.claims)
        .chain(&output.learned_memory_updates)
        .chain(&output.counterevidence)
        .collect::<Vec<_>>();
    if observations.len() > MAX_OBSERVATIONS {
        bail!("review accepts at most {MAX_OBSERVATIONS} observations");
    }
    for observation in observations {
        bounded_text("claim", &observation.claim)?;
        let scope = observation.scope.as_deref().unwrap_or_default();
        bounded_text("scope", scope)?;
        for reference in observation
            .evidence_refs
            .iter()
            .chain(&observation.counterevidence_refs)
        {
            if projects.get(reference).copied() != Some(scope) {
                bail!("review scope must match the project_id of every cited event");
            }
        }
        if let Some(action) = &observation.recommended_action {
            bounded_text("recommended_action", action)?;
        }
        if matches!(
            observation.kind.as_str(),
            "judgment" | "duplicate" | "superseded"
        ) {
            bounded_text(
                "rationale",
                observation
                    .recommended_action
                    .as_deref()
                    .unwrap_or_default(),
            )?;
            if observation.codification.is_some() {
                bail!("judgment/duplicate/superseded lessons cannot also propose codification");
            }
            if observation.kind != "judgment" && observation.counterevidence_refs.is_empty() {
                bail!("duplicate/superseded lessons require replacement counterevidence refs");
            }
        }
        if let Some(proposal) = &observation.codification {
            let distinct = observation.evidence_refs.iter().collect::<BTreeSet<_>>();
            if distinct.len() < 2 {
                bail!("codification requires at least two distinct supporting evidence refs");
            }
            bounded_text("target", &proposal.target)?;
            if proposal.target.contains(['\\', ':'])
                || proposal.target.chars().any(char::is_control)
                || !Path::new(&proposal.target)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
            {
                bail!("codification target must be a project-relative path without traversal");
            }
            bounded_text("expected_behavior", &proposal.expected_behavior)?;
            bounded_text("validation", &proposal.validation)?;
        }
    }
    Ok(output)
}
