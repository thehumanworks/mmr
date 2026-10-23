//! Find and retrieve command execution: matching, session ranking and continuation.
//!
//! Clap arguments and routing stay in the parent. Responses and algorithm helpers
//! are private here; only the two output handlers cross that boundary.

use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;

use super::{
    CliFailure, FindFormatArg, RetrieveArgs, SearchTextArgs, linked_project, serialize,
    shell_quote, source_filter_name, source_name,
};
use crate::messages::service::{MessageQueryOptions, QueryService};
use crate::store::{EventRecord, ProjectRecord, Store, content_hash};
use crate::types::{ApiMessage, SortBy, SortOptions, SortOrder, SourceFilter};

#[derive(Debug, Serialize)]
struct SearchResponse {
    query: String,
    total_results: usize,
    results: Vec<SearchResult>,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    project_id: String,
    source: String,
    session_id: String,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

pub(super) fn find_output(
    args: &SearchTextArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let response = search_response(args, source_filter)?;
    if args.format == FindFormatArg::Json {
        return serialize(&response, pretty);
    }

    let mut output = String::new();
    for result in response.results {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            result.citation, result.line_number, result.source, result.snippet
        ));
    }
    Ok(output)
}

fn search_response(
    args: &SearchTextArgs,
    source_filter: Option<SourceFilter>,
) -> Result<SearchResponse> {
    let mut store = Store::open_default()?;
    let project = linked_project(&store, args.project.as_deref())?;
    let results = search_project(
        &mut store,
        &project,
        source_filter,
        args.session.as_deref(),
        args.role.as_deref(),
        args.event_type.as_deref(),
        &args.query,
        args.ignore_case,
        args.context,
    )?;

    Ok(SearchResponse {
        query: args.query.clone(),
        total_results: results.len(),
        results,
    })
}

#[allow(clippy::too_many_arguments)]
fn search_project(
    store: &mut Store,
    project: &ProjectRecord,
    source_filter: Option<SourceFilter>,
    session: Option<&str>,
    role: Option<&str>,
    event_type: Option<&str>,
    query: &str,
    ignore_case: bool,
    context: usize,
) -> Result<Vec<SearchResult>> {
    if query.is_empty() {
        bail!("search query is empty");
    }

    let events =
        store.events_for_project(&project.id, source_filter_name(source_filter), session)?;
    let mut results = Vec::new();
    for event in events {
        if role.is_some_and(|role| role != event.role) {
            continue;
        }
        if event_type.is_some_and(|event_type| event_type != event.event_type) {
            continue;
        }

        let search_document = store.upsert_search_document(&event)?;
        for line_match in
            find_line_matches(&search_document.document_text, query, ignore_case, context)
        {
            results.push(SearchResult {
                project_id: event.project_id.clone(),
                source: event.source.clone(),
                session_id: event.session_id.clone(),
                event_id: event.id.clone(),
                event_type: event.event_type.clone(),
                role: event.role.clone(),
                timestamp: event.timestamp.clone(),
                citation: search_document.citation.clone(),
                line_number: line_match.line_number,
                snippet: line_match.snippet,
                before: line_match.before,
                after: line_match.after,
            });
        }
    }
    if source_filter.is_none() && session.is_none() {
        for memory in store.learned_memory_for_project(&project.id)? {
            if memory.status != "active" {
                continue;
            }
            if role.is_some_and(|role| role != "memory") {
                continue;
            }
            if event_type.is_some_and(|event_type| event_type != "learned_memory") {
                continue;
            }
            for line_match in find_line_matches(&memory.claim, query, ignore_case, context) {
                results.push(SearchResult {
                    project_id: memory.project_id.clone(),
                    source: "learned_memory".to_string(),
                    session_id: memory
                        .dream_run_id
                        .clone()
                        .unwrap_or_else(|| "learned_memory".to_string()),
                    event_id: memory.id.clone(),
                    event_type: "learned_memory".to_string(),
                    role: "memory".to_string(),
                    timestamp: memory.created_at.clone(),
                    citation: format!("mmr://learned-memory/{}", memory.id),
                    line_number: line_match.line_number,
                    snippet: line_match.snippet,
                    before: line_match.before,
                    after: line_match.after,
                });
            }
        }
    }

    results.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    Ok(results)
}

#[derive(Debug, Serialize)]
struct RetrieveResponse {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    debug: Option<RetrieveDebugOutput>,
    total_matches: usize,
    total_selected_sessions: usize,
    selected_sessions: Vec<RetrieveSelectedSession>,
    unreadable_matches: Vec<RetrieveUnreadableMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_command: Option<Option<String>>,
    suggested_next_action: Option<String>,
}

#[derive(Debug, Serialize)]
struct RetrieveDebugOutput {
    limits: RetrieveLimits,
    scope: RetrieveScopeOutput,
    total_ranked_sessions: usize,
}

#[derive(Debug, Clone, Serialize)]
struct RetrieveLimits {
    max_sessions: usize,
    before_messages: usize,
    after_messages: usize,
    max_messages_per_session: usize,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Serialize)]
struct RetrieveScopeOutput {
    all_projects: bool,
    all_sources: bool,
    source_filter: Option<String>,
    total_projects_searched: usize,
    projects: Vec<String>,
}

impl RetrieveLimits {
    fn from_args(args: &RetrieveArgs) -> Self {
        let limit = args.limit.unwrap_or_else(|| {
            args.max_sessions
                .saturating_mul(args.max_messages_per_session)
        });
        Self {
            max_sessions: args.max_sessions,
            before_messages: args.before_messages,
            after_messages: args.after_messages,
            max_messages_per_session: args.max_messages_per_session,
            limit,
            offset: args.offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetrieveSessionIdentity {
    source: String,
    project_name: String,
    source_session_id: String,
}

#[derive(Debug, Serialize)]
struct RetrieveSelectedSession {
    rank: usize,
    source: String,
    project_name: String,
    source_session_id: String,
    rank_reason: RetrieveRankReason,
    match_count: usize,
    first_match_citation: String,
    matches: Vec<RetrieveMatchOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_window: Option<RetrieveMessageWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<ApiMessage>>,
}

#[derive(Debug, Serialize)]
struct RetrieveRankReason {
    match_count: usize,
    latest_match_timestamp: String,
    tie_break: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RetrieveMessageWindow {
    before_messages: usize,
    after_messages: usize,
    max_messages_per_session: usize,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RetrieveMatchOutput {
    source: String,
    project_name: String,
    source_session_id: String,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RetrieveUnreadableMatch {
    source: String,
    project_name: String,
    source_session_id: Option<String>,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
    reason: String,
}

#[derive(Debug, Clone)]
struct RetrieveMatchRecord {
    identity: RetrieveSessionIdentity,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

impl RetrieveMatchRecord {
    fn output(&self) -> RetrieveMatchOutput {
        RetrieveMatchOutput {
            source: self.identity.source.clone(),
            project_name: self.identity.project_name.clone(),
            source_session_id: self.identity.source_session_id.clone(),
            event_id: public_retrieve_event_id(&self.event_id),
            event_type: self.event_type.clone(),
            role: self.role.clone(),
            timestamp: self.timestamp.clone(),
            citation: self.citation.clone(),
            line_number: self.line_number,
            snippet: self.snippet.clone(),
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }

    fn unreadable(&self, reason: &str) -> RetrieveUnreadableMatch {
        RetrieveUnreadableMatch {
            source: self.identity.source.clone(),
            project_name: self.identity.project_name.clone(),
            source_session_id: Some(self.identity.source_session_id.clone()),
            event_id: public_retrieve_event_id(&self.event_id),
            event_type: self.event_type.clone(),
            role: self.role.clone(),
            timestamp: self.timestamp.clone(),
            citation: self.citation.clone(),
            line_number: self.line_number,
            snippet: self.snippet.clone(),
            before: self.before.clone(),
            after: self.after.clone(),
            reason: reason.to_string(),
        }
    }
}

#[derive(Debug)]
struct RetrieveSearchMatches {
    event_matches: Vec<RetrieveMatchRecord>,
    learned_matches: Vec<RetrieveUnreadableMatch>,
}

#[derive(Debug)]
struct RetrieveWindow {
    messages: Vec<ApiMessage>,
    truncated: bool,
}

#[derive(Debug)]
struct RetrieveRankedSession {
    identity: RetrieveSessionIdentity,
    matches: Vec<RetrieveMatchRecord>,
    match_count: usize,
    latest_match_timestamp: String,
}

#[derive(Debug)]
struct RetrieveSessionCandidate {
    identity: RetrieveSessionIdentity,
    selected: RetrieveSelectedSession,
    message_window: Option<RetrieveMessageWindow>,
    message_history: Vec<ApiMessage>,
    match_count: usize,
}

pub(super) fn retrieve_output(
    args: &RetrieveArgs,
    service: &QueryService,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let limits = RetrieveLimits::from_args(args);
    if args.query.is_empty() {
        return Err(anyhow::Error::new(retrieve_cli_failure(
            "invalid_query",
            "retrieve query is empty",
            pretty,
        )?));
    }

    let pinned_sessions = parse_retrieve_pinned_sessions(args, pretty)?;
    let mut store = Store::open_default()?;
    let source_filter = retrieve_effective_source_filter(args, cli_source, source_filter, pretty)?;
    let store_projects = retrieve_scope_projects(&store, args, pretty)?;
    let matches = if args.all_projects {
        retrieve_all_projects_matches(service, &mut store, &store_projects, source_filter, args)?
    } else {
        retrieve_search_matches(&mut store, &store_projects, source_filter, args)?
    };
    let total_matches = matches.event_matches.len() + matches.learned_matches.len();

    let mut grouped = group_retrieve_matches(matches.event_matches);
    if !pinned_sessions.is_empty() {
        let pinned_set = pinned_sessions.iter().cloned().collect::<BTreeSet<_>>();
        for identity in &pinned_sessions {
            if !grouped.contains_key(identity) {
                return Err(anyhow::Error::new(retrieve_cli_failure(
                    "pinned_session_not_found",
                    format!(
                        "pinned session {}:{}:{} is not present in the current retrieve scope",
                        identity.source, identity.project_name, identity.source_session_id
                    ),
                    pretty,
                )?));
            }
        }
        grouped.retain(|identity, _| pinned_set.contains(identity));
    }

    let mut unreadable_matches = matches.learned_matches;
    let mut ranked_sessions = Vec::new();
    for (identity, mut session_matches) in grouped {
        sort_retrieve_matches(&mut session_matches);
        let Some(provider_source) = source_filter_from_name(identity.source.as_str()) else {
            unreadable_matches.extend(
                session_matches
                    .iter()
                    .map(|item| item.unreadable("unsupported_source")),
            );
            continue;
        };

        if !retrieve_provider_session_exists(service, &identity, provider_source) {
            unreadable_matches.extend(
                session_matches
                    .iter()
                    .map(|item| item.unreadable("provider_transcript_not_found")),
            );
            continue;
        }

        ranked_sessions.push(build_retrieve_ranked_session(identity, session_matches));
    }

    ranked_sessions.sort_by(|left, right| {
        right
            .match_count
            .cmp(&left.match_count)
            .then_with(|| {
                right
                    .latest_match_timestamp
                    .cmp(&left.latest_match_timestamp)
            })
            .then_with(|| left.identity.source.cmp(&right.identity.source))
            .then_with(|| left.identity.project_name.cmp(&right.identity.project_name))
            .then_with(|| {
                left.identity
                    .source_session_id
                    .cmp(&right.identity.source_session_id)
            })
    });

    let total_ranked_sessions = ranked_sessions.len();
    ranked_sessions.truncate(limits.max_sessions);
    let mut candidates = Vec::new();
    for ranked_session in ranked_sessions {
        let provider_messages = if args.full_message_history {
            let provider_source = source_filter_from_name(ranked_session.identity.source.as_str())
                .expect("ranked retrieve session has supported source");
            Some(
                retrieve_provider_messages(service, &ranked_session.identity, provider_source)
                    .unwrap_or_default(),
            )
        } else {
            None
        };
        candidates.push(build_retrieve_session_candidate(
            args,
            &limits,
            ranked_session,
            provider_messages,
        ));
    }
    assign_retrieve_ranks(&mut candidates);
    let (next_page, next_offset) = if args.full_message_history {
        apply_retrieve_flattened_pagination(&mut candidates, &limits)
    } else {
        (false, limits.offset)
    };
    let selected_identities = candidates
        .iter()
        .map(|candidate| candidate.identity.clone())
        .collect::<Vec<_>>();
    let next_command = if args.full_message_history && next_page {
        Some(build_retrieve_next_command(
            args,
            &limits,
            source_filter,
            &selected_identities,
            next_offset,
        )?)
    } else {
        None
    };
    let debug = if args.debug {
        let scope_projects =
            retrieve_scope_project_names(service, &store_projects, source_filter, args);
        Some(RetrieveDebugOutput {
            limits: limits.clone(),
            scope: RetrieveScopeOutput {
                all_projects: args.all_projects,
                all_sources: source_filter.is_none(),
                source_filter: source_filter.map(source_name).map(str::to_string),
                total_projects_searched: scope_projects.len(),
                projects: scope_projects,
            },
            total_ranked_sessions,
        })
    } else {
        None
    };
    let selected_sessions = candidates
        .into_iter()
        .map(|mut candidate| {
            if args.full_message_history {
                candidate.selected.message_window = candidate.message_window;
                candidate.selected.messages = Some(candidate.message_history);
            }
            candidate.selected
        })
        .collect::<Vec<_>>();

    let response = RetrieveResponse {
        query: args.query.clone(),
        debug,
        total_matches,
        total_selected_sessions: selected_sessions.len(),
        selected_sessions,
        unreadable_matches,
        next_page: args.full_message_history.then_some(next_page),
        next_offset: args.full_message_history.then_some(next_offset),
        next_command: args.full_message_history.then_some(next_command),
        suggested_next_action: suggested_retrieve_next_action(total_matches),
    };
    serialize(&response, pretty)
}

fn retrieve_effective_source_filter(
    args: &RetrieveArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<Option<SourceFilter>> {
    if args.all_sources && cli_source.is_some() {
        return Err(anyhow::Error::new(retrieve_cli_failure(
            "invalid_scope_flags",
            "--all-sources cannot be combined with --source",
            pretty,
        )?));
    }
    if args.all_sources {
        Ok(None)
    } else {
        Ok(source_filter)
    }
}

fn retrieve_scope_projects(
    store: &Store,
    args: &RetrieveArgs,
    pretty: bool,
) -> Result<Vec<ProjectRecord>> {
    if args.all_projects && args.project.is_some() {
        return Err(anyhow::Error::new(retrieve_cli_failure(
            "invalid_scope_flags",
            "--all-projects cannot be combined with --project",
            pretty,
        )?));
    }
    if args.all_projects {
        return store.projects();
    }
    Ok(vec![linked_project(store, args.project.as_deref())?])
}

fn retrieve_scope_project_names(
    service: &QueryService,
    store_projects: &[ProjectRecord],
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Vec<String> {
    if !args.all_projects {
        return store_projects
            .iter()
            .map(|project| project.canonical_path.clone())
            .collect();
    }

    let mut projects = service
        .projects(
            source_filter,
            None,
            0,
            SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
        )
        .projects
        .into_iter()
        .map(|project| project.name)
        .collect::<Vec<_>>();
    projects.extend(
        store_projects
            .iter()
            .map(|project| project.canonical_path.clone()),
    );
    projects.sort();
    projects.dedup();
    projects
}

fn parse_retrieve_pinned_sessions(
    args: &RetrieveArgs,
    pretty: bool,
) -> Result<Vec<RetrieveSessionIdentity>> {
    let mut identities = Vec::new();
    for raw in &args.pinned_sessions {
        let identity = serde_json::from_str::<RetrieveSessionIdentity>(raw).map_err(|error| {
            anyhow::Error::new(
                retrieve_cli_failure(
                    "invalid_pinned_session",
                    format!("invalid pinned session JSON: {error}"),
                    pretty,
                )
                .expect("serialize retrieve pinned error"),
            )
        })?;
        if identity.source.trim().is_empty()
            || identity.project_name.trim().is_empty()
            || identity.source_session_id.trim().is_empty()
            || source_filter_from_name(identity.source.as_str()).is_none()
        {
            return Err(anyhow::Error::new(retrieve_cli_failure(
                "invalid_pinned_session",
                "pinned session must include source, project_name, and source_session_id",
                pretty,
            )?));
        }
        identities.push(identity);
    }
    Ok(identities)
}

fn retrieve_cli_failure(
    error_kind: &str,
    message: impl Into<String>,
    pretty: bool,
) -> Result<CliFailure> {
    let message = message.into();
    let value = serde_json::json!({
        "error_kind": error_kind,
        "message": message,
    });
    Ok(CliFailure::new(2, serialize(&value, pretty)?, message))
}

fn retrieve_all_projects_matches(
    service: &QueryService,
    store: &mut Store,
    store_projects: &[ProjectRecord],
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Result<RetrieveSearchMatches> {
    let mut event_matches = retrieve_provider_message_matches(service, source_filter, args)?;
    let mut store_matches = retrieve_search_matches(store, store_projects, source_filter, args)?;

    let provider_identities = event_matches
        .iter()
        .map(|item| item.identity.clone())
        .collect::<BTreeSet<_>>();
    for item in store_matches.event_matches.drain(..) {
        if provider_identities.contains(&item.identity) {
            continue;
        }
        store_matches
            .learned_matches
            .push(item.unreadable("provider_transcript_not_found"));
    }

    sort_retrieve_matches(&mut event_matches);
    store_matches.learned_matches.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    Ok(RetrieveSearchMatches {
        event_matches,
        learned_matches: store_matches.learned_matches,
    })
}

fn retrieve_provider_message_matches(
    service: &QueryService,
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Result<Vec<RetrieveMatchRecord>> {
    if args
        .event_type
        .as_deref()
        .is_some_and(|event_type| event_type != "message")
    {
        return Ok(Vec::new());
    }

    let session_ids = args.session.iter().cloned().collect::<Vec<_>>();
    let response = service.messages(
        &session_ids,
        None,
        source_filter,
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let matches = response
        .messages
        .par_iter()
        .flat_map(|message| {
            if args
                .role
                .as_deref()
                .is_some_and(|role| role != message.role)
            {
                return Vec::new();
            }
            find_line_matches(
                &message.content,
                &args.query,
                args.ignore_case,
                args.context,
            )
            .into_iter()
            .map(|line_match| retrieve_match_from_provider_message(message, line_match))
            .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Ok(matches)
}

fn retrieve_search_matches(
    store: &mut Store,
    projects: &[ProjectRecord],
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Result<RetrieveSearchMatches> {
    let mut event_matches = Vec::new();
    for project in projects {
        let events = store.events_for_project(
            &project.id,
            source_filter_name(source_filter),
            args.session.as_deref(),
        )?;
        for event in events {
            if args.role.as_deref().is_some_and(|role| role != event.role) {
                continue;
            }
            if args
                .event_type
                .as_deref()
                .is_some_and(|event_type| event_type != event.event_type)
            {
                continue;
            }

            let search_document = store.upsert_search_document(&event)?;
            for line_match in find_line_matches(
                &search_document.document_text,
                &args.query,
                args.ignore_case,
                args.context,
            ) {
                event_matches.push(retrieve_match_from_event(
                    project,
                    &event,
                    &search_document.citation,
                    line_match,
                ));
            }
        }
    }

    let mut learned_matches = Vec::new();
    if source_filter.is_none() && args.session.is_none() {
        for project in projects {
            for memory in store.learned_memory_for_project(&project.id)? {
                if memory.status != "active" {
                    continue;
                }
                if args.role.as_deref().is_some_and(|role| role != "memory") {
                    continue;
                }
                if args
                    .event_type
                    .as_deref()
                    .is_some_and(|event_type| event_type != "learned_memory")
                {
                    continue;
                }
                for line_match in
                    find_line_matches(&memory.claim, &args.query, args.ignore_case, args.context)
                {
                    learned_matches.push(RetrieveUnreadableMatch {
                        source: "learned_memory".to_string(),
                        project_name: project.canonical_path.clone(),
                        source_session_id: memory.dream_run_id.clone(),
                        event_id: memory.id.clone(),
                        event_type: "learned_memory".to_string(),
                        role: "memory".to_string(),
                        timestamp: memory.created_at.clone(),
                        citation: format!("mmr://learned-memory/{}", memory.id),
                        line_number: line_match.line_number,
                        snippet: line_match.snippet,
                        before: line_match.before,
                        after: line_match.after,
                        reason: "learned_memory_match".to_string(),
                    });
                }
            }
        }
    }

    sort_retrieve_matches(&mut event_matches);
    learned_matches.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    Ok(RetrieveSearchMatches {
        event_matches,
        learned_matches,
    })
}

fn retrieve_match_from_event(
    project: &ProjectRecord,
    event: &EventRecord,
    citation: &str,
    line_match: LineMatch,
) -> RetrieveMatchRecord {
    RetrieveMatchRecord {
        identity: RetrieveSessionIdentity {
            source: event.source.clone(),
            project_name: project.canonical_path.clone(),
            source_session_id: event.source_session_id.clone(),
        },
        event_id: event.id.clone(),
        event_type: event.event_type.clone(),
        role: event.role.clone(),
        timestamp: event.timestamp.clone(),
        citation: citation.to_string(),
        line_number: line_match.line_number,
        snippet: line_match.snippet,
        before: line_match.before,
        after: line_match.after,
    }
}

fn retrieve_match_from_provider_message(
    message: &ApiMessage,
    line_match: LineMatch,
) -> RetrieveMatchRecord {
    let event_id = provider_message_event_id(message, line_match.line_number);
    RetrieveMatchRecord {
        identity: RetrieveSessionIdentity {
            source: message.source.clone(),
            project_name: message.project_name.clone(),
            source_session_id: message.session_id.clone(),
        },
        event_id: event_id.clone(),
        event_type: "message".to_string(),
        role: message.role.clone(),
        timestamp: message.timestamp.clone(),
        citation: format!("mmr://message/{event_id}"),
        line_number: line_match.line_number,
        snippet: line_match.snippet,
        before: line_match.before,
        after: line_match.after,
    }
}

fn provider_message_event_id(message: &ApiMessage, line_number: usize) -> String {
    let hash = content_hash(&format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        message.source,
        message.project_name,
        message.session_id,
        message.role,
        message.timestamp,
        line_number,
        message.content
    ));
    format!(
        "message:v1:{}",
        hash.strip_prefix("sha256:").unwrap_or(hash.as_str())
    )
}

fn group_retrieve_matches(
    matches: Vec<RetrieveMatchRecord>,
) -> BTreeMap<RetrieveSessionIdentity, Vec<RetrieveMatchRecord>> {
    let mut grouped: BTreeMap<RetrieveSessionIdentity, Vec<RetrieveMatchRecord>> = BTreeMap::new();
    for item in matches {
        grouped.entry(item.identity.clone()).or_default().push(item);
    }
    grouped
}

fn sort_retrieve_matches(matches: &mut [RetrieveMatchRecord]) {
    matches.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
}

fn retrieve_provider_messages(
    service: &QueryService,
    identity: &RetrieveSessionIdentity,
    source: SourceFilter,
) -> Result<Vec<ApiMessage>> {
    record_retrieve_window_load(identity);
    let response = service.messages(
        std::slice::from_ref(&identity.source_session_id),
        Some(identity.project_name.as_str()),
        Some(source),
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    Ok(response.messages)
}

fn retrieve_provider_session_exists(
    service: &QueryService,
    identity: &RetrieveSessionIdentity,
    source: SourceFilter,
) -> bool {
    let Ok(response) = service.sessions(
        Some(identity.project_name.as_str()),
        Some(source),
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    ) else {
        return false;
    };
    response.sessions.iter().any(|session| {
        session.source == identity.source && session.session_id == identity.source_session_id
    })
}

fn record_retrieve_window_load(identity: &RetrieveSessionIdentity) {
    let Ok(path) = std::env::var("MMR_TEST_RETRIEVE_WINDOW_LOAD_LOG") else {
        return;
    };
    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(
        file,
        "{}\t{}\t{}",
        identity.source, identity.project_name, identity.source_session_id
    );
}

fn build_retrieve_ranked_session(
    identity: RetrieveSessionIdentity,
    matches: Vec<RetrieveMatchRecord>,
) -> RetrieveRankedSession {
    let latest_match_timestamp = matches
        .iter()
        .map(|item| item.timestamp.as_str())
        .max()
        .unwrap_or_default()
        .to_string();
    RetrieveRankedSession {
        identity,
        match_count: matches.len(),
        latest_match_timestamp,
        matches,
    }
}

fn build_retrieve_session_candidate(
    args: &RetrieveArgs,
    limits: &RetrieveLimits,
    ranked_session: RetrieveRankedSession,
    provider_messages: Option<Vec<ApiMessage>>,
) -> RetrieveSessionCandidate {
    let identity = ranked_session.identity;
    let matches = ranked_session.matches;
    let latest_match_timestamp = ranked_session.latest_match_timestamp;
    let rank_reason = RetrieveRankReason {
        match_count: ranked_session.match_count,
        latest_match_timestamp: latest_match_timestamp.clone(),
        tie_break: vec![
            identity.source.clone(),
            identity.project_name.clone(),
            identity.source_session_id.clone(),
        ],
    };
    let first_match_citation = matches
        .first()
        .map(|item| item.citation.clone())
        .unwrap_or_default();
    let (message_history, message_window) = if let Some(provider_messages) = provider_messages {
        let RetrieveWindow {
            messages,
            truncated,
        } = build_retrieve_message_window(args, limits, &matches, provider_messages);
        (
            messages,
            Some(RetrieveMessageWindow {
                before_messages: limits.before_messages,
                after_messages: limits.after_messages,
                max_messages_per_session: limits.max_messages_per_session,
                truncated,
            }),
        )
    } else {
        (Vec::new(), None)
    };
    let selected = RetrieveSelectedSession {
        rank: 0,
        source: identity.source.clone(),
        project_name: identity.project_name.clone(),
        source_session_id: identity.source_session_id.clone(),
        rank_reason,
        match_count: ranked_session.match_count,
        first_match_citation,
        matches: matches.iter().map(RetrieveMatchRecord::output).collect(),
        message_window: None,
        messages: None,
    };

    RetrieveSessionCandidate {
        identity,
        selected,
        message_window,
        message_history,
        match_count: ranked_session.match_count,
    }
}

fn assign_retrieve_ranks(candidates: &mut [RetrieveSessionCandidate]) {
    for (idx, candidate) in candidates.iter_mut().enumerate() {
        candidate.selected.rank = idx + 1;
        candidate.selected.match_count = candidate.match_count;
    }
}

fn apply_retrieve_flattened_pagination(
    candidates: &mut [RetrieveSessionCandidate],
    limits: &RetrieveLimits,
) -> (bool, usize) {
    let full_messages = candidates
        .iter()
        .map(|candidate| candidate.message_history.clone())
        .collect::<Vec<_>>();
    let flattened = full_messages
        .iter()
        .enumerate()
        .flat_map(|(session_idx, messages)| {
            messages
                .iter()
                .enumerate()
                .map(move |(message_idx, _)| (session_idx, message_idx))
        })
        .collect::<Vec<_>>();
    let total = flattened.len();
    let page_start = limits.offset.min(total);
    let page_end = page_start.saturating_add(limits.limit).min(total);

    for candidate in candidates.iter_mut() {
        candidate.message_history.clear();
    }
    for (session_idx, message_idx) in &flattened[page_start..page_end] {
        if let Some(message) = full_messages
            .get(*session_idx)
            .and_then(|messages| messages.get(*message_idx))
        {
            candidates[*session_idx]
                .message_history
                .push(message.clone());
        }
    }

    let next_page = limits.limit > 0 && page_end < total;
    let next_offset = if next_page { page_end } else { limits.offset };
    (next_page, next_offset)
}

fn build_retrieve_message_window(
    args: &RetrieveArgs,
    limits: &RetrieveLimits,
    matches: &[RetrieveMatchRecord],
    messages: Vec<ApiMessage>,
) -> RetrieveWindow {
    if messages.is_empty() {
        return RetrieveWindow {
            messages,
            truncated: false,
        };
    }

    let mut anchors = BTreeSet::new();
    for item in matches {
        anchors.insert(find_retrieve_anchor_index(
            &messages,
            item,
            &args.query,
            args.ignore_case,
        ));
    }
    if anchors.is_empty() {
        anchors.insert(0);
    }

    let mut window_indices = BTreeSet::new();
    for anchor in &anchors {
        let start = anchor.saturating_sub(limits.before_messages);
        let end = anchor
            .saturating_add(limits.after_messages)
            .saturating_add(1)
            .min(messages.len());
        for idx in start..end {
            window_indices.insert(idx);
        }
    }

    let raw_len = window_indices.len();
    let mut capped_indices = BTreeSet::new();
    for anchor in &anchors {
        if capped_indices.len() >= limits.max_messages_per_session {
            break;
        }
        if window_indices.contains(anchor) {
            capped_indices.insert(*anchor);
        }
    }
    for idx in window_indices {
        if capped_indices.len() >= limits.max_messages_per_session {
            break;
        }
        capped_indices.insert(idx);
    }

    let truncated = raw_len > capped_indices.len();
    let selected = capped_indices
        .into_iter()
        .filter_map(|idx| messages.get(idx).cloned())
        .collect::<Vec<_>>();
    RetrieveWindow {
        messages: selected,
        truncated,
    }
}

fn find_retrieve_anchor_index(
    messages: &[ApiMessage],
    item: &RetrieveMatchRecord,
    query: &str,
    ignore_case: bool,
) -> usize {
    if let Some((idx, _)) = messages.iter().enumerate().find(|(_, message)| {
        message.timestamp == item.timestamp
            && contains_retrieve_text(&message.content, query, ignore_case)
    }) {
        return idx;
    }
    if let Some((idx, _)) = messages
        .iter()
        .enumerate()
        .find(|(_, message)| message.timestamp == item.timestamp)
    {
        return idx;
    }
    if let Some((idx, _)) = messages
        .iter()
        .enumerate()
        .find(|(_, message)| contains_retrieve_text(&message.content, query, ignore_case))
    {
        return idx;
    }
    messages
        .iter()
        .enumerate()
        .find(|(_, message)| message.timestamp >= item.timestamp)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| messages.len().saturating_sub(1))
}

fn contains_retrieve_text(haystack: &str, needle: &str, ignore_case: bool) -> bool {
    if ignore_case {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    } else {
        haystack.contains(needle)
    }
}

fn build_retrieve_next_command(
    args: &RetrieveArgs,
    limits: &RetrieveLimits,
    source_filter: Option<SourceFilter>,
    identities: &[RetrieveSessionIdentity],
    next_offset: usize,
) -> Result<String> {
    let mut parts = vec!["mmr".to_string()];
    if let Some(source) = source_filter {
        parts.push("--source".to_string());
        parts.push(source_name(source).to_string());
    }
    parts.push("retrieve".to_string());
    parts.push(shell_quote(&args.query));
    if args.debug {
        parts.push("--debug".to_string());
    }
    if args.full_message_history {
        parts.push("--full-message-history".to_string());
    }
    if args.all_sources {
        parts.push("--all-sources".to_string());
    }

    if args.all_projects {
        parts.push("--all-projects".to_string());
    } else if let Some(project) = args
        .project
        .as_ref()
        .and_then(|path| path.to_str().map(str::to_string))
        .or_else(|| {
            identities
                .first()
                .map(|identity| identity.project_name.clone())
        })
    {
        parts.push("--project".to_string());
        parts.push(shell_quote(&project));
    }
    if let Some(session) = &args.session {
        parts.push("--session".to_string());
        parts.push(shell_quote(session));
    }
    if let Some(role) = &args.role {
        parts.push("--role".to_string());
        parts.push(shell_quote(role));
    }
    if let Some(event_type) = &args.event_type {
        parts.push("--event-type".to_string());
        parts.push(shell_quote(event_type));
    }
    if args.ignore_case {
        parts.push("--ignore-case".to_string());
    }
    if args.context > 0 {
        parts.push("-C".to_string());
        parts.push(args.context.to_string());
    }
    parts.push("--max-sessions".to_string());
    parts.push(limits.max_sessions.to_string());
    parts.push("--before-messages".to_string());
    parts.push(limits.before_messages.to_string());
    parts.push("--after-messages".to_string());
    parts.push(limits.after_messages.to_string());
    parts.push("--max-messages-per-session".to_string());
    parts.push(limits.max_messages_per_session.to_string());
    parts.push("--limit".to_string());
    parts.push(limits.limit.to_string());
    parts.push("--offset".to_string());
    parts.push(next_offset.to_string());

    for identity in identities {
        parts.push("--pinned-session".to_string());
        parts.push(shell_quote(&serde_json::to_string(identity)?));
    }

    Ok(parts.join(" "))
}

fn source_filter_from_name(source: &str) -> Option<SourceFilter> {
    match source {
        "claude" => Some(SourceFilter::Claude),
        "codex" => Some(SourceFilter::Codex),
        "cursor" => Some(SourceFilter::Cursor),
        "grok" => Some(SourceFilter::Grok),
        "pi" => Some(SourceFilter::Pi),
        _ => None,
    }
}

fn public_retrieve_event_id(event_id: &str) -> String {
    event_id
        .strip_prefix("evt:v1:")
        .map(|suffix| format!("event:v1:{suffix}"))
        .unwrap_or_else(|| event_id.to_string())
}

fn suggested_retrieve_next_action(total_matches: usize) -> Option<String> {
    if total_matches == 0 {
        Some(
            "No normalized matches found. Try --ignore-case, a shorter query, or mmr find for raw match diagnostics."
                .to_string(),
        )
    } else {
        None
    }
}

#[derive(Debug)]
struct LineMatch {
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

fn find_line_matches(
    document_text: &str,
    query: &str,
    ignore_case: bool,
    context: usize,
) -> Vec<LineMatch> {
    let lines = document_text.lines().collect::<Vec<_>>();
    let query_cmp = if ignore_case {
        query.to_lowercase()
    } else {
        query.to_string()
    };
    let mut matches = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let line_cmp = if ignore_case {
            line.to_lowercase()
        } else {
            (*line).to_string()
        };
        if !line_cmp.contains(&query_cmp) {
            continue;
        }
        let before_start = idx.saturating_sub(context);
        let after_end = (idx + 1 + context).min(lines.len());
        matches.push(LineMatch {
            line_number: idx + 1,
            snippet: truncate_snippet(line),
            before: lines[before_start..idx]
                .iter()
                .map(|line| truncate_snippet(line))
                .collect(),
            after: lines[idx + 1..after_end]
                .iter()
                .map(|line| truncate_snippet(line))
                .collect(),
        });
    }

    matches
}

fn truncate_snippet(line: &str) -> String {
    const MAX_SNIPPET_CHARS: usize = 500;
    let mut snippet = line.chars().take(MAX_SNIPPET_CHARS).collect::<String>();
    if line.chars().count() > MAX_SNIPPET_CHARS {
        snippet.push_str("...");
    }
    snippet
}
