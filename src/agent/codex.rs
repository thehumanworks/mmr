use codex_app_server_sdk::{
    CodexClient, ResumeThread, Thread, ThreadOptions, TurnOptions, WsConfig,
};

use crate::types::agent::{CodexGenerateRequest, CodexGenerateResponse};

pub struct CodexAgent {
    codex: CodexClient,
}

impl CodexAgent {
    pub async fn new() -> Self {
        let codex = CodexClient::start_and_connect_ws(WsConfig::default())
            .await
            .unwrap();
        Self { codex }
    }

    fn build_thread_options(&self, developer_instructions: Option<&str>) -> ThreadOptions {
        ThreadOptions::builder()
            .skip_git_repo_check(true)
            .model("gpt-5.4-mini")
            .model_reasoning_effort(codex_app_server_sdk::ModelReasoningEffort::High)
            .developer_instructions(developer_instructions.unwrap_or(""))
            .build()
    }

    async fn start_or_resume_thread(
        &self,
        resume_thread: Option<ResumeThread>,
        developer_instructions: Option<&str>,
    ) -> Thread {
        let thread_options = self.build_thread_options(developer_instructions);
        if let Some(kind) = resume_thread {
            match kind {
                ResumeThread::Latest => self.codex.resume_latest_thread(thread_options),
                ResumeThread::ById(thread_id) => {
                    self.codex.resume_thread(thread_id, thread_options)
                }
            }
        } else {
            self.codex.start_thread(thread_options)
        }
    }

    pub async fn generate<'a>(
        &'a self,
        request: CodexGenerateRequest<'a>,
    ) -> anyhow::Result<CodexGenerateResponse> {
        let mut thread = self
            .start_or_resume_thread(request.resume_thread, request.developer_instructions)
            .await;
        let turn = thread.run(request.input, TurnOptions::default()).await?;
        Ok(CodexGenerateResponse::new(turn.final_response, thread.id()))
    }
}
