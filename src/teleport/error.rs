use serde_json::json;

#[derive(Debug, Clone)]
pub struct TeleportFailure {
    pub command: &'static str,
    pub exit_code: i32,
    pub message: String,
    pub error_kind: Option<String>,
}

impl TeleportFailure {
    pub fn usage(command: &'static str, message: impl Into<String>) -> Self {
        Self {
            command,
            exit_code: 2,
            message: message.into(),
            error_kind: None,
        }
    }

    pub fn runtime(command: &'static str, message: impl Into<String>) -> Self {
        Self {
            command,
            exit_code: 3,
            message: message.into(),
            error_kind: None,
        }
    }

    pub fn with_error_kind(mut self, error_kind: impl Into<String>) -> Self {
        self.error_kind = Some(error_kind.into());
        self
    }

    pub fn to_stdout_json(&self, pretty: bool) -> Result<String, serde_json::Error> {
        let mut value = json!({
            "command": self.command,
            "status": "failed",
            "message": self.message,
        });
        if let Some(error_kind) = &self.error_kind {
            value["error_kind"] = json!(error_kind);
        }
        if pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        }
    }
}

impl std::fmt::Display for TeleportFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TeleportFailure {}

impl From<super::bundle::BundleLocatorError> for TeleportFailure {
    fn from(error: super::bundle::BundleLocatorError) -> Self {
        let subcommand = match &error {
            super::bundle::BundleLocatorError::MultipleLocators { subcommand }
            | super::bundle::BundleLocatorError::MissingLocator { subcommand } => {
                subcommand.as_str()
            }
        };
        let command = match subcommand {
            "inspect" => "teleport/inspect",
            "apply" => "teleport/apply",
            "resume" => "teleport/resume",
            "export" => "teleport/export",
            "send" => "teleport/send",
            "receive" => "teleport/receive",
            _ => "teleport/inspect",
        };
        Self::usage(command, error.to_string())
    }
}
