use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "https://api.morphllm.com/v1";
const DEFAULT_MODEL: &str = "morph-compactor";

#[derive(Debug, Clone)]
pub struct MorphCompactClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl MorphCompactClient {
    pub fn from_env() -> Result<Self> {
        let api_key = required_env("MORPHLLM_API_KEY")?;
        let base_url = std::env::var("MORPHLLM_BASE_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Self::new(api_key, base_url))
    }

    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    pub async fn compact(&self, request: CompactRequest<'_>) -> Result<CompactResult> {
        let response = self
            .client
            .post(self.compact_url())
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .context("failed to call Morph Compact API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed reading Morph Compact response body")?;

        if !status.is_success() {
            bail!("Morph Compact API error ({status}): {body}");
        }

        let parsed: CompactApiResponse = serde_json::from_str(&body)
            .with_context(|| format!("failed to parse Morph Compact response: {body}"))?;
        let output = parsed
            .output
            .as_ref()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| anyhow!("Morph Compact response did not include output"))?;

        Ok(CompactResult {
            id: parsed.id,
            model: parsed.model.unwrap_or_else(|| request.model.to_string()),
            output,
            messages: parsed.messages,
            usage: parsed.usage,
        })
    }

    fn compact_url(&self) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        if trimmed.ends_with("/compact") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/compact")
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CompactRequest<'a> {
    pub input: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression_ratio: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserve_recent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_line_ranges: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_markers: Option<bool>,
    pub model: &'a str,
}

impl<'a> CompactRequest<'a> {
    pub fn new(input: &'a str, model: &'a str) -> Self {
        Self {
            input,
            query: None,
            compression_ratio: None,
            preserve_recent: None,
            include_line_ranges: None,
            include_markers: None,
            model,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactResult {
    pub id: Option<String>,
    pub model: String,
    pub output: String,
    pub messages: Vec<CompactResponseMessage>,
    pub usage: Option<CompactUsage>,
}

#[derive(Debug, Deserialize)]
struct CompactApiResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    messages: Vec<CompactResponseMessage>,
    #[serde(default)]
    usage: Option<CompactUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResponseMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub compacted_line_ranges: Vec<CompactLineRange>,
    #[serde(default)]
    pub kept_line_ranges: Vec<CompactLineRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactLineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub compression_ratio: Option<f64>,
    #[serde(default)]
    pub processing_time_ms: Option<u64>,
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{name} must be set"))
}

pub fn default_compact_model() -> &'static str {
    DEFAULT_MODEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_url_appends_endpoint_to_base_url() {
        let client = MorphCompactClient::new("key", "https://api.morphllm.com/v1/");
        assert_eq!(client.compact_url(), "https://api.morphllm.com/v1/compact");
    }

    #[test]
    fn compact_url_accepts_full_endpoint() {
        let client = MorphCompactClient::new("key", "https://proxy.example/v1/compact");
        assert_eq!(client.compact_url(), "https://proxy.example/v1/compact");
    }

    #[test]
    fn compact_response_parses_usage_and_ranges() {
        let parsed: CompactApiResponse = serde_json::from_str(
            r#"{"id":"cmpr-1","model":"morph-compactor","output":"kept text","messages":[{"role":"user","content":"kept text","compacted_line_ranges":[{"start":2,"end":5}],"kept_line_ranges":[]}],"usage":{"input_tokens":100,"output_tokens":42,"compression_ratio":0.42,"processing_time_ms":10}}"#,
        )
        .unwrap();
        assert_eq!(parsed.output.as_deref(), Some("kept text"));
        assert_eq!(
            parsed.messages[0].compacted_line_ranges[0].start, 2,
            "line ranges must parse"
        );
        assert_eq!(
            parsed.usage.unwrap().compression_ratio,
            Some(0.42),
            "usage ratio must parse"
        );
    }
}
