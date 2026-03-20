use std::process::Command;

use anyhow::bail;

const DEFAULT_CURSOR_MODEL: &str = "composer-2-fast";

pub struct CursorAgent<'a> {
    model: &'a str,
    api_key: String,
}

impl<'a> CursorAgent<'a> {
    pub fn new(model: Option<&'a str>, api_key: Option<impl Into<String>>) -> Self {
        let model = model.unwrap_or(DEFAULT_CURSOR_MODEL);
        let api_key = api_key
            .map(|k| k.into())
            .unwrap_or(std::env::var("CURSOR_API_KEY").unwrap());
        Self { model, api_key }
    }

    fn call_cursor_agent(&self, input: &str) -> anyhow::Result<String> {
        let output = Command::new("agent")
            .args(["-f", "--approve-mcps", "--model", self.model, "-p", input])
            .env(
                "CURSOR_API_KEY",
                std::env::var("CURSOR_API_KEY").unwrap_or(self.api_key.to_string()),
            )
            .output()?;

        if !output.status.success() {
            bail!(
                "failed to call cursor agent: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout).unwrap())
    }

    pub fn generate(&self, input: &str) -> anyhow::Result<String> {
        let response = self.call_cursor_agent(input)?;
        Ok(response)
    }
}
