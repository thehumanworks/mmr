//! `mmr find`: case-insensitive text search over native provider history.
//!
//! It reads the same transcripts as `read project`, so it needs no setup, and
//! every match names a session that `read session` accepts.

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use super::{
    effective_project_scope, oldest_first, read_cwd_project_messages, serialize, shell_quote,
};
use crate::messages::service::{MessageQueryOptions, QueryService};
use crate::types::{ApiMessage, SourceFilter};

/// Characters of context kept on each side of the first match in a snippet.
const SNIPPET_CONTEXT_CHARS: usize = 120;

#[derive(Args, Debug)]
pub struct FindArgs {
    /// Text to find in message content (case-insensitive)
    #[arg(value_parser = clap::builder::NonEmptyStringValueParser::new())]
    query: String,
    /// Project name or path (omit to use the current directory)
    #[arg(long, conflicts_with = "all")]
    project: Option<String>,
    /// Search every project instead of the current one
    #[arg(long)]
    all: bool,
    /// Only search this session
    #[arg(long)]
    session: Option<String>,
    /// Maximum number of matches to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Number of newest-first matches to skip
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Debug, Serialize)]
struct FindResponse {
    command: &'static str,
    query: String,
    scope: FindScope,
    total_matches: usize,
    matches: Vec<FindMatch>,
    next_page: bool,
    next_offset: usize,
    next_command: Option<String>,
}

#[derive(Debug, Serialize)]
struct FindScope {
    project: Option<String>,
    all: bool,
    source: Option<&'static str>,
    session: Option<String>,
}

#[derive(Debug, Serialize)]
struct FindMatch {
    session_id: String,
    source: String,
    project_name: String,
    timestamp: String,
    role: String,
    snippet: String,
    read_command: String,
}

pub(super) fn find_output(
    args: FindArgs,
    service: &QueryService,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let (project, messages) = scope_messages(service, &args, source_filter)?;

    let needle = args.query.to_lowercase();
    let mut matches = messages
        .iter()
        .filter_map(|message| find_match(message, &args.query, &needle))
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    let total_matches = matches.len();
    let page = matches
        .into_iter()
        .skip(args.offset)
        .take(args.limit)
        .collect::<Vec<_>>();
    let next_offset = args.offset.saturating_add(page.len());
    let next_page = next_offset < total_matches;
    let response = FindResponse {
        command: "find",
        next_command: next_page.then(|| next_find_command(&args, source_filter, next_offset)),
        query: args.query,
        scope: FindScope {
            project,
            all: args.all,
            source: source_filter.map(SourceFilter::as_str),
            session: args.session,
        },
        total_matches,
        matches: page,
        next_page,
        next_offset,
    };
    serialize(&response, pretty)
}

/// The project in scope and the messages `find` searches: one session, the
/// explicit project, every project with `--all`, or else the cwd project.
fn scope_messages(
    service: &QueryService,
    args: &FindArgs,
    source_filter: Option<SourceFilter>,
) -> Result<(Option<String>, Vec<ApiMessage>)> {
    let unpaged = MessageQueryOptions::new(None, 0, oldest_first());
    let sessions = args.session.iter().cloned().collect::<Vec<_>>();
    if args.session.is_some() || args.project.is_some() || args.all {
        let project = args.project.clone();
        let messages = service.messages(&sessions, project.as_deref(), source_filter, unpaged)?;
        return Ok((project, messages.messages));
    }
    match effective_project_scope(None, false) {
        Some(project) => {
            let messages = read_cwd_project_messages(service, source_filter, None, 0)?;
            Ok((Some(project), messages.messages))
        }
        None => {
            let messages = service.messages(&[], None, source_filter, unpaged)?;
            Ok((None, messages.messages))
        }
    }
}

fn find_match(message: &ApiMessage, query: &str, needle: &str) -> Option<FindMatch> {
    if !message.content.to_lowercase().contains(needle) {
        return None;
    }
    let source = message.source.as_str().to_string();
    Some(FindMatch {
        read_command: format!(
            "mmr --source {source} read session {}",
            shell_quote(&message.session_id)
        ),
        session_id: message.session_id.clone(),
        source,
        project_name: message.project_name.clone(),
        timestamp: message.timestamp.clone(),
        role: message.role.clone(),
        snippet: snippet(&message.content, query),
    })
}

/// The first match with up to `SNIPPET_CONTEXT_CHARS` characters either side,
/// whitespace collapsed, and `…` where the content was cut.
fn snippet(content: &str, query: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let start = match_char_index(&chars, query).unwrap_or(0);
    let from = start.saturating_sub(SNIPPET_CONTEXT_CHARS);
    let to = (start + query.chars().count() + SNIPPET_CONTEXT_CHARS).min(chars.len());
    let mut text = chars[from..to]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if from > 0 {
        text.insert(0, '…');
    }
    if to < chars.len() {
        text.push('…');
    }
    text
}

/// Index of the first character where `query` matches, ignoring case.
fn match_char_index(chars: &[char], query: &str) -> Option<usize> {
    let query = query.chars().collect::<Vec<_>>();
    if query.is_empty() || query.len() > chars.len() {
        return None;
    }
    (0..=chars.len() - query.len()).find(|&start| {
        chars[start..start + query.len()]
            .iter()
            .zip(&query)
            .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
    })
}

fn next_find_command(
    args: &FindArgs,
    source_filter: Option<SourceFilter>,
    next_offset: usize,
) -> String {
    let mut parts = vec!["mmr".to_string()];
    if let Some(source) = source_filter {
        parts.push(format!("--source {}", source.as_str()));
    }
    parts.push(format!("find {}", shell_quote(&args.query)));
    if let Some(project) = &args.project {
        parts.push(format!("--project {}", shell_quote(project)));
    }
    if args.all {
        parts.push("--all".to_string());
    }
    if let Some(session) = &args.session {
        parts.push(format!("--session {}", shell_quote(session)));
    }
    parts.push(format!("--limit {} --offset {next_offset}", args.limit));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_keeps_short_content_whole() {
        assert_eq!(
            snippet("Keep the retry limit at three.", "RETRY"),
            "Keep the retry limit at three."
        );
    }

    #[test]
    fn snippet_cuts_long_content_around_the_first_match() {
        let content = format!("{} needle {}", "a ".repeat(200), "b ".repeat(200));
        let text = snippet(&content, "NEEDLE");
        assert!(text.starts_with('…') && text.ends_with('…'), "{text}");
        assert!(text.contains("needle"));
        assert!(text.chars().count() < 2 * SNIPPET_CONTEXT_CHARS + 10);
    }

    #[test]
    fn match_char_index_ignores_case_across_multibyte_text() {
        let chars = "Ünïcode ÄBC".chars().collect::<Vec<_>>();
        assert_eq!(match_char_index(&chars, "äbc"), Some(8));
        assert_eq!(match_char_index(&chars, "zzz"), None);
    }
}
