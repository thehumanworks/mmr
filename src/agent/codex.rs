use codex_app_server_sdk::{
    CodexClient, ModelReasoningEffort, ThreadOptions, TurnOptions, WsConfig,
};

use crate::types::agent::{CodexGenerateRequest, CodexGenerateResponse};

const DEFAULT_CODEX_MODEL: &str = "gpt-5.4-mini";
const DEFAULT_CODEX_REASONING_EFFORT: ModelReasoningEffort = ModelReasoningEffort::Medium;

pub struct CodexAgent {
    codex: CodexClient,
}

fn build_thread_options(developer_instructions: Option<&str>) -> ThreadOptions {
    ThreadOptions::builder()
        .skip_git_repo_check(true)
        .model(DEFAULT_CODEX_MODEL)
        .model_reasoning_effort(DEFAULT_CODEX_REASONING_EFFORT)
        .developer_instructions(developer_instructions.unwrap_or(""))
        .build()
}

impl CodexAgent {
    pub async fn new() -> Self {
        let codex = CodexClient::start_and_connect_ws(WsConfig::default())
            .await
            .unwrap();
        Self { codex }
    }

    async fn start_thread(
        &self,
        developer_instructions: Option<&str>,
    ) -> codex_app_server_sdk::Thread {
        let thread_options = build_thread_options(developer_instructions);
        self.codex.start_thread(thread_options)
    }

    pub async fn generate<'a>(
        &'a self,
        request: CodexGenerateRequest<'a>,
    ) -> anyhow::Result<CodexGenerateResponse> {
        let mut thread = self.start_thread(request.developer_instructions).await;
        let turn = thread.run(request.input, TurnOptions::default()).await?;
        Ok(CodexGenerateResponse::new(turn.final_response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_options_use_updated_default_model_and_reasoning_effort() {
        let thread_options = build_thread_options(Some("follow repo instructions"));

        assert_eq!(thread_options.model.as_deref(), Some(DEFAULT_CODEX_MODEL));
        assert_eq!(
            thread_options.model_reasoning_effort,
            Some(DEFAULT_CODEX_REASONING_EFFORT)
        );
        assert_eq!(
            thread_options.developer_instructions.as_deref(),
            Some("follow repo instructions")
        );
        assert_eq!(thread_options.skip_git_repo_check, Some(true));
    }
}
