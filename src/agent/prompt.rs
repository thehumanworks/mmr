use anyhow::Result;
use std::process::Command;

use crate::agent::codex::CodexAgent;
use crate::agent::gemini_api::Gemini;
use crate::messages::service::QueryService;
use crate::messages::utils::{format_messages_for_input, load_session_transcripts};
use crate::types::{
    Agent, CodexGenerateRequest, GeminiGenerateRequest, InteractionInput, InteractionInputType,
    PromptRequest, PromptResponse, RememberSelection, TargetAgent,
};

pub const PROMPT_OPTIMIZER_BASE_INSTRUCTION: &str = r#"You are a Prompt Optimizer — a specialized AI that crafts high-quality prompts for AI coding agents.

## Input
You receive:
1. A user query describing what they want to accomplish.
2. Optionally, session transcript(s) from prior coding sessions on this project, ordered most recent first.
3. Optionally, codebase snippets showing relevant files and code patterns.

## Task
Generate an optimized prompt that the user can give to the target AI coding agent. The prompt must:
- Be self-contained and actionable — the target agent should be able to start working immediately.
- Incorporate relevant context from session history (decisions made, files modified, patterns established, open items).
- Prioritize recent sessions over older ones.
- Reference specific file paths, function names, and patterns from the project when available.
- Be precise about the desired outcome, including success criteria and verification steps.

## Output
Return ONLY the optimized prompt text. No preamble, no explanation, no wrapping. The output will be copied directly into the target agent's input."#;

pub const CLAUDE_TARGET_INSTRUCTION: &str = r#"## Target: Claude Code (Claude Opus 4.6)

### Optimize for Claude's strengths
- Structure the prompt with XML tags (<context>, <task>, <constraints>, <verification>) — Claude parses these unambiguously.
- Be specific and direct — Claude excels at following complex multi-step instructions precisely.
- Include concrete examples of desired output when the format matters.
- Encourage reading files before editing — Claude performs best when it understands existing code first.
- Leverage Claude's strong planning: frame multi-step tasks as sequential numbered steps.
- Place long-form context (file contents, history) at the top, with the task query at the end.

### Compensate for Claude's weaknesses
- Explicitly state "Only make changes that are directly requested. Do not refactor surrounding code or add unnecessary abstractions." — Claude tends to over-engineer.
- Use normal phrasing instead of aggressive emphasis (avoid CRITICAL, MUST, NEVER in caps) — Opus 4.6 is more responsive to the system prompt and may overtrigger on emphatic instructions.
- Include "Do not add error handling, fallbacks, or validation for scenarios that cannot happen." — Claude adds unnecessary defensive code.
- Add "Keep solutions simple and focused. Do not add features beyond what was asked." — prevents scope creep.
- Specify "Do not add docstrings, comments, or type annotations to code you did not change." — prevents unnecessary changes.

### Claude Code harness conventions
- Reference CLAUDE.md if the project has one.
- Specify test commands explicitly (e.g., "Run `cargo test` to verify").
- Include file paths to read first before making changes.
- Frame verification as: "After completing changes, run [command] and confirm [expected result]."
- Encourage parallel tool calls for independent file reads."#;

pub const CODEX_TARGET_INSTRUCTION: &str = r#"## Target: Codex CLI (GPT-5.4)

### Optimize for Codex's strengths
- Be concise and imperative — Codex works best with terse, action-oriented instructions.
- Specify file paths and function names upfront — Codex excels at targeted edits when given exact locations.
- Include test commands alongside implementation requests — GPT-5.4 generates better code when tests are specified upfront.
- Use numbered steps for sequential tasks — Codex follows ordered instructions reliably.
- Leverage AGENTS.md conventions if the project has one.
- Define explicit output contracts and completion criteria.

### Compensate for Codex's weaknesses
- Include "Search the codebase first before adding new logic — reuse existing helpers and patterns." — Codex may miss existing utilities.
- Add "Read the relevant files before making changes to understand existing patterns." — Codex can jump to edits without sufficient context.
- Specify "Conform to existing codebase conventions: naming, formatting, patterns, and error handling style." — Codex may produce less idiomatic code.
- Include explicit edge cases to handle — GPT-5.4 may miss edge cases if not enumerated.
- Add "Do not use broad try/catch blocks or silent defaults — propagate errors explicitly."
- Add "Batch logical edits together rather than making repeated micro-edits."

### Codex CLI harness conventions
- Prefer `rg` over `grep` for file search (faster in Codex sandbox).
- Include verification commands: "Run `[test command]` and confirm all tests pass."
- Reference AGENTS.md file conventions if present.
- Use the apply_patch tool for single-file edits."#;

fn build_optimizer_instruction(target: TargetAgent) -> String {
    let target_instruction = match target {
        TargetAgent::Claude => CLAUDE_TARGET_INSTRUCTION,
        TargetAgent::Codex => CODEX_TARGET_INSTRUCTION,
    };
    format!("{PROMPT_OPTIMIZER_BASE_INSTRUCTION}\n\n{target_instruction}")
}

fn build_optimizer_input(
    query: &str,
    session_context: Option<&str>,
    codebase_context: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    if let Some(sessions) = session_context {
        parts.push(format!("<session_history>\n{sessions}\n</session_history>"));
    }

    if let Some(codebase) = codebase_context {
        parts.push(format!(
            "<codebase_context>\n{codebase}\n</codebase_context>"
        ));
    }

    parts.push(format!(
        "<query>\n{query}\n</query>\n\nGenerate an optimized prompt for the target agent based on the query and any provided context."
    ));

    parts.join("\n\n")
}

fn gather_codebase_context(query: &str, project_path: &str) -> Option<String> {
    let mut sections = Vec::new();

    // File tree listing (limited to 50 entries)
    if let Ok(output) = Command::new("rg")
        .args(["--files", "--max-count", "50"])
        .current_dir(project_path)
        .output()
        && output.status.success()
    {
        let files = String::from_utf8_lossy(&output.stdout);
        let trimmed = files.trim();
        if !trimmed.is_empty() {
            sections.push(format!("## Project files\n{trimmed}"));
        }
    }

    // Keyword search: extract words from query (3+ chars) and search for them
    let keywords: Vec<&str> = query
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .filter(|w| {
            !matches!(
                w.to_ascii_lowercase().as_str(),
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "that"
                    | "this"
                    | "from"
                    | "have"
                    | "not"
                    | "are"
                    | "was"
                    | "but"
                    | "all"
            )
        })
        .take(5)
        .collect();

    for keyword in &keywords {
        if let Ok(output) = Command::new("rg")
            .args(["--max-count", "3", "--max-columns", "200", "-n", keyword])
            .current_dir(project_path)
            .output()
            && output.status.success()
        {
            let matches = String::from_utf8_lossy(&output.stdout);
            let trimmed = matches.trim();
            if !trimmed.is_empty() {
                sections.push(format!("## Matches for \"{keyword}\"\n{trimmed}"));
            }
        }
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

async fn optimize_with_gemini(
    service: &QueryService,
    request: PromptRequest<'_>,
) -> Result<PromptResponse> {
    let gemini = Gemini::new(request.model, None)?;
    let system_instruction = build_optimizer_instruction(request.target);

    let (session_context, codebase_context) = gather_context(service, &request);

    let input = build_optimizer_input(
        request.query,
        session_context.as_deref(),
        codebase_context.as_deref(),
    );

    let result = gemini
        .generate(GeminiGenerateRequest {
            input: vec![InteractionInput::new(InteractionInputType::Text, &input)],
            system_instruction: Some(&system_instruction),
        })
        .await?;

    Ok(PromptResponse {
        target: request.target,
        prompt: result.text,
    })
}

async fn optimize_with_codex(
    service: &QueryService,
    request: PromptRequest<'_>,
) -> Result<PromptResponse> {
    let codex = CodexAgent::new().await;
    let system_instruction = build_optimizer_instruction(request.target);

    let (session_context, codebase_context) = gather_context(service, &request);

    let input = build_optimizer_input(
        request.query,
        session_context.as_deref(),
        codebase_context.as_deref(),
    );

    let result = codex
        .generate(CodexGenerateRequest {
            input: &input,
            developer_instructions: Some(&system_instruction),
        })
        .await?;

    Ok(PromptResponse {
        target: request.target,
        prompt: result.get_text().to_string(),
    })
}

fn gather_context(
    service: &QueryService,
    request: &PromptRequest<'_>,
) -> (Option<String>, Option<String>) {
    let session_context = load_session_transcripts(
        service,
        request.project,
        &RememberSelection::All,
        request.source,
    )
    .ok()
    .map(|transcripts| format_messages_for_input(&transcripts));

    let codebase_context = if session_context.is_none() {
        gather_codebase_context(request.query, request.project)
    } else {
        None
    };

    (session_context, codebase_context)
}

pub async fn optimize_prompt(
    service: &QueryService,
    request: PromptRequest<'_>,
) -> Result<PromptResponse> {
    match request.agent {
        Agent::Gemini => optimize_with_gemini(service, request).await,
        Agent::Codex => optimize_with_codex(service, request).await,
    }
}

pub fn try_copy_to_clipboard(text: &str) {
    let _ = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_target_instruction_includes_xml_tags_and_anti_overengineering() {
        let instruction = build_optimizer_instruction(TargetAgent::Claude);
        assert!(
            instruction.contains("Prompt Optimizer"),
            "must include base identity"
        );
        assert!(
            instruction.contains("XML tags"),
            "Claude target must recommend XML tags"
        );
        assert!(
            instruction.contains("Do not refactor surrounding code"),
            "Claude target must include anti-over-engineering"
        );
        assert!(
            instruction.contains("CLAUDE.md"),
            "Claude target must reference CLAUDE.md"
        );
        assert!(
            instruction.contains("parallel tool calls"),
            "Claude target must encourage parallel tool calls"
        );
    }

    #[test]
    fn codex_target_instruction_includes_terse_style_and_agents_md() {
        let instruction = build_optimizer_instruction(TargetAgent::Codex);
        assert!(
            instruction.contains("Prompt Optimizer"),
            "must include base identity"
        );
        assert!(
            instruction.contains("concise and imperative"),
            "Codex target must recommend terse style"
        );
        assert!(
            instruction.contains("AGENTS.md"),
            "Codex target must reference AGENTS.md"
        );
        assert!(
            instruction.contains("apply_patch"),
            "Codex target must reference apply_patch tool"
        );
        assert!(
            instruction.contains("rg"),
            "Codex target must recommend rg over grep"
        );
    }

    #[test]
    fn both_targets_share_base_instruction() {
        let claude = build_optimizer_instruction(TargetAgent::Claude);
        let codex = build_optimizer_instruction(TargetAgent::Codex);
        assert!(claude.contains(PROMPT_OPTIMIZER_BASE_INSTRUCTION));
        assert!(codex.contains(PROMPT_OPTIMIZER_BASE_INSTRUCTION));
    }

    #[test]
    fn build_input_with_all_context() {
        let input = build_optimizer_input(
            "add auth",
            Some("session transcript here"),
            Some("file list here"),
        );
        assert!(input.contains("<session_history>"));
        assert!(input.contains("session transcript here"));
        assert!(input.contains("<codebase_context>"));
        assert!(input.contains("file list here"));
        assert!(input.contains("<query>"));
        assert!(input.contains("add auth"));
    }

    #[test]
    fn build_input_with_query_only() {
        let input = build_optimizer_input("fix bug", None, None);
        assert!(!input.contains("<session_history>"));
        assert!(!input.contains("<codebase_context>"));
        assert!(input.contains("<query>"));
        assert!(input.contains("fix bug"));
    }

    #[test]
    fn build_input_with_session_context_only() {
        let input = build_optimizer_input("add tests", Some("transcript"), None);
        assert!(input.contains("<session_history>"));
        assert!(!input.contains("<codebase_context>"));
        assert!(input.contains("<query>"));
    }

    #[test]
    fn gather_codebase_context_returns_none_for_nonexistent_dir() {
        let result = gather_codebase_context("test query", "/nonexistent/path/mmr-test-12345");
        assert!(result.is_none());
    }

    #[test]
    fn try_copy_to_clipboard_does_not_panic() {
        // On CI without display server, this should silently fail
        try_copy_to_clipboard("test text");
    }
}
