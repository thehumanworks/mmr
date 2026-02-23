use anyhow::Result;
use duckdb::{params, Connection};

pub(crate) type SearchRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    String,
);

#[allow(clippy::type_complexity)]
pub(crate) fn run_search(
    conn: &Connection,
    query: &str,
    project_filter: &str,
    source_filter: &str,
    per_page: usize,
    offset: usize,
) -> (usize, Vec<SearchRow>) {
    if let Ok(result) = try_fts_search(conn, query, project_filter, source_filter, per_page, offset)
    {
        return result;
    }
    like_search(conn, query, project_filter, source_filter, per_page, offset)
}

#[allow(clippy::type_complexity)]
fn try_fts_search(
    conn: &Connection,
    query: &str,
    project_filter: &str,
    source_filter: &str,
    per_page: usize,
    offset: usize,
) -> Result<(usize, Vec<SearchRow>)> {
    let mut where_clauses = vec!["score IS NOT NULL".to_string()];
    let mut bind_values: Vec<String> = vec![query.to_string()];

    if !project_filter.is_empty() {
        where_clauses.push("project = ?".to_string());
        bind_values.push(project_filter.to_string());
    }
    if source_filter == "claude" || source_filter == "codex" {
        where_clauses.push("source = ?".to_string());
        bind_values.push(source_filter.to_string());
    }

    let where_str = where_clauses.join(" AND ");

    let count_sql = format!(
        "SELECT COUNT(*) FROM (SELECT *, fts_main_messages.match_bm25(id, ?) AS score FROM messages) t WHERE {}",
        where_str
    );
    let search_sql = format!(
        "SELECT id, project, project_path, session_id, role, content_text, model, timestamp, is_subagent, source
         FROM (SELECT *, fts_main_messages.match_bm25(id, ?) AS score FROM messages) t
         WHERE {}
         ORDER BY score DESC
         LIMIT {} OFFSET {}",
        where_str, per_page, offset
    );

    let count: i64 = match bind_values.len() {
        1 => conn.query_row(&count_sql, params![bind_values[0]], |row| row.get(0))?,
        2 => conn.query_row(&count_sql, params![bind_values[0], bind_values[1]], |row| {
            row.get(0)
        })?,
        3 => conn.query_row(
            &count_sql,
            params![bind_values[0], bind_values[1], bind_values[2]],
            |row| row.get(0),
        )?,
        _ => unreachable!(),
    };

    let mut stmt = conn.prepare(&search_sql)?;
    let rows: Vec<SearchRow> = match bind_values.len() {
        1 => stmt
            .query_map(params![bind_values[0]], map_search_row)?
            .filter_map(|r| r.ok())
            .collect(),
        2 => stmt
            .query_map(params![bind_values[0], bind_values[1]], map_search_row)?
            .filter_map(|r| r.ok())
            .collect(),
        3 => stmt
            .query_map(
                params![bind_values[0], bind_values[1], bind_values[2]],
                map_search_row,
            )?
            .filter_map(|r| r.ok())
            .collect(),
        _ => unreachable!(),
    };

    Ok((count as usize, rows))
}

#[allow(clippy::type_complexity)]
fn like_search(
    conn: &Connection,
    query: &str,
    project_filter: &str,
    source_filter: &str,
    per_page: usize,
    offset: usize,
) -> (usize, Vec<SearchRow>) {
    let mut where_clauses = vec!["content_text LIKE '%' || ? || '%'".to_string()];
    let mut bind_values: Vec<String> = vec![query.to_string()];

    if !project_filter.is_empty() {
        where_clauses.push("project = ?".to_string());
        bind_values.push(project_filter.to_string());
    }
    if source_filter == "claude" || source_filter == "codex" {
        where_clauses.push("source = ?".to_string());
        bind_values.push(source_filter.to_string());
    }

    let where_str = where_clauses.join(" AND ");

    let count_sql = format!("SELECT COUNT(*) FROM messages WHERE {}", where_str);
    let search_sql = format!(
        "SELECT id, project, project_path, session_id, role, content_text, model, timestamp, is_subagent, source
         FROM messages WHERE {}
         ORDER BY timestamp DESC
         LIMIT {} OFFSET {}",
        where_str, per_page, offset
    );

    let count: i64 = match bind_values.len() {
        1 => conn
            .query_row(&count_sql, params![bind_values[0]], |row| row.get(0))
            .unwrap_or(0),
        2 => conn
            .query_row(&count_sql, params![bind_values[0], bind_values[1]], |row| {
                row.get(0)
            })
            .unwrap_or(0),
        3 => conn
            .query_row(
                &count_sql,
                params![bind_values[0], bind_values[1], bind_values[2]],
                |row| row.get(0),
            )
            .unwrap_or(0),
        _ => unreachable!(),
    };

    let mut stmt = conn.prepare(&search_sql).unwrap();
    let rows: Vec<SearchRow> = match bind_values.len() {
        1 => stmt
            .query_map(params![bind_values[0]], map_search_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect(),
        2 => stmt
            .query_map(params![bind_values[0], bind_values[1]], map_search_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect(),
        3 => stmt
            .query_map(
                params![bind_values[0], bind_values[1], bind_values[2]],
                map_search_row,
            )
            .unwrap()
            .filter_map(|r| r.ok())
            .collect(),
        _ => unreachable!(),
    };

    (count as usize, rows)
}

fn map_search_row(row: &duckdb::Row) -> duckdb::Result<SearchRow> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, String>(6)?,
        row.get::<_, String>(7)?,
        row.get::<_, bool>(8)?,
        row.get::<_, String>(9)?,
    ))
}
