use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::source;
use crate::teleport::project_aliases;
use crate::types::query::{
    PreviewCandidate, ProjectAggregate, ProjectAggregateState, ResolvedProject, SessionAggregate,
    SessionAggregateState,
};
use crate::types::{
    ApiMessage, ApiMessagesResponse, ApiProject, ApiProjectsResponse, ApiSession,
    ApiSessionsResponse, MessageRecord, SortBy, SortOptions, SortOrder, SourceFilter, SourceKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageIndexRange {
    pub from: Option<usize>,
    pub to: Option<usize>,
}

impl MessageIndexRange {
    pub fn new(from: Option<usize>, to: Option<usize>) -> Option<Self> {
        if from.is_none() && to.is_none() {
            None
        } else {
            Some(Self { from, to })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageQueryOptions {
    pub limit: Option<usize>,
    pub offset: usize,
    pub sort: SortOptions,
    pub message_index_range: Option<MessageIndexRange>,
}

impl MessageQueryOptions {
    pub fn new(limit: Option<usize>, offset: usize, sort: SortOptions) -> Self {
        Self {
            limit,
            offset,
            sort,
            message_index_range: None,
        }
    }

    pub fn with_message_index_range(
        mut self,
        message_index_range: Option<MessageIndexRange>,
    ) -> Self {
        self.message_index_range = message_index_range;
        self
    }
}

#[derive(Debug)]
pub struct TeleportSessionContext {
    pub session: ApiSession,
    pub source_file: PathBuf,
}

#[derive(Debug)]
pub struct QueryService {
    messages: Vec<MessageRecord>,
    projects: Vec<ProjectAggregate>,
    sessions: Vec<SessionAggregate>,
}

impl QueryService {
    pub fn load() -> Result<Self> {
        let messages = source::load_messages()?;
        Ok(Self::from_messages(messages))
    }

    fn from_messages(messages: Vec<MessageRecord>) -> Self {
        let mut project_map: HashMap<(SourceKind, String), ProjectAggregateState> = HashMap::new();
        let mut session_map: HashMap<(SourceKind, String, String), SessionAggregateState> =
            HashMap::new();

        for message in &messages {
            let project_key = (message.source, message.project_name.clone());
            let project_state =
                project_map
                    .entry(project_key)
                    .or_insert_with(|| ProjectAggregateState {
                        source: message.source,
                        name: message.project_name.clone(),
                        original_path: message.project_path.clone(),
                        last_activity: message.timestamp.clone(),
                        message_count: 0,
                        session_ids: HashSet::new(),
                    });

            project_state.message_count += 1;
            project_state.session_ids.insert(message.session_id.clone());
            if project_state.original_path.is_empty() && !message.project_path.is_empty() {
                project_state.original_path = message.project_path.clone();
            }
            if message.timestamp > project_state.last_activity {
                project_state.last_activity = message.timestamp.clone();
            }

            let session_key = (
                message.source,
                message.project_name.clone(),
                message.session_id.clone(),
            );
            let session_state =
                session_map
                    .entry(session_key)
                    .or_insert_with(|| SessionAggregateState {
                        project_name: message.project_name.clone(),
                        project_path: message.project_path.clone(),
                        source: message.source,
                        session_id: message.session_id.clone(),
                        first_timestamp: message.timestamp.clone(),
                        last_timestamp: message.timestamp.clone(),
                        message_count: 0,
                        user_messages: 0,
                        assistant_messages: 0,
                        preview: None,
                    });

            session_state.message_count += 1;
            if session_state.project_path.is_empty() && !message.project_path.is_empty() {
                session_state.project_path = message.project_path.clone();
            }
            if message.timestamp < session_state.first_timestamp {
                session_state.first_timestamp = message.timestamp.clone();
            }
            if message.timestamp > session_state.last_timestamp {
                session_state.last_timestamp = message.timestamp.clone();
            }

            if message.role == "user" {
                session_state.user_messages += 1;
                let candidate = PreviewCandidate {
                    timestamp: message.timestamp.clone(),
                    source_file: message.source_file.clone(),
                    line_index: message.line_index,
                    content: message.content.clone(),
                };
                let should_replace = session_state
                    .preview
                    .as_ref()
                    .map(|current| preview_cmp(&candidate, current) == Ordering::Less)
                    .unwrap_or(true);
                if should_replace {
                    session_state.preview = Some(candidate);
                }
            }

            if message.role == "assistant" {
                session_state.assistant_messages += 1;
            }
        }

        let projects = project_map
            .into_values()
            .map(|state| ProjectAggregate {
                name: state.name,
                source: state.source,
                original_path: state.original_path,
                session_count: state.session_ids.len() as i32,
                message_count: state.message_count,
                last_activity: state.last_activity,
            })
            .collect::<Vec<_>>();

        let sessions = session_map
            .into_values()
            .map(|state| SessionAggregate {
                project_name: state.project_name,
                project_path: state.project_path,
                source: state.source,
                session_id: state.session_id,
                first_timestamp: state.first_timestamp,
                last_timestamp: state.last_timestamp,
                message_count: state.message_count,
                user_messages: state.user_messages,
                assistant_messages: state.assistant_messages,
                preview: truncate_preview(
                    &state
                        .preview
                        .map(|preview| preview.content)
                        .unwrap_or_default(),
                    120,
                ),
            })
            .collect::<Vec<_>>();

        Self {
            messages,
            projects,
            sessions,
        }
    }

    pub fn projects(
        &self,
        source_filter: Option<SourceFilter>,
        limit: Option<usize>,
        offset: usize,
        sort: SortOptions,
    ) -> ApiProjectsResponse {
        let mut filtered = self
            .projects
            .iter()
            .filter(|project| matches_source_filter(project.source, source_filter))
            .cloned()
            .collect::<Vec<_>>();
        sort_projects(&mut filtered, sort.by, sort.order);

        let projects = apply_pagination(filtered, limit, offset)
            .into_iter()
            .map(|project| ApiProject {
                name: project.name.clone(),
                source: project.source.as_str().to_string(),
                original_path: project.original_path.clone(),
                aliases: project_lookup_aliases(&project.name, &project.original_path),
                session_count: project.session_count,
                message_count: project.message_count,
                last_activity: project.last_activity,
            })
            .collect::<Vec<_>>();

        let total_messages = self
            .messages
            .iter()
            .filter(|message| matches_source_filter(message.source, source_filter))
            .count() as i64;
        let total_sessions = self
            .sessions
            .iter()
            .filter(|session| matches_source_filter(session.source, source_filter))
            .count() as i64;

        ApiProjectsResponse {
            projects,
            total_messages,
            total_sessions,
        }
    }

    pub fn sessions(
        &self,
        project: Option<&str>,
        source_filter: Option<SourceFilter>,
        limit: Option<usize>,
        offset: usize,
        sort: SortOptions,
    ) -> Result<ApiSessionsResponse> {
        let resolved_project = match project {
            Some(project) => Some(resolve_project(&self.projects, source_filter, project)?),
            None => None,
        };

        let mut filtered = self
            .sessions
            .iter()
            .filter(|session| {
                matches_source_filter(session.source, source_filter)
                    && matches_project_filter(session, resolved_project.as_ref())
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_sessions(&mut filtered, sort.by, sort.order);

        let total_sessions = filtered.len() as i64;

        let sessions = apply_pagination(filtered, limit, offset)
            .into_iter()
            .map(|session| ApiSession {
                session_id: session.session_id,
                source: session.source.as_str().to_string(),
                project_name: session.project_name,
                project_path: session.project_path,
                first_timestamp: session.first_timestamp,
                last_timestamp: session.last_timestamp,
                message_count: session.message_count,
                user_messages: session.user_messages,
                assistant_messages: session.assistant_messages,
                preview: session.preview,
            })
            .collect::<Vec<_>>();

        Ok(ApiSessionsResponse {
            sessions,
            total_sessions,
        })
    }

    pub fn messages(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
        source_filter: Option<SourceFilter>,
        options: MessageQueryOptions,
    ) -> Result<ApiMessagesResponse> {
        let resolved_project = match project {
            Some(project) => Some(resolve_project(&self.projects, source_filter, project)?),
            None => None,
        };

        let mut filtered = self
            .messages
            .iter()
            .filter(|message| {
                matches_source_filter(message.source, source_filter)
                    && matches_session_filter(message.session_id.as_str(), session_id)
                    && matches_message_project_filter(message, resolved_project.as_ref())
            })
            .cloned()
            .collect::<Vec<_>>();
        let total_messages = filtered.len() as i64;
        let session_message_counts = build_session_message_counts(&filtered);
        sort_messages(
            &mut filtered,
            options.sort.by,
            options.sort.order,
            &session_message_counts,
        );
        let selected_total = options
            .message_index_range
            .map(|range| message_index_range_len(filtered.len(), range))
            .unwrap_or(filtered.len());
        let filtered = apply_message_index_range(filtered, options.message_index_range);

        let paged = if options.sort.by == SortBy::Timestamp && options.sort.order == SortOrder::Asc
        {
            // Preserve the historical "newest window, then chronological output" behavior.
            let descending = filtered.into_iter().rev().collect::<Vec<_>>();
            let mut paged = apply_pagination(descending, options.limit, options.offset);
            paged.reverse();
            paged
        } else {
            apply_pagination(filtered, options.limit, options.offset)
        };

        let page_size = paged.len();
        let next_offset = (options.offset + page_size) as i64;
        let next_page = options.limit.is_some() && next_offset < selected_total as i64;

        let messages = paged
            .into_iter()
            .map(|message| ApiMessage {
                session_id: message.session_id,
                source: message.source.as_str().to_string(),
                project_name: message.project_name,
                role: message.role,
                content: message.content,
                model: message.model,
                timestamp: message.timestamp,
                is_subagent: message.is_subagent,
                msg_type: message.msg_type,
                input_tokens: message.input_tokens,
                output_tokens: message.output_tokens,
            })
            .collect();

        Ok(ApiMessagesResponse {
            messages,
            total_messages,
            next_page,
            next_offset,
            next_command: None,
        })
    }

    pub fn resolve_teleport_session(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
        source_filter: Option<SourceFilter>,
    ) -> Result<TeleportSessionContext> {
        let resolved_project = match project {
            Some(project) => Some(resolve_project(&self.projects, source_filter, project)?),
            None => None,
        };

        let mut filtered = self
            .sessions
            .iter()
            .filter(|session| {
                matches_source_filter(session.source, source_filter)
                    && matches_project_filter(session, resolved_project.as_ref())
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_sessions(&mut filtered, SortBy::Timestamp, SortOrder::Desc);

        let mut candidates = filtered
            .into_iter()
            .map(|session| ApiSession {
                session_id: session.session_id,
                source: session.source.as_str().to_string(),
                project_name: session.project_name,
                project_path: session.project_path,
                first_timestamp: session.first_timestamp,
                last_timestamp: session.last_timestamp,
                message_count: session.message_count,
                user_messages: session.user_messages,
                assistant_messages: session.assistant_messages,
                preview: session.preview,
            })
            .collect::<Vec<_>>();
        if let Some(session_id) = session_id {
            candidates.retain(|session| session.session_id == session_id);
            if candidates.is_empty() {
                bail!("session {session_id} not found in scope");
            }
            if candidates.len() > 1 {
                bail!(
                    "multiple sessions matched session id {session_id}; pass --project or --source"
                );
            }
        } else if candidates.is_empty() {
            bail!("no sessions found in scope");
        } else {
            candidates.truncate(1);
        }

        let session = candidates
            .into_iter()
            .next()
            .expect("teleport session candidate");
        let source_file = self
            .messages
            .iter()
            .filter(|message| {
                message.session_id == session.session_id
                    && message.project_name == session.project_name
                    && message.source.as_str() == session.source
            })
            .map(|message| message.source_file.clone())
            .min()
            .map(PathBuf::from)
            .with_context(|| {
                format!(
                    "native transcript path missing for session {}",
                    session.session_id
                )
            })?;

        Ok(TeleportSessionContext {
            session,
            source_file,
        })
    }

    pub fn latest_session_messages(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
        source_filter: Option<SourceFilter>,
        window: usize,
        message_index_range: Option<MessageIndexRange>,
    ) -> Result<ApiMessagesResponse> {
        let resolved_project = match project {
            Some(project) => Some(resolve_project(&self.projects, source_filter, project)?),
            None => None,
        };

        let scoped = self
            .messages
            .iter()
            .filter(|message| {
                matches_source_filter(message.source, source_filter)
                    && matches_session_filter(message.session_id.as_str(), session_id)
                    && matches_message_project_filter(message, resolved_project.as_ref())
            })
            .collect::<Vec<_>>();

        let Some(latest) = scoped
            .iter()
            .max_by(|a, b| latest_session_message_cmp(a, b))
        else {
            return Ok(ApiMessagesResponse {
                messages: Vec::new(),
                total_messages: 0,
                next_page: false,
                next_offset: 0,
                next_command: None,
            });
        };
        let latest_key = (
            latest.source,
            latest.project_name.clone(),
            latest.session_id.clone(),
        );

        let mut latest_session_messages = scoped
            .into_iter()
            .filter(|message| session_key(message) == latest_key)
            .cloned()
            .collect::<Vec<_>>();
        sort_messages(
            &mut latest_session_messages,
            SortBy::Timestamp,
            SortOrder::Asc,
            &HashMap::new(),
        );

        let total_messages = latest_session_messages.len() as i64;
        let ranged = apply_message_index_range(latest_session_messages, message_index_range);
        let mut windowed = ranged.into_iter().rev().take(window).collect::<Vec<_>>();
        windowed.reverse();
        let next_offset = windowed.len() as i64;

        Ok(ApiMessagesResponse {
            messages: windowed.into_iter().map(api_message_from_record).collect(),
            total_messages,
            next_page: false,
            next_offset,
            next_command: None,
        })
    }
}

fn path_basename(value: &str) -> Option<&str> {
    Path::new(value.trim())
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
}

fn project_lookup_aliases(name: &str, original_path: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    let mut push_alias = |alias: &str| {
        if alias.is_empty() || alias == name {
            return;
        }
        if !aliases.iter().any(|existing| existing == alias) {
            aliases.push(alias.to_string());
        }
    };

    if let Some(basename) = path_basename(name) {
        push_alias(basename);
    }
    if original_path != name
        && let Some(basename) = path_basename(original_path)
    {
        push_alias(basename);
    }

    let canonical = if name.starts_with('/') {
        name
    } else {
        original_path
    };
    if canonical.starts_with('/') {
        for alias in project_aliases(canonical) {
            push_alias(&alias);
        }
    }

    aliases.sort();
    aliases.dedup();
    aliases
}

/// Resolve a project identifier against known projects, handling Codex path normalization
/// and deterministic basename aliases for read commands and teleport.
/// When source_filter is None, searches all sources.
fn resolve_project(
    projects: &[ProjectAggregate],
    source_filter: Option<SourceFilter>,
    project: &str,
) -> Result<ResolvedProject> {
    let trimmed = project.trim();
    if trimmed.is_empty() {
        return Ok(ResolvedProject {
            names: vec![trimmed.to_string()],
        });
    }

    let mut candidates = vec![trimmed.to_string()];
    if trimmed.starts_with('/') {
        let without_leading = trimmed.trim_start_matches('/');
        if !without_leading.is_empty() {
            candidates.push(without_leading.to_string());
        }
    } else {
        candidates.push(format!("/{trimmed}"));
    }
    candidates.sort();
    candidates.dedup();

    let should_search_codex = source_filter.is_none() || source_filter == Some(SourceFilter::Codex);
    let should_search_claude =
        source_filter.is_none() || source_filter == Some(SourceFilter::Claude);
    let should_search_cursor =
        source_filter.is_none() || source_filter == Some(SourceFilter::Cursor);
    let should_search_grok = source_filter.is_none() || source_filter == Some(SourceFilter::Grok);
    let should_search_pi = source_filter.is_none() || source_filter == Some(SourceFilter::Pi);

    let mut matched_names = Vec::new();

    if should_search_codex {
        for candidate in &candidates {
            if let Some(project_match) = projects
                .iter()
                .find(|item| {
                    item.source == SourceKind::Codex
                        && (item.name == *candidate || item.original_path == *candidate)
                })
                .filter(|m| !matched_names.contains(&m.name))
            {
                matched_names.push(project_match.name.clone());
            }
        }
    }

    if should_search_claude {
        for candidate in &candidates {
            if let Some(project_match) = projects
                .iter()
                .find(|item| {
                    item.source == SourceKind::Claude
                        && (item.name == *candidate || item.original_path == *candidate)
                })
                .filter(|m| !matched_names.contains(&m.name))
            {
                matched_names.push(project_match.name.clone());
            }
        }
    }

    if should_search_cursor {
        for candidate in &candidates {
            if let Some(project_match) = projects
                .iter()
                .find(|item| {
                    item.source == SourceKind::Cursor
                        && (item.name == *candidate || item.original_path == *candidate)
                })
                .filter(|m| !matched_names.contains(&m.name))
            {
                matched_names.push(project_match.name.clone());
            }
        }
    }

    if should_search_grok {
        for candidate in &candidates {
            if let Some(project_match) = projects
                .iter()
                .find(|item| {
                    item.source == SourceKind::Grok
                        && (item.name == *candidate || item.original_path == *candidate)
                })
                .filter(|m| !matched_names.contains(&m.name))
            {
                matched_names.push(project_match.name.clone());
            }
        }
    }

    if should_search_pi {
        for candidate in &candidates {
            if let Some(project_match) = projects
                .iter()
                .find(|item| {
                    item.source == SourceKind::Pi
                        && (item.name == *candidate || item.original_path == *candidate)
                })
                .filter(|m| !matched_names.contains(&m.name))
            {
                matched_names.push(project_match.name.clone());
            }
        }
    }

    if !matched_names.is_empty() {
        return Ok(ResolvedProject {
            names: matched_names,
        });
    }

    let basename = path_basename(trimmed).unwrap_or(trimmed);
    let mut alias_matches = Vec::new();
    let mut alias_identities = Vec::new();

    for item in projects {
        if !matches_source_filter(item.source, source_filter) {
            continue;
        }
        for candidate in [&item.name, &item.original_path] {
            if candidate.is_empty() {
                continue;
            }
            let matches_basename = path_basename(candidate) == Some(basename);
            let matches_literal = candidate == trimmed;
            if (matches_basename || matches_literal) && !alias_matches.contains(&item.name) {
                alias_matches.push(item.name.clone());
                let identity = if item.original_path.is_empty() {
                    item.name.clone()
                } else {
                    item.original_path.clone()
                };
                alias_identities.push(identity);
            }
        }
    }

    alias_matches.sort();
    alias_matches.dedup();
    alias_identities.sort();
    alias_identities.dedup();

    if alias_identities.len() > 1 {
        bail!("multiple projects matched alias {basename:?}; pass an exact project path");
    }

    if !alias_matches.is_empty() {
        return Ok(ResolvedProject {
            names: alias_matches,
        });
    }

    Ok(ResolvedProject {
        names: vec![project.to_string()],
    })
}

fn matches_project_filter(session: &SessionAggregate, resolved: Option<&ResolvedProject>) -> bool {
    match resolved {
        None => true,
        Some(rp) => rp.names.contains(&session.project_name),
    }
}

fn matches_message_project_filter(
    message: &MessageRecord,
    resolved: Option<&ResolvedProject>,
) -> bool {
    match resolved {
        None => true,
        Some(rp) => rp.names.contains(&message.project_name),
    }
}

fn matches_session_filter(session_id: &str, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(id) => session_id == id,
    }
}

fn matches_source_filter(source: SourceKind, filter: Option<SourceFilter>) -> bool {
    match filter {
        None => true,
        Some(SourceFilter::Claude) => source == SourceKind::Claude,
        Some(SourceFilter::Codex) => source == SourceKind::Codex,
        Some(SourceFilter::Cursor) => source == SourceKind::Cursor,
        Some(SourceFilter::Grok) => source == SourceKind::Grok,
        Some(SourceFilter::Pi) => source == SourceKind::Pi,
    }
}

fn sort_projects(projects: &mut [ProjectAggregate], sort_by: SortBy, order: SortOrder) {
    projects.sort_by(|a, b| {
        let primary = match sort_by {
            SortBy::Timestamp => a.last_activity.cmp(&b.last_activity),
            SortBy::MessageCount => a.message_count.cmp(&b.message_count),
        };
        let secondary = match sort_by {
            SortBy::Timestamp => a.message_count.cmp(&b.message_count),
            SortBy::MessageCount => a.last_activity.cmp(&b.last_activity),
        };

        apply_sort_order(primary, order)
            .then_with(|| apply_sort_order(secondary, order))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.original_path.cmp(&b.original_path))
            .then_with(|| a.source.cmp(&b.source))
    });
}

fn sort_sessions(sessions: &mut [SessionAggregate], sort_by: SortBy, order: SortOrder) {
    sessions.sort_by(|a, b| {
        let primary = match sort_by {
            SortBy::Timestamp => a.last_timestamp.cmp(&b.last_timestamp),
            SortBy::MessageCount => a.message_count.cmp(&b.message_count),
        };
        let secondary = match sort_by {
            SortBy::Timestamp => a.message_count.cmp(&b.message_count),
            SortBy::MessageCount => a.last_timestamp.cmp(&b.last_timestamp),
        };

        apply_sort_order(primary, order)
            .then_with(|| apply_sort_order(secondary, order))
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.project_name.cmp(&b.project_name))
            .then_with(|| a.project_path.cmp(&b.project_path))
            .then_with(|| a.source.cmp(&b.source))
    });
}

type SessionMessageCountKey = (SourceKind, String, String);

fn build_session_message_counts(
    messages: &[MessageRecord],
) -> HashMap<SessionMessageCountKey, i32> {
    let mut counts = HashMap::new();
    for message in messages {
        let key = (
            message.source,
            message.project_name.clone(),
            message.session_id.clone(),
        );
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

fn sort_messages(
    messages: &mut [MessageRecord],
    sort_by: SortBy,
    order: SortOrder,
    session_message_counts: &HashMap<SessionMessageCountKey, i32>,
) {
    messages.sort_by(|a, b| {
        let primary = match sort_by {
            SortBy::Timestamp => message_chronological_cmp(a, b),
            SortBy::MessageCount => message_session_count(a, session_message_counts)
                .cmp(&message_session_count(b, session_message_counts)),
        };
        let secondary = match sort_by {
            SortBy::Timestamp => Ordering::Equal,
            SortBy::MessageCount => message_chronological_cmp(a, b),
        };

        apply_sort_order(primary, order)
            .then_with(|| apply_sort_order(secondary, order))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
}

fn latest_session_message_cmp(a: &MessageRecord, b: &MessageRecord) -> Ordering {
    message_chronological_cmp(a, b)
        .then_with(|| a.session_id.cmp(&b.session_id))
        .then_with(|| a.project_name.cmp(&b.project_name))
        .then_with(|| a.project_path.cmp(&b.project_path))
        .then_with(|| a.source.cmp(&b.source))
}

fn session_key(message: &MessageRecord) -> SessionMessageCountKey {
    (
        message.source,
        message.project_name.clone(),
        message.session_id.clone(),
    )
}

fn api_message_from_record(message: MessageRecord) -> ApiMessage {
    ApiMessage {
        session_id: message.session_id,
        source: message.source.as_str().to_string(),
        project_name: message.project_name,
        role: message.role,
        content: message.content,
        model: message.model,
        timestamp: message.timestamp,
        is_subagent: message.is_subagent,
        msg_type: message.msg_type,
        input_tokens: message.input_tokens,
        output_tokens: message.output_tokens,
    }
}

fn message_session_count(
    message: &MessageRecord,
    session_message_counts: &HashMap<SessionMessageCountKey, i32>,
) -> i32 {
    let key = (
        message.source,
        message.project_name.clone(),
        message.session_id.clone(),
    );
    session_message_counts.get(&key).copied().unwrap_or(0)
}

fn apply_sort_order(ordering: Ordering, order: SortOrder) -> Ordering {
    match order {
        SortOrder::Asc => ordering,
        SortOrder::Desc => ordering.reverse(),
    }
}

fn apply_pagination<T>(items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    let start = offset.min(items.len());
    let end = match limit {
        Some(limit) => (start + limit).min(items.len()),
        None => items.len(),
    };
    items.into_iter().skip(start).take(end - start).collect()
}

fn apply_message_index_range<T>(items: Vec<T>, range: Option<MessageIndexRange>) -> Vec<T> {
    let Some(range) = range else {
        return items;
    };
    let start = range.from.unwrap_or(0).min(items.len());
    let end = range.to.unwrap_or(items.len()).min(items.len());
    if end < start {
        return Vec::new();
    }
    items.into_iter().skip(start).take(end - start).collect()
}

fn message_index_range_len(len: usize, range: MessageIndexRange) -> usize {
    let start = range.from.unwrap_or(0).min(len);
    let end = range.to.unwrap_or(len).min(len);
    if end < start {
        return 0;
    }
    end - start
}

fn preview_cmp(a: &PreviewCandidate, b: &PreviewCandidate) -> Ordering {
    a.timestamp
        .cmp(&b.timestamp)
        .then_with(|| a.source_file.cmp(&b.source_file))
        .then_with(|| a.line_index.cmp(&b.line_index))
}

fn message_chronological_cmp(a: &MessageRecord, b: &MessageRecord) -> Ordering {
    a.timestamp
        .cmp(&b.timestamp)
        .then_with(|| a.source_file.cmp(&b.source_file))
        .then_with(|| a.line_index.cmp(&b.line_index))
}

fn truncate_preview(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let byte_idx = value
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    format!("{}...", &value[..byte_idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        source: SourceKind,
        project_name: &str,
        session_id: &str,
        role: &str,
        content: &str,
        timestamp: &str,
        line_index: usize,
    ) -> MessageRecord {
        MessageRecord {
            source,
            project_name: project_name.to_string(),
            project_path: project_name.to_string(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            model: "model".to_string(),
            timestamp: timestamp.to_string(),
            is_subagent: false,
            msg_type: role.to_string(),
            input_tokens: 0,
            output_tokens: 0,
            source_file: "fixture.jsonl".to_string(),
            line_index,
        }
    }

    #[test]
    fn messages_pagination_is_offset_from_newest_then_chronological() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "session-1",
                "user",
                "first",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "session-1",
                "assistant",
                "second",
                "2025-01-01T00:01:00",
                1,
            ),
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "session-1",
                "user",
                "third",
                "2025-01-01T00:02:00",
                2,
            ),
        ]);

        let response = service
            .messages(
                Some("session-1"),
                None,
                None,
                MessageQueryOptions::new(
                    Some(1),
                    1,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )
            .expect("query");
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].content, "second");
    }

    #[test]
    fn latest_session_messages_selects_latest_session_then_chronological_window() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Codex,
                "/Users/test/proj",
                "older-session",
                "user",
                "older",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/proj",
                "latest-session",
                "user",
                "first",
                "2025-01-02T00:00:00",
                1,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/proj",
                "latest-session",
                "assistant",
                "second",
                "2025-01-02T00:01:00",
                2,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/proj",
                "latest-session",
                "user",
                "third",
                "2025-01-02T00:02:00",
                3,
            ),
        ]);

        let response = service
            .latest_session_messages(
                None,
                Some("/Users/test/proj"),
                Some(SourceFilter::Codex),
                2,
                None,
            )
            .expect("query");

        assert_eq!(response.total_messages, 3);
        assert_eq!(response.messages.len(), 2);
        assert_eq!(response.messages[0].content, "second");
        assert_eq!(response.messages[1].content, "third");
        assert!(response.messages.iter().all(|message| {
            message.session_id == "latest-session" && message.project_name == "/Users/test/proj"
        }));
        assert!(!response.next_page);
    }

    #[test]
    fn codex_project_resolution_accepts_missing_leading_slash() {
        let service = QueryService::from_messages(vec![record(
            SourceKind::Codex,
            "/Users/test/codex-proj",
            "session-1",
            "user",
            "message",
            "2025-01-01T00:00:00",
            0,
        )]);

        let response = service
            .sessions(
                Some("Users/test/codex-proj"),
                Some(SourceFilter::Codex),
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
            )
            .expect("query");
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].project_name, "/Users/test/codex-proj");
    }

    #[test]
    fn sessions_without_filters_returns_all() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "sess-claude-1",
                "user",
                "hello",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/codex-proj",
                "sess-codex-1",
                "user",
                "world",
                "2025-01-02T00:00:00",
                0,
            ),
        ]);

        let response = service
            .sessions(
                None,
                None,
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
            )
            .expect("query");
        assert_eq!(response.total_sessions, 2);
        assert_eq!(response.sessions.len(), 2);
    }

    #[test]
    fn sessions_with_source_filter_only() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "sess-claude-1",
                "user",
                "hello",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/codex-proj",
                "sess-codex-1",
                "user",
                "world",
                "2025-01-02T00:00:00",
                0,
            ),
        ]);

        let response = service
            .sessions(
                None,
                Some(SourceFilter::Codex),
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
            )
            .expect("query");
        assert_eq!(response.total_sessions, 1);
        assert_eq!(response.sessions[0].source, "codex");
    }

    #[test]
    fn messages_without_session_returns_all() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "sess-1",
                "user",
                "msg1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/proj",
                "sess-2",
                "user",
                "msg2",
                "2025-01-02T00:00:00",
                0,
            ),
        ]);

        let response = service
            .messages(
                None,
                None,
                None,
                MessageQueryOptions::new(
                    None,
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )
            .expect("query");
        assert_eq!(response.total_messages, 2);
        assert_eq!(response.messages.len(), 2);
    }

    #[test]
    fn messages_with_project_filter() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "sess-1",
                "user",
                "msg1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/other",
                "sess-2",
                "user",
                "msg2",
                "2025-01-02T00:00:00",
                0,
            ),
        ]);

        let response = service
            .messages(
                None,
                Some("-Users-test-proj"),
                Some(SourceFilter::Claude),
                MessageQueryOptions::new(
                    None,
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )
            .expect("query");
        assert_eq!(response.total_messages, 1);
        assert_eq!(response.messages[0].project_name, "-Users-test-proj");
    }

    #[test]
    fn project_resolution_searches_both_sources_when_no_filter() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-Users-test-proj",
                "sess-claude-1",
                "user",
                "hello",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/codex-proj",
                "sess-codex-1",
                "user",
                "world",
                "2025-01-02T00:00:00",
                0,
            ),
        ]);

        // Without source filter, searching by codex project path should find it
        let response = service
            .sessions(
                Some("Users/test/codex-proj"),
                None,
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
            )
            .expect("query");
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].source, "codex");
    }

    #[test]
    fn sessions_sort_by_message_count_desc_uses_message_count_key() {
        let service = QueryService::from_messages(vec![
            // Most recent timestamp but fewest messages
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-a",
                "user",
                "a1",
                "2025-01-03T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-a",
                "assistant",
                "a2",
                "2025-01-03T00:01:00",
                1,
            ),
            // Oldest timestamp but most messages
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "user",
                "b1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "assistant",
                "b2",
                "2025-01-01T00:01:00",
                1,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "user",
                "b3",
                "2025-01-01T00:02:00",
                2,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "assistant",
                "b4",
                "2025-01-01T00:03:00",
                3,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "user",
                "b5",
                "2025-01-01T00:04:00",
                4,
            ),
            // Middle timestamp and middle message count
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-c",
                "user",
                "c1",
                "2025-01-02T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-c",
                "assistant",
                "c2",
                "2025-01-02T00:01:00",
                1,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-c",
                "user",
                "c3",
                "2025-01-02T00:02:00",
                2,
            ),
        ]);

        let response = service
            .sessions(
                None,
                Some(SourceFilter::Codex),
                None,
                0,
                SortOptions::new(SortBy::MessageCount, SortOrder::Desc),
            )
            .expect("query");
        let ids = response
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["session-b", "session-c", "session-a"]);
    }

    #[test]
    fn sessions_sort_by_message_count_asc_uses_message_count_key() {
        let service = QueryService::from_messages(vec![
            // Most recent timestamp but fewest messages
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-a",
                "user",
                "a1",
                "2025-01-03T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-a",
                "assistant",
                "a2",
                "2025-01-03T00:01:00",
                1,
            ),
            // Oldest timestamp but most messages
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "user",
                "b1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "assistant",
                "b2",
                "2025-01-01T00:01:00",
                1,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "user",
                "b3",
                "2025-01-01T00:02:00",
                2,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "assistant",
                "b4",
                "2025-01-01T00:03:00",
                3,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-b",
                "user",
                "b5",
                "2025-01-01T00:04:00",
                4,
            ),
            // Middle timestamp and middle message count
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-c",
                "user",
                "c1",
                "2025-01-02T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-c",
                "assistant",
                "c2",
                "2025-01-02T00:01:00",
                1,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/sort-proj",
                "session-c",
                "user",
                "c3",
                "2025-01-02T00:02:00",
                2,
            ),
        ]);

        let response = service
            .sessions(
                None,
                Some(SourceFilter::Codex),
                None,
                0,
                SortOptions::new(SortBy::MessageCount, SortOrder::Asc),
            )
            .expect("query");
        let ids = response
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["session-a", "session-c", "session-b"]);
    }

    #[test]
    fn sort_projects_message_count_asc_tie_breaks_by_original_path() {
        let mut projects = vec![
            ProjectAggregate {
                name: "proj".to_string(),
                source: SourceKind::Codex,
                original_path: "/Users/test/b".to_string(),
                session_count: 1,
                message_count: 2,
                last_activity: "2025-01-01T00:00:00".to_string(),
            },
            ProjectAggregate {
                name: "proj".to_string(),
                source: SourceKind::Codex,
                original_path: "/Users/test/a".to_string(),
                session_count: 1,
                message_count: 2,
                last_activity: "2025-01-01T00:00:00".to_string(),
            },
        ];

        sort_projects(&mut projects, SortBy::MessageCount, SortOrder::Asc);
        let paths = projects
            .iter()
            .map(|project| project.original_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["/Users/test/a", "/Users/test/b"]);
    }

    #[test]
    fn sort_sessions_message_count_asc_tie_breaks_by_project_identity() {
        let mut sessions = vec![
            SessionAggregate {
                project_name: "/Users/test/b".to_string(),
                project_path: "/Users/test/b".to_string(),
                source: SourceKind::Codex,
                session_id: "same-session".to_string(),
                first_timestamp: "2025-01-01T00:00:00".to_string(),
                last_timestamp: "2025-01-02T00:00:00".to_string(),
                message_count: 2,
                user_messages: 1,
                assistant_messages: 1,
                preview: String::new(),
            },
            SessionAggregate {
                project_name: "/Users/test/a".to_string(),
                project_path: "/Users/test/a".to_string(),
                source: SourceKind::Codex,
                session_id: "same-session".to_string(),
                first_timestamp: "2025-01-01T00:00:00".to_string(),
                last_timestamp: "2025-01-02T00:00:00".to_string(),
                message_count: 2,
                user_messages: 1,
                assistant_messages: 1,
                preview: String::new(),
            },
        ];

        sort_sessions(&mut sessions, SortBy::MessageCount, SortOrder::Asc);
        let project_names = sessions
            .iter()
            .map(|session| session.project_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(project_names, vec!["/Users/test/a", "/Users/test/b"]);
    }

    #[test]
    fn messages_pagination_response_has_next_page_when_more_results() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m2",
                "2025-01-01T00:01:00",
                1,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m3",
                "2025-01-01T00:02:00",
                2,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m4",
                "2025-01-01T00:03:00",
                3,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m5",
                "2025-01-01T00:04:00",
                4,
            ),
        ]);

        let response = service
            .messages(
                None,
                None,
                None,
                MessageQueryOptions::new(
                    Some(2),
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )
            .expect("query");
        assert_eq!(response.messages.len(), 2);
        assert_eq!(response.total_messages, 5);
        assert!(response.next_page);
        assert_eq!(response.next_offset, 2);
    }

    #[test]
    fn messages_pagination_response_no_next_page_at_end() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m2",
                "2025-01-01T00:01:00",
                1,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m3",
                "2025-01-01T00:02:00",
                2,
            ),
        ]);

        let response = service
            .messages(
                None,
                None,
                None,
                MessageQueryOptions::new(
                    Some(2),
                    2,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )
            .expect("query");
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.total_messages, 3);
        assert!(!response.next_page);
        assert_eq!(response.next_offset, 3);
    }

    #[test]
    fn messages_pagination_response_no_next_page_without_limit() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m1",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Claude,
                "-p",
                "s1",
                "user",
                "m2",
                "2025-01-01T00:01:00",
                1,
            ),
        ]);

        let response = service
            .messages(
                None,
                None,
                None,
                MessageQueryOptions::new(
                    None,
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )
            .expect("query");
        assert_eq!(response.messages.len(), 2);
        assert!(!response.next_page);
        assert_eq!(response.next_offset, 2);
    }

    #[test]
    fn resolve_project_matches_basename_alias() {
        let service = QueryService::from_messages(vec![record(
            SourceKind::Codex,
            "/Users/alice/dev/mmr",
            "sess-1",
            "user",
            "hello",
            "2025-01-01T00:00:00",
            0,
        )]);

        let resolved = resolve_project(&service.projects, Some(SourceFilter::Codex), "mmr")
            .expect("basename alias should resolve");

        assert_eq!(resolved.names, vec!["/Users/alice/dev/mmr".to_string()]);
    }

    #[test]
    fn resolve_project_rejects_ambiguous_basename_alias() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Codex,
                "/Users/alice/dev/mmr",
                "sess-1",
                "user",
                "hello",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/bob/work/mmr",
                "sess-2",
                "user",
                "hello",
                "2025-01-01T00:01:00",
                0,
            ),
        ]);

        let error = resolve_project(&service.projects, Some(SourceFilter::Codex), "mmr")
            .expect_err("ambiguous basename should fail");
        assert!(
            error
                .to_string()
                .contains("multiple projects matched alias")
        );
    }

    #[test]
    fn resolve_project_basename_alias_can_match_same_path_across_sources() {
        let mut claude = record(
            SourceKind::Claude,
            "-Users-alice-dev-mmr",
            "sess-claude",
            "user",
            "hello",
            "2025-01-01T00:01:00",
            0,
        );
        claude.project_path = "/Users/alice/dev/mmr".to_string();
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Codex,
                "/Users/alice/dev/mmr",
                "sess-codex",
                "user",
                "hello",
                "2025-01-01T00:00:00",
                0,
            ),
            claude,
        ]);

        let resolved =
            resolve_project(&service.projects, None, "mmr").expect("same-path aliases resolve");

        assert_eq!(
            resolved.names,
            vec![
                "-Users-alice-dev-mmr".to_string(),
                "/Users/alice/dev/mmr".to_string()
            ]
        );
    }

    #[test]
    fn project_lookup_aliases_exposes_basename_and_hyphen_aliases() {
        let aliases =
            project_lookup_aliases("/Users/test/remapped-proj", "/Users/test/remapped-proj");
        assert!(aliases.contains(&"remapped-proj".to_string()));
        assert!(aliases.contains(&"-Users-test-remapped-proj".to_string()));
    }

    #[test]
    fn sessions_resolve_basename_project_alias_for_read_commands() {
        let service = QueryService::from_messages(vec![record(
            SourceKind::Codex,
            "/Users/test/remapped-proj",
            "sess-codex-1",
            "user",
            "hello",
            "2025-01-01T00:00:00",
            0,
        )]);

        let response = service
            .sessions(
                Some("remapped-proj"),
                Some(SourceFilter::Codex),
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
            )
            .expect("basename alias should resolve for sessions");

        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].session_id, "sess-codex-1");
    }

    #[test]
    fn resolve_teleport_session_selects_latest_in_project_scope() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Codex,
                "/Users/test/codex-proj",
                "sess-older",
                "user",
                "older",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/codex-proj",
                "sess-latest",
                "user",
                "latest",
                "2025-01-02T00:00:00",
                0,
            ),
        ]);

        let context = service
            .resolve_teleport_session(
                None,
                Some("/Users/test/codex-proj"),
                Some(SourceFilter::Codex),
            )
            .expect("latest session in scope");

        assert_eq!(context.session.session_id, "sess-latest");
    }

    #[test]
    fn resolve_teleport_session_rejects_ambiguous_session_id() {
        let service = QueryService::from_messages(vec![
            record(
                SourceKind::Codex,
                "/Users/test/one",
                "sess-dup",
                "user",
                "one",
                "2025-01-01T00:00:00",
                0,
            ),
            record(
                SourceKind::Codex,
                "/Users/test/two",
                "sess-dup",
                "user",
                "two",
                "2025-01-01T00:01:00",
                0,
            ),
        ]);

        let error = service
            .resolve_teleport_session(Some("sess-dup"), None, Some(SourceFilter::Codex))
            .expect_err("duplicate session ids should fail");
        assert!(error.to_string().contains("multiple sessions matched"));
    }
}
