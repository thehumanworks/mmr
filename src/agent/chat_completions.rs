use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CHAT_COMPLETIONS_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone)]
pub struct ChatCompletionsClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl ChatCompletionsClient {
    pub fn from_env() -> Result<Self> {
        let api_key = required_env("OPENAI_API_KEY")?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_CHAT_COMPLETIONS_BASE_URL.to_string());

        Ok(Self::new(api_key, base_url))
    }

    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    pub async fn create(&self, request: ChatCompletionRequest<'_>) -> Result<ChatCompletionResult> {
        let response = self
            .client
            .post(self.chat_completions_url())
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .context("failed to call OpenAI-compatible chat completions API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed reading chat completions response body")?;

        if !status.is_success() {
            bail!("chat completions API error ({status}): {body}");
        }

        let parsed: ChatCompletionResponse = serde_json::from_str(&body)
            .with_context(|| format!("failed to parse chat completions response: {body}"))?;
        let text = parsed
            .first_text_choice_text()
            .ok_or_else(|| anyhow!("chat completions response did not include text output"))?;

        Ok(ChatCompletionResult {
            text,
            id: parsed.id,
            model: parsed.model,
        })
    }

    fn chat_completions_url(&self) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        if trimmed.ends_with("/chat/completions") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/chat/completions")
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<ChatCompletionMessage<'a>>,
}

impl<'a> ChatCompletionRequest<'a> {
    pub fn new(model: &'a str, system_prompt: &'a str, user_prompt: &'a str) -> Self {
        Self {
            model,
            messages: vec![
                ChatCompletionMessage::system(system_prompt),
                ChatCompletionMessage::user(user_prompt),
            ],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionMessage<'a> {
    pub role: ChatCompletionRole,
    pub content: &'a str,
}

impl<'a> ChatCompletionMessage<'a> {
    fn system(content: &'a str) -> Self {
        Self {
            role: ChatCompletionRole::System,
            content,
        }
    }

    fn user(content: &'a str) -> Self {
        Self {
            role: ChatCompletionRole::User,
            content,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatCompletionRole {
    System,
    User,
}

#[derive(Debug, Clone)]
pub struct ChatCompletionResult {
    pub text: String,
    pub id: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

impl ChatCompletionResponse {
    fn first_text_choice_text(&self) -> Option<String> {
        self.choices
            .iter()
            .filter_map(|choice| choice.message.content.as_ref()?.to_text())
            .map(|text| text.trim().to_string())
            .find(|text| !text.is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: Option<ChatResponseContent>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ChatResponseContent {
    Text(String),
    Parts(Vec<ChatResponseContentPart>),
}

impl ChatResponseContent {
    fn to_text(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::Parts(parts) => {
                let text = parts
                    .iter()
                    .filter_map(|part| part.text.as_ref().or(part.refusal.as_ref()))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("");
                (!text.is_empty()).then_some(text)
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponseContentPart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    refusal: Option<String>,
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{name} must be set"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_completions_url_appends_endpoint_to_base_url() {
        let client = ChatCompletionsClient::new("key", "https://proxy.example/v1/");
        assert_eq!(
            client.chat_completions_url(),
            "https://proxy.example/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_accepts_full_endpoint() {
        let client = ChatCompletionsClient::new("key", "https://proxy.example/v1/chat/completions");
        assert_eq!(
            client.chat_completions_url(),
            "https://proxy.example/v1/chat/completions"
        );
    }

    #[test]
    fn response_extracts_string_content() {
        let parsed: ChatCompletionResponse = serde_json::from_str(
            r#"{"id":"chatcmpl-1","model":"model-a","choices":[{"message":{"role":"assistant","content":" summary "}}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.first_text_choice_text().as_deref(), Some("summary"));
    }

    #[test]
    fn response_extracts_content_parts() {
        let parsed: ChatCompletionResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":[{"type":"text","text":"part one"},{"type":"text","text":" part two"}]}}]}"#,
        )
        .unwrap();
        assert_eq!(
            parsed.first_text_choice_text().as_deref(),
            Some("part one part two")
        );
    }
}
