use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::types::MessageRecord;

mod claude;
mod codex;

pub fn resolve_home_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("SIMPLEMMR_HOME") {
        return Ok(PathBuf::from(home));
    }

    dirs::home_dir().context("failed to resolve home directory")
}

pub fn load_messages() -> Result<Vec<MessageRecord>> {
    let home = resolve_home_dir()?;
    let (codex_result, claude_result) = rayon::join(
        || codex::load_codex_messages(&home),
        || claude::load_claude_messages(&home),
    );

    let mut messages = codex_result?;
    messages.extend(claude_result?);
    Ok(messages)
}

fn decode_project_name(project_name: &str) -> String {
    project_name.to_string()
}
