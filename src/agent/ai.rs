use anyhow::{Result, bail};
use codex_app_server_sdk::ResumeThread;

use crate::agent::codex::CodexAgent;
use crate::agent::gemini_api::Gemini;
use crate::messages::service::QueryService;
use crate::messages::utils::{format_messages_for_input, load_session_transcripts};
use crate::types::{
    Agent, CodexGenerateRequest, GeminiGenerateRequest, InteractionInput, InteractionInputType,
    RememberRequest, RememberResponse,
};

/// Preserved in every call. Establishes agent identity and describes the input format.
pub const MEMORY_AGENT_BASE_INSTRUCTION: &str = r#"You are a Memory Agent — a specialized AI that analyzes AI coding session transcripts.

## Input Format

You receive a list of messages from one or more coding sessions, ordered most recent first. Each message has a role (user/assistant/tool), content, and a timestamp. Messages may span multiple sessions, each identified by a session_id."#;

/// Default output format and rules appended to the base. Overridden by `--instructions`.
const MEMORY_AGENT_DEFAULT_OUTPUT_INSTRUCTION: &str = r#"## Purpose

Produce structured continuity briefs that enable an AI agent resuming work on this project to pick up exactly where the previous session left off, at the highest quality.

## Output Format

Produce a structured summary with these sections. Omit any section that has no relevant content.

### 1. Status
One sentence: what state is the project/task in right now? Is there an open task in progress, or was the last session completed cleanly?

### 2. What Was Done
Bullet list of concrete changes made, ordered by importance:
- Files created, modified, or deleted (with paths)
- Features implemented, bugs fixed, refactors applied
- Tests added or modified, and their pass/fail status at session end
- Dependencies added or changed
- Configuration or infrastructure changes

### 3. Key Decisions & Context
Bullet list of decisions made during the session(s) that a resuming agent needs to know:
- Architectural choices and why they were made
- Approaches that were tried and abandoned (and why)
- Constraints or requirements discovered during the work
- User preferences or instructions that affect future work

### 4. Open Items
Bullet list of anything unfinished, blocked, or explicitly deferred:
- Tasks started but not completed
- Known bugs or failing tests at session end
- TODOs mentioned by user or agent
- Questions that were raised but not answered

### 5. Relevant File Map
List the key files involved in the work with a one-line description of each file's role. Only include files that were actively worked on or are critical context for resuming.

### 6. Resume Instructions
A short paragraph (2-4 sentences) telling the resuming agent exactly what to do first. Be specific: name the file, the function, the test, the command. This is the most important section — it should be actionable enough that the resuming agent can start working immediately without re-reading the full history.

## Rules

- Be precise. Use exact file paths, function names, variable names, and command invocations.
- Be concise. This is a working document, not a narrative. No filler, no hedging.
- Prioritize recency. More recent sessions matter more than older ones.
- Distinguish facts from intent. "User asked for X" is different from "X was implemented".
- If messages are from multiple sessions, note session boundaries only when the context shift matters.
- If the conversation is too short or trivial to warrant a full summary, say so in one line and skip the sections."#;

fn build_system_instruction(instructions: Option<&str>) -> String {
    match instructions {
        Some(custom) => format!("{MEMORY_AGENT_BASE_INSTRUCTION}\n\n{custom}"),
        None => {
            format!("{MEMORY_AGENT_BASE_INSTRUCTION}\n\n{MEMORY_AGENT_DEFAULT_OUTPUT_INSTRUCTION}")
        }
    }
}

fn remember_with_gemini(
    service: &QueryService,
    request: RememberRequest<'_>,
) -> Result<RememberResponse> {
    let gemini = Gemini::new(request.model, None)?;
    let system_instruction = build_system_instruction(request.instructions);

    let input = if request.continue_from.is_some() {
        request.follow_up.unwrap_or_default().to_string()
    } else {
        let sessions =
            load_session_transcripts(service, request.project, request.source, request.mode)?;
        let formatted = format_messages_for_input(&sessions);
        format!("Analyze the following AI coding session transcript(s).\n\n{formatted}")
    };

    let result = gemini.generate(GeminiGenerateRequest {
        input: vec![InteractionInput::new(InteractionInputType::Text, &input)],
        system_instruction: Some(&system_instruction),
        previous_interaction_id: request.continue_from,
    })?;

    Ok(RememberResponse::new(
        Agent::Gemini,
        result.text,
        Some(result.interaction_id),
    ))
}

async fn remember_with_codex(
    service: &QueryService,
    request: RememberRequest<'_>,
) -> Result<RememberResponse> {
    let codex = CodexAgent::new().await;
    let system_instruction = build_system_instruction(request.instructions);

    let input = if request.continue_from.is_some() {
        request.follow_up.unwrap_or_default().to_string()
    } else {
        let sessions =
            load_session_transcripts(service, request.project, request.source, request.mode)?;
        let formatted = format_messages_for_input(&sessions);
        format!("Analyze the following AI coding session transcript(s).\n\n{formatted}")
    };

    let result = codex
        .generate(CodexGenerateRequest {
            input: &input,
            developer_instructions: Some(&system_instruction),
            resume_thread: request
                .continue_from
                .map(|id| ResumeThread::ById(id.to_string())),
        })
        .await?;

    Ok(RememberResponse::new(
        Agent::Codex,
        result.get_text().to_string(),
        result.get_thread_id().map(|id| id.to_string()),
    ))
}

pub async fn remember(
    service: &QueryService,
    request: RememberRequest<'_>,
) -> Result<RememberResponse> {
    if request.follow_up.is_some() && request.continue_from.is_none() {
        bail!("--follow-up requires --continue-from");
    }

    match request.agent {
        Agent::Gemini => remember_with_gemini(service, request),
        Agent::Codex => remember_with_codex(service, request).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_instructions_override_output_section_but_preserve_base() {
        let custom = "Answer the user's question directly.";
        let effective = build_system_instruction(Some(custom));
        assert!(
            effective.contains("Memory Agent"),
            "base identity must be preserved"
        );
        assert!(
            effective.contains("Input Format"),
            "base input format must be preserved"
        );
        assert!(
            effective.contains(custom),
            "custom instructions must appear"
        );
        assert!(
            !effective.contains("Output Format"),
            "default output format must be replaced"
        );
        assert!(
            !effective.contains("Resume Instructions"),
            "default output sections must be replaced"
        );
        assert!(
            !effective.contains("continuity brief"),
            "default purpose must not leak into custom instructions"
        );
        assert!(
            !effective.contains("Purpose"),
            "default Purpose section must be replaced"
        );
    }

    #[test]
    fn default_instructions_include_full_output_section() {
        let effective = build_system_instruction(None);
        assert!(effective.contains("Memory Agent"));
        assert!(effective.contains("Input Format"));
        assert!(effective.contains("Purpose"));
        assert!(effective.contains("continuity brief"));
        assert!(effective.contains("Output Format"));
        assert!(effective.contains("Resume Instructions"));
        assert!(effective.contains("Rules"));
    }

    #[test]
    fn base_instruction_contains_no_output_directing_language() {
        let base = MEMORY_AGENT_BASE_INSTRUCTION;
        assert!(
            base.contains("Memory Agent"),
            "base must establish agent identity"
        );
        assert!(
            base.contains("Input Format"),
            "base must describe the input format"
        );
        assert!(
            !base.contains("continuity brief"),
            "base must not direct output format"
        );
        assert!(
            !base.contains("sole purpose"),
            "base must not constrain the agent's purpose"
        );
        assert!(
            !base.contains("highest quality"),
            "base must not include output quality directives"
        );
    }
}
