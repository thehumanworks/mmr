use anyhow::Result;
use duckdb::{params, Connection};

use crate::api::dto::{
    ApiAnalyticsResponse, ApiMessage, ApiMessagesResponse, ApiModelStats, ApiProject,
    ApiProjectStats, ApiProjectsResponse, ApiSearchResponse, ApiSearchResult, ApiSession,
    ApiSessionsResponse, ApiSourceStats,
};
use crate::db::rebuild_derived_tables;

use super::search::run_search;

pub struct QueryService<'a> {
    conn: &'a Connection,
}

impl<'a> QueryService<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn projects(
        &self,
        source: Option<&str>,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<ApiProjectsResponse> {
        cmd_projects(self.conn, source, limit, offset)
    }

    pub fn sessions(
        &self,
        project: &str,
        source: Option<&str>,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<ApiSessionsResponse> {
        cmd_sessions(self.conn, project, source, limit, offset)
    }

    pub fn messages(
        &self,
        session_id: &str,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<ApiMessagesResponse> {
        cmd_messages(self.conn, session_id, limit, offset)
    }

    pub fn search(
        &self,
        query: &str,
        project: Option<&str>,
        source: Option<&str>,
        page: usize,
        per_page: usize,
    ) -> Result<ApiSearchResponse> {
        cmd_search(self.conn, query, project, source, page, per_page)
    }

    pub fn stats(&self, source: Option<&str>) -> Result<ApiAnalyticsResponse> {
        cmd_stats(self.conn, source)
    }
}

pub(crate) fn pagination_clause(limit: Option<usize>, offset: usize) -> String {
    let mut clause = String::new();
    if let Some(limit) = limit {
        clause.push_str(&format!(" LIMIT {}", limit));
    }
    if offset > 0 {
        if limit.is_none() {
            clause.push_str(" LIMIT 9223372036854775807");
        }
        clause.push_str(&format!(" OFFSET {}", offset));
    }
    clause
}

pub(crate) fn cmd_projects(
    conn: &Connection,
    source: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiProjectsResponse> {
    let pagination = pagination_clause(limit, offset);
    let (query_sql, has_source) = match source {
        Some(s) if s == "claude" || s == "codex" => (
            format!(
                "SELECT name, source, original_path, session_count, message_count, last_activity FROM projects WHERE source = ? ORDER BY last_activity DESC{}",
                pagination
            ),
            true,
        ),
        _ => (
            format!(
                "SELECT name, source, original_path, session_count, message_count, last_activity FROM projects ORDER BY last_activity DESC{}",
                pagination
            ),
            false,
        ),
    };

    let mut stmt = conn.prepare(&query_sql)?;
    let projects: Vec<ApiProject> = if has_source {
        stmt.query_map(params![source.unwrap()], |row| {
            Ok(ApiProject {
                name: row.get::<_, String>(0)?,
                source: row.get::<_, String>(1)?,
                original_path: row.get::<_, String>(2)?,
                session_count: row.get::<_, i32>(3)?,
                message_count: row.get::<_, i32>(4)?,
                last_activity: row.get::<_, String>(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(ApiProject {
                name: row.get::<_, String>(0)?,
                source: row.get::<_, String>(1)?,
                original_path: row.get::<_, String>(2)?,
                session_count: row.get::<_, i32>(3)?,
                message_count: row.get::<_, i32>(4)?,
                last_activity: row.get::<_, String>(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let (total_messages, total_sessions) = if has_source {
        let m: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE source = ?",
            params![source.unwrap()],
            |row| row.get(0),
        )?;
        let s: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = ?",
            params![source.unwrap()],
            |row| row.get(0),
        )?;
        (m, s)
    } else {
        let m: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        let s: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        (m, s)
    };

    Ok(ApiProjectsResponse {
        projects,
        total_messages,
        total_sessions,
    })
}

pub(crate) fn resolve_project_for_source(conn: &Connection, source: &str, project: &str) -> String {
    if source != "codex" {
        return project.to_string();
    }

    let mut candidates = Vec::new();
    let trimmed = project.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
        if trimmed.starts_with('/') {
            let without = trimmed.trim_start_matches('/');
            if !without.is_empty() {
                candidates.push(without.to_string());
            }
        } else {
            candidates.push(format!("/{}", trimmed));
        }
    }

    candidates.sort();
    candidates.dedup();

    for candidate in candidates {
        let found: Result<String, _> = conn.query_row(
            "SELECT name FROM projects WHERE source = 'codex' AND (name = ? OR original_path = ?) LIMIT 1",
            params![&candidate, &candidate],
            |row| row.get(0),
        );
        if let Ok(name) = found {
            return name;
        }
    }

    project.to_string()
}

pub(crate) fn cmd_sessions(
    conn: &Connection,
    project: &str,
    source: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiSessionsResponse> {
    let source = source.unwrap_or("codex");
    let project = resolve_project_for_source(conn, source, project);

    let project_path: String = conn
        .query_row(
            "SELECT original_path FROM projects WHERE name = ? AND source = ?",
            params![&project, source],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| project.clone());

    let query_sql = format!(
        "SELECT session_id, first_timestamp, last_timestamp, message_count, user_messages, assistant_messages
         FROM sessions WHERE project = ? AND source = ? ORDER BY last_timestamp DESC{}",
        pagination_clause(limit, offset)
    );
    let mut stmt = conn.prepare(&query_sql)?;

    let sessions: Vec<ApiSession> = stmt
        .query_map(params![&project, source], |row| {
            let sid: String = row.get(0)?;
            Ok((
                sid,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i32>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, i32>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(sid, first_ts, last_ts, msg_count, user_msgs, asst_msgs)| {
            let preview: String = conn
                .query_row(
                    "SELECT content_text FROM messages WHERE session_id = ? AND project = ? AND source = ? AND role = 'user' ORDER BY id ASC LIMIT 1",
                    params![&sid, &project, source],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            let preview_short = if preview.len() > 120 {
                let end = preview.ceil_char_boundary(120);
                format!("{}...", &preview[..end])
            } else {
                preview
            };
            ApiSession {
                session_id: sid,
                first_timestamp: first_ts,
                last_timestamp: last_ts,
                message_count: msg_count,
                user_messages: user_msgs,
                assistant_messages: asst_msgs,
                preview: preview_short,
            }
        })
        .collect();

    Ok(ApiSessionsResponse {
        project_name: project,
        project_path,
        source: source.to_string(),
        sessions,
    })
}

pub(crate) fn cmd_messages(
    conn: &Connection,
    session_id: &str,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiMessagesResponse> {
    let mut rebuilt = false;

    let mut session_ctx: Option<(String, String)> = conn
        .query_row(
            "SELECT project, source FROM sessions WHERE session_id = ?",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    if session_ctx.is_none() {
        let has_non_subagent_messages: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM messages
                WHERE session_id = ? AND is_subagent = FALSE
             )",
            params![session_id],
            |row| row.get(0),
        )?;
        if has_non_subagent_messages {
            rebuild_derived_tables(conn)?;
            rebuilt = true;
            session_ctx = conn
                .query_row(
                    "SELECT project, source FROM sessions WHERE session_id = ?",
                    params![session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
        }
    }

    let (mut project_name, mut source) =
        session_ctx.unwrap_or_else(|| (String::new(), String::new()));
    let mut project_path: Option<String> = if project_name.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT original_path FROM projects WHERE name = ? AND source = ?",
            params![&project_name, &source],
            |row| row.get(0),
        )
        .ok()
    };

    if !project_name.is_empty() && project_path.is_none() && !rebuilt {
        rebuild_derived_tables(conn)?;
        session_ctx = conn
            .query_row(
                "SELECT project, source FROM sessions WHERE session_id = ?",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        if let Some((p, s)) = session_ctx {
            project_name = p;
            source = s;
        }
        project_path = conn
            .query_row(
                "SELECT original_path FROM projects WHERE name = ? AND source = ?",
                params![&project_name, &source],
                |row| row.get(0),
            )
            .ok();
    }

    let project_path = project_path.unwrap_or_else(|| project_name.clone());

    let query_sql = format!(
        "SELECT role, content_text, model, timestamp, is_subagent, msg_type, input_tokens, output_tokens
             FROM messages
             WHERE session_id = ?
             ORDER BY id DESC{}",
        pagination_clause(limit, offset)
    );
    let mut stmt = conn.prepare(&query_sql)?;
    let mut messages: Vec<ApiMessage> = stmt
        .query_map(params![session_id], |row| {
            Ok(ApiMessage {
                role: row.get::<_, String>(0)?,
                content: row.get::<_, String>(1)?,
                model: row.get::<_, String>(2)?,
                timestamp: row.get::<_, String>(3)?,
                is_subagent: row.get::<_, bool>(4)?,
                msg_type: row.get::<_, String>(5)?,
                input_tokens: row.get::<_, i64>(6)?,
                output_tokens: row.get::<_, i64>(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    messages.reverse();

    Ok(ApiMessagesResponse {
        session_id: session_id.to_string(),
        project_name,
        project_path,
        source,
        messages,
    })
}

pub(crate) fn cmd_search(
    conn: &Connection,
    query: &str,
    project: Option<&str>,
    source: Option<&str>,
    page: usize,
    per_page: usize,
) -> Result<ApiSearchResponse> {
    if query.is_empty() {
        return Ok(ApiSearchResponse {
            query: String::new(),
            total_count: 0,
            page,
            per_page,
            results: Vec::new(),
        });
    }

    let resolved_project = match (project, source) {
        (Some(p), Some("codex")) => resolve_project_for_source(conn, "codex", p),
        (Some(p), _) => p.to_string(),
        (None, _) => String::new(),
    };
    let project_filter = resolved_project.as_str();
    let source_filter = source.unwrap_or("");
    let offset = page * per_page;

    let (total_count, rows) =
        run_search(conn, query, project_filter, source_filter, per_page, offset);

    let results: Vec<ApiSearchResult> = rows
        .into_iter()
        .map(
            |(
                id,
                project,
                project_path,
                session_id,
                role,
                content,
                model,
                timestamp,
                is_subagent,
                source,
            )| ApiSearchResult {
                id,
                project,
                project_path,
                session_id,
                role,
                content,
                model,
                timestamp,
                is_subagent,
                source,
            },
        )
        .collect();

    Ok(ApiSearchResponse {
        query: query.to_string(),
        total_count,
        page,
        per_page,
        results,
    })
}

pub(crate) fn cmd_stats(conn: &Connection, source: Option<&str>) -> Result<ApiAnalyticsResponse> {
    let source_filter = source.filter(|s| *s == "claude" || *s == "codex");

    let source_stats: Vec<ApiSourceStats> = if let Some(sf) = source_filter {
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) as msg_count, COUNT(DISTINCT session_id) as sess_count, COUNT(DISTINCT project) as proj_count
             FROM messages WHERE source = ? GROUP BY source ORDER BY msg_count DESC",
        )?;
        stmt.query_map(params![sf], |row| {
            Ok(ApiSourceStats {
                source: row.get::<_, String>(0)?,
                message_count: row.get::<_, i64>(1)?,
                session_count: row.get::<_, i64>(2)?,
                project_count: row.get::<_, i64>(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) as msg_count, COUNT(DISTINCT session_id) as sess_count, COUNT(DISTINCT project) as proj_count
             FROM messages GROUP BY source ORDER BY msg_count DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(ApiSourceStats {
                source: row.get::<_, String>(0)?,
                message_count: row.get::<_, i64>(1)?,
                session_count: row.get::<_, i64>(2)?,
                project_count: row.get::<_, i64>(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let model_stats: Vec<ApiModelStats> = if let Some(sf) = source_filter {
        let mut stmt = conn.prepare(
            "SELECT source, model, COUNT(*) as msg_count, SUM(input_tokens) as total_input, SUM(output_tokens) as total_output
             FROM messages
             WHERE model != '' AND role = 'assistant' AND source = ?
             GROUP BY source, model
             ORDER BY msg_count DESC",
        )?;
        stmt.query_map(params![sf], |row| {
            Ok(ApiModelStats {
                source: row.get::<_, String>(0)?,
                model: row.get::<_, String>(1)?,
                message_count: row.get::<_, i64>(2)?,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT source, model, COUNT(*) as msg_count, SUM(input_tokens) as total_input, SUM(output_tokens) as total_output
             FROM messages
             WHERE model != '' AND role = 'assistant'
             GROUP BY source, model
             ORDER BY msg_count DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(ApiModelStats {
                source: row.get::<_, String>(0)?,
                model: row.get::<_, String>(1)?,
                message_count: row.get::<_, i64>(2)?,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let project_stats: Vec<ApiProjectStats> = if let Some(sf) = source_filter {
        let mut stmt = conn.prepare(
            "SELECT source, project_path, COUNT(*) as cnt,
                    SUM(CASE WHEN role='user' THEN 1 ELSE 0 END) as user_cnt,
                    SUM(CASE WHEN role='assistant' THEN 1 ELSE 0 END) as asst_cnt
             FROM messages WHERE source = ?
             GROUP BY source, project_path
             ORDER BY cnt DESC",
        )?;
        stmt.query_map(params![sf], |row| {
            Ok(ApiProjectStats {
                source: row.get::<_, String>(0)?,
                project_path: row.get::<_, String>(1)?,
                total_messages: row.get::<_, i64>(2)?,
                user_messages: row.get::<_, i64>(3)?,
                assistant_messages: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT source, project_path, COUNT(*) as cnt,
                    SUM(CASE WHEN role='user' THEN 1 ELSE 0 END) as user_cnt,
                    SUM(CASE WHEN role='assistant' THEN 1 ELSE 0 END) as asst_cnt
             FROM messages
             GROUP BY source, project_path
             ORDER BY cnt DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(ApiProjectStats {
                source: row.get::<_, String>(0)?,
                project_path: row.get::<_, String>(1)?,
                total_messages: row.get::<_, i64>(2)?,
                user_messages: row.get::<_, i64>(3)?,
                assistant_messages: row.get::<_, i64>(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(ApiAnalyticsResponse {
        source_stats,
        model_stats,
        project_stats,
    })
}
