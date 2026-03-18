use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct ClaudeFileTask {
    pub path: PathBuf,
    pub project_name: String,
    pub is_subagent: bool,
}
