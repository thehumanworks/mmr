use std::cmp::Reverse;

use crate::{
    messages::service::QueryService,
    types::{
        ApiSession, RememberSelection, SortBy, SortOptions, SortOrder, SourceFilter,
        agent::{SessionSelection, SessionTranscript},
    },
};
use anyhow::bail;
use rayon::prelude::*;

pub(crate) fn load_session_transcripts(
    service: &QueryService,
    project: &str,
    selection: &RememberSelection,
    source: Option<SourceFilter>,
) -> anyhow::Result<Vec<SessionTranscript>> {
    let sessions = service.sessions(
        Some(project),
        source,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    );

    let selected = select_sessions(&sessions.sessions, selection);
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

    if transcripts.is_empty() {
        bail!("No sessions found for project {}", project);
    }

    Ok(transcripts)
}

fn select_sessions(
    sessions: &[ApiSession],
    selection: &RememberSelection,
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

    match selection {
        RememberSelection::Latest => all.into_iter().take(1).collect(),
        RememberSelection::All => all,
        RememberSelection::Session { session_id } => all
            .into_iter()
            .filter(|s| s.session_id == *session_id)
            .collect(),
    }
}

fn parse_source_filter(source: &str) -> Option<SourceFilter> {
    match source {
        "claude" => Some(SourceFilter::Claude),
        "codex" => Some(SourceFilter::Codex),
        _ => None,
    }
}

pub(crate) fn format_messages_for_input(session_data: &[SessionTranscript]) -> String {
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
}
