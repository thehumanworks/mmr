use anyhow::Result;
use std::process::Command;

use crate::agent::codex::CodexAgent;
use crate::agent::cursor::CursorAgent;
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

### Prompt shape for GPT-5.4
- Use explicit XML-tagged contracts. Prefer `<output_contract>`, `<completeness_contract>`, `<missing_context_gating>`, and `<verification_loop>` instead of vague prose.
- In `<output_contract>`, specify the exact sections, their order, the required format, any length limits, and "Do not include extra commentary."
- In `<completeness_contract>`, state that the task is incomplete until every requested item is covered or marked `[blocked]`.
- In `<missing_context_gating>`, tell Codex not to guess, to retrieve missing context when possible, and to ask one minimal clarifying question only if the missing information is not retrievable.
- In `<verification_loop>`, require a final check for correctness, grounding in the available context, output format, and validation results.

### Execution and tool-use guidance
- Add `<tool_persistence_rules>` so Codex keeps using tools until the task is complete and verification passes.
- Add `<dependency_checks>` so prerequisite discovery happens before edits, not after.
- Be concise and imperative, but make the task concrete: name file paths, functions, commands, edge cases, and success criteria.
- Use numbered steps when sequence matters, and include explicit verification commands alongside implementation requests.
- Reference AGENTS.md conventions if the project has one.

### Codex CLI harness conventions
- Prefer `rg` over `grep` for search.
- Tell Codex to read the relevant files before editing and reuse existing helpers and patterns.
- Ask for focused edits that match the existing codebase conventions.
- Use the apply_patch tool for single-file edits and batch logical edits together."#;

pub const CURSOR_TARGET_INSTRUCTION: &str = r#"## Target: Cursor Agent (Composer)

### Optimize for Cursor's strengths
- Be direct and specific — Cursor's agent excels at focused, single-task instructions with clear file paths.
- Reference specific files by path — Cursor automatically reads referenced files via tool calling.
- Include concrete examples of desired output when the format matters.
- Leverage Cursor's automatic file context: mention filenames and the agent will read them.
- Frame multi-step tasks as sequential numbered steps with explicit verification.

### Cursor Agent harness conventions
- Cursor operates in print mode with --force for direct file modifications.
- Specify output format when needed (text, json, stream-json).
- Reference .cursorrules if the project has one.
- Include test commands explicitly (e.g., "Run `cargo test` to verify").
- Include file paths to read first before making changes.
- Frame verification as: "After completing changes, run [command] and confirm [expected result]."
- Keep prompts focused — Cursor works best with one clear objective per prompt."#;

fn build_optimizer_instruction(target: TargetAgent) -> String {
    let target_instruction = match target {
        TargetAgent::Claude => CLAUDE_TARGET_INSTRUCTION,
        TargetAgent::Codex => CODEX_TARGET_INSTRUCTION,
        TargetAgent::Cursor => CURSOR_TARGET_INSTRUCTION,
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

fn optimize_with_cursor(
    service: &QueryService,
    request: PromptRequest<'_>,
) -> Result<PromptResponse> {
    let cursor = CursorAgent::new(None, None::<String>);
    let system_instruction = build_optimizer_instruction(request.target);

    let (session_context, codebase_context) = gather_context(service, &request);

    let input = build_optimizer_input(
        request.query,
        session_context.as_deref(),
        codebase_context.as_deref(),
    );

    let result = cursor.generate(&format!(
        "<system>\n{}\n</system>\n\n<user>{}</user>\n",
        system_instruction, input
    ))?;

    Ok(PromptResponse {
        target: request.target,
        prompt: result,
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
        Agent::Cursor => optimize_with_cursor(service, request),
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
    fn codex_target_instruction_uses_gpt54_contract_patterns() {
        let instruction = build_optimizer_instruction(TargetAgent::Codex);
        assert!(
            instruction.contains("Prompt Optimizer"),
            "must include base identity"
        );
        assert!(
            instruction.contains("<output_contract>"),
            "Codex target must include output contracts"
        );
        assert!(
            instruction.contains("<completeness_contract>"),
            "Codex target must include completeness contracts"
        );
        assert!(
            instruction.contains("<verification_loop>"),
            "Codex target must include verification loop guidance"
        );
        assert!(
            instruction.contains("<tool_persistence_rules>"),
            "Codex target must include tool persistence guidance"
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
    fn all_targets_share_base_instruction() {
        let claude = build_optimizer_instruction(TargetAgent::Claude);
        let codex = build_optimizer_instruction(TargetAgent::Codex);
        let cursor = build_optimizer_instruction(TargetAgent::Cursor);
        assert!(claude.contains(PROMPT_OPTIMIZER_BASE_INSTRUCTION));
        assert!(codex.contains(PROMPT_OPTIMIZER_BASE_INSTRUCTION));
        assert!(cursor.contains(PROMPT_OPTIMIZER_BASE_INSTRUCTION));
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
