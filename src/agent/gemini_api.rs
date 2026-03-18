use anyhow::{Context, Result, anyhow, bail};

use crate::types::agent::{
    GeminiGenerateRequest, GeminiGenerateResponse, InteractionCreateRequest,
    InteractionCreateResponse,
};

const DEFAULT_MODEL: &str = "gemini-3.1-flash-lite-preview";
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct Gemini {
    client: reqwest::Client,
    api_key: String,
    pub model: String,
    base_url: String,
}

impl Gemini {
    pub fn new(model: Option<&str>, api_key: Option<&str>) -> Result<Self> {
        let client = reqwest::Client::new();
        let model = model.unwrap_or(DEFAULT_MODEL).to_string();
        let api_key = match api_key {
            Some(key) if !key.trim().is_empty() => key.to_string(),
            _ => resolve_api_key()?,
        };
        let base_url = std::env::var("GEMINI_API_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Self {
            client,
            api_key,
            model,
            base_url,
        })
    }

    pub async fn generate(
        &self,
        request: GeminiGenerateRequest<'_>,
    ) -> Result<GeminiGenerateResponse> {
        let url = format!("{}/interactions", self.base_url.trim_end_matches('/'));
        let payload = InteractionCreateRequest {
            model: &self.model,
            input: request.input,
            system_instruction: request.system_instruction,
        };

        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&payload)
            .send()
            .await
            .context("failed to call Gemini Interactions API")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed reading Gemini Interactions API response body")?;

        if !status.is_success() {
            bail!("Gemini Interactions API error ({status}): {body}");
        }

        let parsed: InteractionCreateResponse = serde_json::from_str(&body)
            .with_context(|| format!("failed to parse Gemini Interactions API response: {body}"))?;

        let text = parsed
            .outputs
            .into_iter()
            .filter_map(|output| output.text)
            .map(|text| text.trim().to_string())
            .find(|text| !text.is_empty())
            .ok_or_else(|| {
                anyhow!("Gemini Interactions API response did not include text output")
            })?;

        Ok(GeminiGenerateResponse { text })
    }
}

fn resolve_api_key() -> Result<String> {
    if let Ok(value) = std::env::var("GOOGLE_API_KEY")
        && !value.trim().is_empty()
    {
        return Ok(value);
    }
    if let Ok(value) = std::env::var("GEMINI_API_KEY")
        && !value.trim().is_empty()
    {
        return Ok(value);
    }

    bail!("GOOGLE_API_KEY or GEMINI_API_KEY must be set")
}
