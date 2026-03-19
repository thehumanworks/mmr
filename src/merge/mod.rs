use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use time::{Duration, PrimitiveDateTime, macros::format_description};

use crate::messages::service::QueryService;
use crate::source::resolve_home_dir;
use crate::types::{
    ApiMergeResponse, ApiMergeSession, MessageRecord, SortBy, SortOptions, SortOrder, SourceFilter,
    SourceKind,
};

#[derive(Debug, Clone)]
pub enum MergeRequest {
    SessionToSession {
        from_session: String,
        to_session: String,
        from_agent: Option<SourceFilter>,
        to_agent: Option<SourceFilter>,
    },
    AgentToAgent {
        from_agent: SourceFilter,
        to_agent: SourceFilter,
        session: Option<String>,
        project: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct SessionHandle {
    source: SourceKind,
    project_name: String,
    project_path: String,
    session_id: String,
    source_file: PathBuf,
    messages: Vec<MessageRecord>,
}

#[derive(Debug)]
struct PersistedMerge {
    api: ApiMergeSession,
    considerations: Vec<String>,
}

#[derive(Debug)]
struct PreparedMessage {
    role: String,
    content: String,
    model: String,
    timestamp: String,
    input_tokens: i64,
    output_tokens: i64,
}

#[derive(Debug)]
struct TargetProject {
    project_name: String,
    project_path: String,
}

#[derive(Debug)]
struct IdFactory {
    seed_millis: u128,
    counter: u64,
}

impl IdFactory {
    fn new() -> Self {
        let seed_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            seed_millis,
            counter: 0,
        }
    }

    fn next_session_id(
        &mut self,
        from_source: SourceKind,
        to_source: SourceKind,
        source_session_id: &str,
    ) -> String {
        self.counter += 1;
        format!(
            "merge-{}-to-{}-{}-{}-{}",
            from_source.as_str(),
            to_source.as_str(),
            sanitize_id_fragment(source_session_id),
            self.seed_millis,
            self.counter
        )
    }

    fn next_message_uuid(&mut self) -> String {
        self.counter += 1;
        format!("mmr-msg-{}-{}", self.seed_millis, self.counter)
    }
}

pub fn merge(service: &QueryService, request: MergeRequest) -> Result<ApiMergeResponse> {
    let home = resolve_home_dir()?;
    let mut considerations = BTreeSet::new();

    let (mode, from_agent, to_agent, session_merges) = match request {
        MergeRequest::SessionToSession {
            from_session,
            to_session,
            from_agent,
            to_agent,
        } => {
            let from =
                resolve_unique_session(service, &from_session, from_agent, "from", "from-agent")?;
            let to = resolve_unique_session(service, &to_session, to_agent, "to", "to-agent")?;

            if from.source == to.source
                && from.project_name == to.project_name
                && from.session_id == to.session_id
            {
                bail!("cannot merge a session into itself");
            }

            let persisted = append_into_existing_session(&from, &to)?;
            for item in &persisted.considerations {
                considerations.insert(item.clone());
            }

            (
                "session-to-session".to_string(),
                from.source.as_str().to_string(),
                to.source.as_str().to_string(),
                vec![persisted.api],
            )
        }
        MergeRequest::AgentToAgent {
            from_agent,
            to_agent,
            session,
            project,
        } => {
            if from_agent == to_agent {
                bail!("--from-agent and --to-agent must differ for agent-to-agent merges");
            }

            let mut id_factory = IdFactory::new();
            let source_sessions = resolve_source_sessions(
                service,
                from_agent,
                project.as_deref(),
                session.as_deref(),
            )?;

            let mut persisted = Vec::with_capacity(source_sessions.len());
            for source_session in source_sessions {
                let merge =
                    create_target_session(&home, &source_session, to_agent, &mut id_factory)?;
                for item in &merge.considerations {
                    considerations.insert(item.clone());
                }
                persisted.push(merge.api);
            }

            (
                "agent-to-agent".to_string(),
                source_filter_as_str(from_agent).to_string(),
                source_filter_as_str(to_agent).to_string(),
                persisted,
            )
        }
    };

    let total_sessions_merged = session_merges.len() as i64;
    let total_messages_merged = session_merges
        .iter()
        .map(|item| i64::from(item.merged_messages))
        .sum();

    Ok(ApiMergeResponse {
        mode,
        from_agent,
        to_agent,
        session_merges,
        total_sessions_merged,
        total_messages_merged,
        schema_considerations: considerations.into_iter().collect(),
    })
}

fn append_into_existing_session(
    from: &SessionHandle,
    to: &SessionHandle,
) -> Result<PersistedMerge> {
    let prepared_messages = prepare_messages_for_existing_session(&from.messages, &to.messages)?;
    let target_file = PathBuf::from(&to.source_file);
    let mut considerations = Vec::new();
    let timestamp_strategy = append_timestamp_strategy(&from.messages, &prepared_messages);

    let (lines, model_strategy) = match to.source {
        SourceKind::Codex => {
            let provider = existing_codex_provider(to);
            let lines = render_codex_events(&prepared_messages);
            let model_strategy = if from.source == SourceKind::Codex {
                format!("preserved-codex-session-provider:{provider}")
            } else {
                considerations_for_codex_target(&mut considerations, from);
                format!("collapsed-source-models-to-existing-codex-provider:{provider}")
            };
            (lines, model_strategy)
        }
        SourceKind::Claude => {
            let mut id_factory = IdFactory::new();
            let previous_uuid = read_last_claude_uuid(&target_file, &to.session_id)?;
            let lines = render_claude_events(
                &prepared_messages,
                &to.session_id,
                &to.project_path,
                previous_uuid.as_deref(),
                &mut id_factory,
            );
            let model_strategy = if from.source == SourceKind::Claude {
                "preserved-claude-assistant-models".to_string()
            } else {
                considerations.push(
                    "Claude stores model metadata on assistant messages; Codex only stores a session-level provider, so imported assistant messages reuse that provider string as their model value.".to_string(),
                );
                let assistant_model = first_assistant_model(&prepared_messages)
                    .unwrap_or_else(|| "unknown".to_string());
                format!("expanded-codex-provider-into-claude-assistant-models:{assistant_model}")
            };
            (lines, model_strategy)
        }
    };

    if timestamp_strategy == "shifted-to-append-after-target" {
        considerations.push(
            "Session-to-session merges retime copied messages when needed so the imported block stays after the destination session's existing last message.".to_string(),
        );
    }

    write_jsonl_lines(&target_file, &lines)?;

    Ok(PersistedMerge {
        api: ApiMergeSession {
            from_session_id: from.session_id.clone(),
            to_session_id: to.session_id.clone(),
            from_source: from.source.as_str().to_string(),
            to_source: to.source.as_str().to_string(),
            from_project_name: from.project_name.clone(),
            to_project_name: to.project_name.clone(),
            created_target_session: false,
            merged_messages: prepared_messages.len() as i32,
            timestamp_strategy,
            model_strategy,
            target_file: target_file.display().to_string(),
        },
        considerations,
    })
}

fn create_target_session(
    home: &Path,
    source_session: &SessionHandle,
    to_agent: SourceFilter,
    id_factory: &mut IdFactory,
) -> Result<PersistedMerge> {
    let to_source = source_filter_to_kind(to_agent);
    let target_session_id =
        id_factory.next_session_id(source_session.source, to_source, &source_session.session_id);
    let prepared_messages = prepare_messages_for_new_session(&source_session.messages);
    let target_project = target_project_for_merge(source_session, to_source);
    let mut considerations = Vec::new();

    let (target_file, lines, model_strategy) = match to_source {
        SourceKind::Codex => {
            considerations_for_codex_target(&mut considerations, source_session);
            let target_file = home
                .join(".codex")
                .join("sessions")
                .join(format!("{target_session_id}.jsonl"));
            let provider = provider_for_new_codex_session(source_session);
            let lines = render_new_codex_session(
                &target_session_id,
                &target_project.project_path,
                &provider,
                &prepared_messages,
            );
            let model_strategy = if source_session.source == SourceKind::Codex {
                format!("preserved-codex-session-provider:{provider}")
            } else {
                format!("collapsed-source-models-to-codex-provider:{provider}")
            };
            (target_file, lines, model_strategy)
        }
        SourceKind::Claude => {
            if source_session
                .messages
                .iter()
                .any(|message| message.is_subagent)
            {
                considerations.push(
                    "Claude subagent ancestry is flattened during merge; imported sessions are written as top-level project sessions.".to_string(),
                );
            }

            let project_dir = home
                .join(".claude")
                .join("projects")
                .join(&target_project.project_name);
            let target_file = project_dir.join(format!("{target_session_id}.jsonl"));
            let lines = render_claude_events(
                &prepared_messages,
                &target_session_id,
                &target_project.project_path,
                None,
                id_factory,
            );
            let model_strategy = if source_session.source == SourceKind::Claude {
                "preserved-claude-assistant-models".to_string()
            } else {
                considerations.push(
                    "Claude stores model metadata on assistant messages; Codex only stores a session-level provider, so imported assistant messages reuse that provider string as their model value.".to_string(),
                );
                let assistant_model = first_assistant_model(&prepared_messages)
                    .unwrap_or_else(|| "unknown".to_string());
                format!("expanded-codex-provider-into-claude-assistant-models:{assistant_model}")
            };
            (target_file, lines, model_strategy)
        }
    };

    write_jsonl_lines(&target_file, &lines)?;

    Ok(PersistedMerge {
        api: ApiMergeSession {
            from_session_id: source_session.session_id.clone(),
            to_session_id: target_session_id,
            from_source: source_session.source.as_str().to_string(),
            to_source: to_source.as_str().to_string(),
            from_project_name: source_session.project_name.clone(),
            to_project_name: target_project.project_name,
            created_target_session: true,
            merged_messages: prepared_messages.len() as i32,
            timestamp_strategy: "preserved-source-timestamps".to_string(),
            model_strategy,
            target_file: target_file.display().to_string(),
        },
        considerations,
    })
}

fn resolve_source_sessions(
    service: &QueryService,
    from_agent: SourceFilter,
    project: Option<&str>,
    session: Option<&str>,
) -> Result<Vec<SessionHandle>> {
    let sessions = service.sessions(
        project,
        Some(from_agent),
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
    );

    let mut handles = sessions
        .sessions
        .into_iter()
        .filter(|candidate| {
            session
                .map(|session_id| candidate.session_id == session_id)
                .unwrap_or(true)
        })
        .filter_map(|candidate| session_handle_from_api(service, &candidate))
        .collect::<Vec<_>>();

    handles.sort_by(session_handle_cmp);

    if let Some(session_id) = session {
        if handles.is_empty() {
            bail!(
                "no {} session matched --session {}",
                source_filter_as_str(from_agent),
                session_id
            );
        }

        if handles.len() > 1 {
            let matches = handles
                .iter()
                .map(|handle| format!("{} ({})", handle.project_name, handle.source.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "--session {} matched multiple {} sessions: {}. Add --project to disambiguate.",
                session_id,
                source_filter_as_str(from_agent),
                matches
            );
        }
    }

    if handles.is_empty() {
        let project_fragment = project
            .map(|value| format!(" with --project {value}"))
            .unwrap_or_default();
        bail!(
            "no {} sessions found{}",
            source_filter_as_str(from_agent),
            project_fragment
        );
    }

    Ok(handles)
}

fn resolve_unique_session(
    service: &QueryService,
    session_id: &str,
    source_filter: Option<SourceFilter>,
    label: &str,
    agent_flag: &str,
) -> Result<SessionHandle> {
    let sessions = service.sessions(
        None,
        source_filter,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
    );

    let mut handles = sessions
        .sessions
        .into_iter()
        .filter(|candidate| candidate.session_id == session_id)
        .filter_map(|candidate| session_handle_from_api(service, &candidate))
        .collect::<Vec<_>>();

    handles.sort_by(session_handle_cmp);

    match handles.len() {
        0 => bail!("{label} session '{session_id}' was not found"),
        1 => Ok(handles.remove(0)),
        _ => {
            let matches = handles
                .iter()
                .map(|handle| format!("{} ({})", handle.project_name, handle.source.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "{label} session '{session_id}' is ambiguous: {}. Pass --{} to disambiguate.",
                matches,
                agent_flag
            )
        }
    }
}

fn session_handle_from_api(
    service: &QueryService,
    session: &crate::types::ApiSession,
) -> Option<SessionHandle> {
    let source = parse_source_kind(&session.source)?;
    let mut messages = service
        .records()
        .iter()
        .filter(|record| {
            record.source == source
                && record.session_id == session.session_id
                && record.project_name == session.project_name
        })
        .cloned()
        .collect::<Vec<_>>();
    messages.sort_by(message_record_cmp);

    let source_file = PathBuf::from(messages.first()?.source_file.clone());
    Some(SessionHandle {
        source,
        project_name: session.project_name.clone(),
        project_path: session.project_path.clone(),
        session_id: session.session_id.clone(),
        source_file,
        messages,
    })
}

fn prepare_messages_for_new_session(source_messages: &[MessageRecord]) -> Vec<PreparedMessage> {
    source_messages
        .iter()
        .map(|message| PreparedMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            model: message.model.clone(),
            timestamp: message.timestamp.clone(),
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
        })
        .collect()
}

fn prepare_messages_for_existing_session(
    source_messages: &[MessageRecord],
    target_messages: &[MessageRecord],
) -> Result<Vec<PreparedMessage>> {
    let timestamps = adjusted_timestamps_for_append(source_messages, target_messages)?;

    Ok(source_messages
        .iter()
        .zip(timestamps)
        .map(|(message, timestamp)| PreparedMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            model: message.model.clone(),
            timestamp,
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
        })
        .collect())
}

fn adjusted_timestamps_for_append(
    source_messages: &[MessageRecord],
    target_messages: &[MessageRecord],
) -> Result<Vec<String>> {
    let target_last = target_messages
        .last()
        .map(|message| message.timestamp.as_str())
        .context("target session has no messages")?;
    let source_first = source_messages
        .first()
        .map(|message| message.timestamp.as_str())
        .context("source session has no messages")?;

    let target_last = parse_timestamp(target_last)?;
    let source_first = parse_timestamp(source_first)?;
    if source_first > target_last {
        return Ok(source_messages
            .iter()
            .map(|message| message.timestamp.clone())
            .collect());
    }

    let delta = (target_last - source_first) + Duration::seconds(1);
    source_messages
        .iter()
        .map(|message| {
            let timestamp = parse_timestamp(&message.timestamp)? + delta;
            format_timestamp(timestamp)
        })
        .collect()
}

fn render_new_codex_session(
    session_id: &str,
    project_path: &str,
    provider: &str,
    messages: &[PreparedMessage],
) -> Vec<Value> {
    let mut lines = Vec::with_capacity(messages.len() + 1);
    let first_timestamp = messages
        .first()
        .map(|message| message.timestamp.clone())
        .unwrap_or_else(|| "1970-01-01T00:00:00".to_string());
    lines.push(json!({
        "type": "session_meta",
        "timestamp": first_timestamp,
        "payload": {
            "id": session_id,
            "cwd": project_path,
            "cli_version": "1.0.0",
            "model_provider": provider,
            "timestamp": first_timestamp,
            "git": {
                "branch": "merged"
            }
        }
    }));
    lines.extend(render_codex_events(messages));
    lines
}

fn render_codex_events(messages: &[PreparedMessage]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|message| match message.role.as_str() {
            "user" => Some(json!({
                "type": "event_msg",
                "timestamp": message.timestamp,
                "payload": {
                    "type": "user_message",
                    "message": message.content,
                }
            })),
            "assistant" => Some(json!({
                "type": "response_item",
                "timestamp": message.timestamp,
                "payload": {
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": message.content,
                    }]
                }
            })),
            _ => None,
        })
        .collect()
}

fn render_claude_events(
    messages: &[PreparedMessage],
    session_id: &str,
    project_path: &str,
    previous_uuid: Option<&str>,
    id_factory: &mut IdFactory,
) -> Vec<Value> {
    let mut parent_uuid = previous_uuid.map(str::to_string);
    let mut lines = Vec::with_capacity(messages.len());

    for message in messages {
        let uuid = id_factory.next_message_uuid();
        let mut value = match message.role.as_str() {
            "user" => json!({
                "type": "user",
                "sessionId": session_id,
                "message": {
                    "role": "user",
                    "content": message.content,
                },
                "timestamp": message.timestamp,
                "uuid": uuid,
                "cwd": project_path,
            }),
            "assistant" => json!({
                "type": "assistant",
                "sessionId": session_id,
                "message": {
                    "role": "assistant",
                    "content": message.content,
                    "model": assistant_model_for_claude(message),
                    "usage": {
                        "input_tokens": message.input_tokens,
                        "output_tokens": message.output_tokens,
                    }
                },
                "timestamp": message.timestamp,
                "uuid": uuid,
                "cwd": project_path,
            }),
            _ => continue,
        };

        if let Some(parent) = &parent_uuid
            && let Some(map) = value.as_object_mut()
        {
            map.insert("parentUuid".to_string(), json!(parent));
        }

        parent_uuid = Some(uuid.clone());
        lines.push(value);
    }

    lines
}

fn write_jsonl_lines(path: &Path, values: &[Value]) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    }

    let needs_leading_newline = path.exists() && !file_ends_with_newline(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    if needs_leading_newline {
        file.write_all(b"\n")
            .with_context(|| format!("failed to write newline to {}", path.display()))?;
    }

    for value in values {
        let line = serde_json::to_string(value)?;
        file.write_all(line.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write newline to {}", path.display()))?;
    }

    Ok(())
}

fn file_ends_with_newline(path: &Path) -> Result<bool> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(bytes.last().map(|byte| *byte == b'\n').unwrap_or(false))
}

fn read_last_claude_uuid(path: &Path, session_id: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    for line in content.lines().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("sessionId").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        if let Some(uuid) = value.get("uuid").and_then(Value::as_str) {
            return Ok(Some(uuid.to_string()));
        }
    }

    Ok(None)
}

fn existing_codex_provider(session: &SessionHandle) -> String {
    session
        .messages
        .iter()
        .find_map(|message| {
            if message.model.trim().is_empty() {
                None
            } else {
                Some(message.model.clone())
            }
        })
        .unwrap_or_else(|| "openai".to_string())
}

fn provider_for_new_codex_session(session: &SessionHandle) -> String {
    if session.source == SourceKind::Codex {
        return existing_codex_provider(session);
    }

    let mut models = session
        .messages
        .iter()
        .filter(|message| message.role == "assistant" && !message.model.trim().is_empty())
        .map(|message| message.model.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    if let Some(model) = models.pop_first() {
        if model.contains("claude") || model.contains("anthropic") {
            return "anthropic".to_string();
        }
        if model.contains("openai")
            || model.starts_with("gpt")
            || model.starts_with("o1")
            || model.starts_with("o3")
            || model.starts_with("o4")
        {
            return "openai".to_string();
        }
        return model;
    }

    match session.source {
        SourceKind::Claude => "anthropic".to_string(),
        SourceKind::Codex => "openai".to_string(),
    }
}

fn assistant_model_for_claude(message: &PreparedMessage) -> String {
    if message.model.trim().is_empty() {
        "unknown".to_string()
    } else {
        message.model.clone()
    }
}

fn first_assistant_model(messages: &[PreparedMessage]) -> Option<String> {
    messages.iter().find_map(|message| {
        if message.role == "assistant" && !message.model.trim().is_empty() {
            Some(message.model.clone())
        } else {
            None
        }
    })
}

fn target_project_for_merge(
    source_session: &SessionHandle,
    to_source: SourceKind,
) -> TargetProject {
    match to_source {
        SourceKind::Codex => {
            let project_path = if source_session.project_path.trim().is_empty() {
                source_session.project_name.clone()
            } else {
                source_session.project_path.clone()
            };
            TargetProject {
                project_name: project_path.clone(),
                project_path,
            }
        }
        SourceKind::Claude => {
            let project_path = if source_session.project_path.trim().is_empty() {
                source_session.project_name.clone()
            } else {
                source_session.project_path.clone()
            };
            TargetProject {
                project_name: encode_claude_project_name(&project_path),
                project_path,
            }
        }
    }
}

fn append_timestamp_strategy(
    source_messages: &[MessageRecord],
    prepared_messages: &[PreparedMessage],
) -> String {
    let shifted = source_messages
        .first()
        .map(|message| message.timestamp.as_str())
        != prepared_messages
            .first()
            .map(|message| message.timestamp.as_str());

    if shifted {
        "shifted-to-append-after-target".to_string()
    } else {
        "preserved-source-timestamps".to_string()
    }
}

fn considerations_for_codex_target(
    considerations: &mut Vec<String>,
    source_session: &SessionHandle,
) {
    if source_session.source != SourceKind::Codex {
        considerations.push(
            "Codex stores model metadata at session scope (`session_meta.payload.model_provider`), so imported per-message model values collapse to a single provider on the destination session.".to_string(),
        );
    }
}

fn parse_source_kind(value: &str) -> Option<SourceKind> {
    match value {
        "claude" => Some(SourceKind::Claude),
        "codex" => Some(SourceKind::Codex),
        _ => None,
    }
}

fn source_filter_to_kind(value: SourceFilter) -> SourceKind {
    match value {
        SourceFilter::Claude => SourceKind::Claude,
        SourceFilter::Codex => SourceKind::Codex,
    }
}

fn source_filter_as_str(value: SourceFilter) -> &'static str {
    match value {
        SourceFilter::Claude => "claude",
        SourceFilter::Codex => "codex",
    }
}

fn encode_claude_project_name(cwd: &str) -> String {
    if cwd == "/" {
        "-".to_string()
    } else {
        format!("-{}", cwd.trim_start_matches('/').replace('/', "-"))
    }
}

fn parse_timestamp(value: &str) -> Result<PrimitiveDateTime> {
    PrimitiveDateTime::parse(
        value,
        &format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
    )
    .with_context(|| format!("invalid timestamp format: {value}"))
}

fn format_timestamp(value: PrimitiveDateTime) -> Result<String> {
    value
        .format(&format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]"
        ))
        .context("failed to format timestamp")
}

fn session_handle_cmp(left: &SessionHandle, right: &SessionHandle) -> std::cmp::Ordering {
    left.source
        .cmp(&right.source)
        .then_with(|| left.project_name.cmp(&right.project_name))
        .then_with(|| left.session_id.cmp(&right.session_id))
}

fn message_record_cmp(left: &MessageRecord, right: &MessageRecord) -> std::cmp::Ordering {
    left.timestamp
        .cmp(&right.timestamp)
        .then_with(|| left.source_file.cmp(&right.source_file))
        .then_with(|| left.line_index.cmp(&right.line_index))
}

fn sanitize_id_fragment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();

    if sanitized.trim_matches('-').is_empty() {
        "session".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(role: &str, timestamp: &str, model: &str) -> PreparedMessage {
        PreparedMessage {
            role: role.to_string(),
            content: "body".to_string(),
            model: model.to_string(),
            timestamp: timestamp.to_string(),
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn record(source: SourceKind, timestamp: &str, model: &str) -> MessageRecord {
        MessageRecord {
            source,
            project_name: "/tmp/proj".to_string(),
            project_path: "/tmp/proj".to_string(),
            session_id: "sess-1".to_string(),
            role: "assistant".to_string(),
            content: "body".to_string(),
            model: model.to_string(),
            timestamp: timestamp.to_string(),
            is_subagent: false,
            msg_type: "assistant".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            source_file: "fixture.jsonl".to_string(),
            line_index: 0,
        }
    }

    #[test]
    fn append_timestamps_shift_forward_when_needed() {
        let source_messages = vec![
            record(SourceKind::Claude, "2025-01-01T00:00:00", "claude-3-opus"),
            record(SourceKind::Claude, "2025-01-01T00:01:00", "claude-3-opus"),
        ];
        let target_messages = vec![record(SourceKind::Codex, "2025-01-02T00:00:00", "openai")];

        let adjusted =
            adjusted_timestamps_for_append(&source_messages, &target_messages).expect("adjusted");
        assert_eq!(adjusted[0], "2025-01-02T00:00:01");
        assert_eq!(adjusted[1], "2025-01-02T00:01:01");
    }

    #[test]
    fn new_codex_sessions_map_claude_models_to_anthropic_provider() {
        let session = SessionHandle {
            source: SourceKind::Claude,
            project_name: "-tmp-proj".to_string(),
            project_path: "/tmp/proj".to_string(),
            session_id: "sess-1".to_string(),
            source_file: PathBuf::from("fixture.jsonl"),
            messages: vec![record(
                SourceKind::Claude,
                "2025-01-01T00:00:00",
                "claude-3-opus",
            )],
        };

        assert_eq!(provider_for_new_codex_session(&session), "anthropic");
    }

    #[test]
    fn claude_assistant_messages_keep_source_model_strings() {
        let messages = vec![
            prepared("user", "2025-01-01T00:00:00", ""),
            prepared("assistant", "2025-01-01T00:00:01", "openai"),
        ];

        let mut ids = IdFactory::new();
        let rendered = render_claude_events(&messages, "sess-1", "/tmp/proj", None, &mut ids);
        assert_eq!(rendered[1]["message"]["model"].as_str(), Some("openai"));
    }
}
