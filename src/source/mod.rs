use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::types::{MessageRecord, SourceFilter};

mod claude;
mod codex;
mod cursor;
mod grok;
mod pi;

pub fn resolve_home_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("SIMPLEMMR_HOME") {
        return Ok(PathBuf::from(home));
    }

    dirs::home_dir().context("failed to resolve home directory")
}

pub fn load_messages() -> Result<Vec<MessageRecord>> {
    load_messages_filtered(None)
}

pub fn load_messages_filtered(source: Option<SourceFilter>) -> Result<Vec<MessageRecord>> {
    let home = resolve_home_dir()?;
    if let Some(source) = source {
        return match source {
            SourceFilter::Claude => claude::load_claude_messages(&home),
            SourceFilter::Codex => codex::load_codex_messages(&home),
            SourceFilter::Cursor => cursor::load_cursor_messages(&home),
            SourceFilter::Grok => grok::load_grok_messages(&home),
            SourceFilter::Pi => pi::load_pi_messages(&home),
        };
    }

    let (codex_result, (claude_result, (cursor_result, (grok_result, pi_result)))) = rayon::join(
        || codex::load_codex_messages(&home),
        || {
            rayon::join(
                || claude::load_claude_messages(&home),
                || {
                    rayon::join(
                        || cursor::load_cursor_messages(&home),
                        || {
                            rayon::join(
                                || grok::load_grok_messages(&home),
                                || pi::load_pi_messages(&home),
                            )
                        },
                    )
                },
            )
        },
    );

    let mut messages = codex_result?;
    messages.extend(claude_result?);
    messages.extend(cursor_result?);
    messages.extend(grok_result?);
    messages.extend(pi_result?);
    Ok(messages)
}

fn decode_project_name(project_name: &str) -> String {
    project_name.to_string()
}
