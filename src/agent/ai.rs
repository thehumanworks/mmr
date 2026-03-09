use std::cmp::Reverse;

use anyhow::{Result, bail};
use rayon::prelude::*;

use crate::agent::gemini::{Gemini, GeminiGenerateRequest};
use crate::model::{ApiMessage, ApiRememberResponse, SortBy, SortOptions, SortOrder, SourceFilter};
use crate::query::QueryService;

pub const MEMORY_AGENT_SYSTEM_INSTRUCTION: &str = r#"You are a Memory Agent — a specialized summarizer that processes AI coding session transcripts and produces structured continuity briefs. Your sole purpose is to enable an AI agent resuming work on this project to pick up exactly where the previous session left off, at the highest quality.

## Input Format

You receive a list of messages from one or more coding sessions, ordered most recent first. Each message has a role (user/assistant/tool), content, and a timestamp. Messages may span multiple sessions, each identified by a session_id.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RememberMode {
    Latest,
    All,
}

pub struct RememberRequest<'a> {
    pub project: &'a str,
    pub source: Option<SourceFilter>,
    pub mode: RememberMode,
    pub continue_from: Option<&'a str>,
    pub follow_up: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub model: Option<&'a str>,
}

#[derive(Debug)]
struct SessionTranscript {
    session_id: String,
    messages: Vec<ApiMessage>,
}

#[derive(Debug, Clone)]
struct SessionSelection {
    session_id: String,
    project_name: String,
    source: SourceFilter,
}

pub fn remember(
    service: &QueryService,
    request: RememberRequest<'_>,
) -> Result<ApiRememberResponse> {
    if request.follow_up.is_some() && request.continue_from.is_none() {
        bail!("--follow-up requires --continue-from");
    }

    let gemini = Gemini::new(request.model, None)?;
    let system_instruction = build_system_instruction(request.prompt);

    let input = match (request.follow_up, request.continue_from) {
        (Some(follow_up), Some(_)) => follow_up.to_string(),
        _ => {
            let sessions =
                load_session_transcripts(service, request.project, request.source, request.mode);
            let formatted = format_messages_for_input(&sessions);
            format!(
                "Analyze the following AI coding session transcript(s) and produce a continuity brief.\n\n{formatted}"
            )
        }
    };

    let result = gemini.generate(GeminiGenerateRequest {
        input: &input,
        system_instruction: Some(system_instruction.as_str()),
        previous_interaction_id: request.continue_from,
    })?;

    Ok(ApiRememberResponse {
        summary: result.text,
        interaction_id: result.interaction_id,
    })
}

fn load_session_transcripts(
    service: &QueryService,
    project: &str,
    source: Option<SourceFilter>,
    mode: RememberMode,
) -> Vec<SessionTranscript> {
    let sessions = service.sessions(
        Some(project),
        source,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    );

    let selected = select_sessions(&sessions.sessions, mode);
    let mut transcripts = selected
        .par_iter()
        .map(|selection| {
            let response = service.messages(
                Some(&selection.session_id),
                Some(&selection.project_name),
                Some(selection.source),
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
            );
            SessionTranscript {
                session_id: selection.session_id.clone(),
                messages: response.messages,
            }
        })
        .collect::<Vec<_>>();

    transcripts.sort_by_key(|transcript| {
        Reverse(
            transcript
                .messages
                .first()
                .map(|msg| msg.timestamp.clone())
                .unwrap_or_default(),
        )
    });

    transcripts
}

fn select_sessions(
    sessions: &[crate::model::ApiSession],
    mode: RememberMode,
) -> Vec<SessionSelection> {
    let all = sessions
        .iter()
        .filter_map(|session| {
            parse_source_filter(&session.source).map(|source| SessionSelection {
                session_id: session.session_id.clone(),
                project_name: session.project_name.clone(),
                source,
            })
        })
        .collect::<Vec<_>>();

    if mode == RememberMode::Latest {
        return all.into_iter().take(1).collect();
    }

    all
}

fn parse_source_filter(source: &str) -> Option<SourceFilter> {
    match source {
        "claude" => Some(SourceFilter::Claude),
        "codex" => Some(SourceFilter::Codex),
        _ => None,
    }
}

fn format_messages_for_input(session_data: &[SessionTranscript]) -> String {
    let mut parts = Vec::new();

    for session in session_data {
        parts.push(format!("=== Session: {} ===", session.session_id));
        for msg in &session.messages {
            let content = maybe_truncate_tool_message(&msg.role, &msg.content);
            parts.push(format!("[{}] {}: {}", msg.timestamp, msg.role, content));
        }
        parts.push(String::new());
    }

    parts.join("\n")
}

fn maybe_truncate_tool_message(role: &str, content: &str) -> String {
    if role != "tool" || content.chars().count() <= 2000 {
        return content.to_string();
    }

    let end = content
        .char_indices()
        .nth(2000)
        .map(|(index, _)| index)
        .unwrap_or(content.len());
    format!("{}\n... [truncated]", &content[..end])
}

fn build_system_instruction(prompt: Option<&str>) -> String {
    let prompt = match prompt {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => return MEMORY_AGENT_SYSTEM_INSTRUCTION.to_string(),
    };

    format!("{MEMORY_AGENT_SYSTEM_INSTRUCTION}\n\n## Additional Directives\n{prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_messages_are_truncated() {
        let long_content = "x".repeat(2010);
        let truncated = maybe_truncate_tool_message("tool", &long_content);
        assert!(truncated.ends_with("\n... [truncated]"));
        assert!(truncated.len() < long_content.len() + 20);
    }

    #[test]
    fn non_tool_messages_are_unchanged() {
        let content = "hello";
        let formatted = maybe_truncate_tool_message("assistant", content);
        assert_eq!(formatted, content);
    }

    #[test]
    fn prompt_is_additive_to_base_system_instruction() {
        let instruction = build_system_instruction(Some("Keep it concise."));
        assert!(instruction.contains("Memory Agent"));
        assert!(instruction.contains("Keep it concise."));
    }
}
