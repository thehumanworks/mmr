use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::types::SourceFilter;

pub const PEER_PROTOCOL_VERSION: u32 = 1;
const SSH_CONNECT_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub original: String,
    pub host_spec: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerProjectIdentity {
    pub local_path: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_remotes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRequestLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecallRequest {
    pub n: u32,
    pub include_newest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerProjectRequest {
    pub protocol_version: u32,
    pub project: PeerProjectIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceFilter>,
    #[serde(default)]
    pub all: bool,
    pub limits: PeerRequestLimits,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall: Option<PeerRecallRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerTeleportPackRequest {
    pub protocol_version: u32,
    pub project: PeerProjectIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub latest: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStatusResponse {
    pub command: String,
    pub status: String,
    pub protocol_version: u32,
    pub mmr_version: String,
    pub capabilities: Vec<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PeerCommandError {
    pub host: String,
    pub error_kind: &'static str,
    pub message: String,
}

impl std::fmt::Display for PeerCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PeerCommandError {}

pub fn parse_ssh_target(value: &str) -> Result<SshTarget> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("SSH target is required");
    }
    reject_shell_metacharacters(trimmed)?;

    if let Some(rest) = trimmed.strip_prefix("ssh://") {
        return parse_ssh_url_target(trimmed, rest);
    }
    if trimmed.contains("://") {
        bail!("unsupported peer target URL {trimmed:?}; expected SSH target");
    }

    let (host_spec, port) = split_host_port(trimmed)?;
    validate_host_spec(&host_spec)?;
    Ok(SshTarget {
        original: trimmed.to_string(),
        host_spec,
        port,
    })
}

pub fn ssh_argv(target: &SshTarget, remote_args: &[&str]) -> Vec<String> {
    let mut argv = vec![
        "ssh".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        format!("ConnectTimeout={SSH_CONNECT_TIMEOUT_SECS}"),
    ];
    if let Some(port) = target.port {
        argv.push("-p".to_string());
        argv.push(port.to_string());
    }
    argv.push("--".to_string());
    argv.push(target.host_spec.clone());
    argv.extend(remote_args.iter().map(|arg| (*arg).to_string()));
    argv
}

pub fn peer_status() -> PeerStatusResponse {
    PeerStatusResponse {
        command: "peer/status".to_string(),
        status: "ok".to_string(),
        protocol_version: PEER_PROTOCOL_VERSION,
        mmr_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: vec![
            "read-project".to_string(),
            "context-project".to_string(),
            "recall".to_string(),
            "teleport-pack".to_string(),
        ],
        sources: vec![
            "codex".to_string(),
            "claude".to_string(),
            "cursor".to_string(),
            "grok".to_string(),
            "pi".to_string(),
        ],
    }
}

pub fn run_peer_json<T, R>(
    host: &str,
    remote_args: &[&str],
    request: Option<&T>,
) -> std::result::Result<R, PeerCommandError>
where
    T: Serialize,
    R: DeserializeOwned,
{
    let target = parse_ssh_target(host).map_err(|error| PeerCommandError {
        host: host.to_string(),
        error_kind: "peer_target_invalid",
        message: error.to_string(),
    })?;
    let argv = ssh_argv(&target, remote_args);
    let output = run_ssh_command(&argv, request).map_err(|message| PeerCommandError {
        host: host.to_string(),
        error_kind: "peer_ssh_failed",
        message,
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let kind = classify_peer_failure(stderr);
        return Err(PeerCommandError {
            host: host.to_string(),
            error_kind: kind,
            message: if stderr.is_empty() {
                format!("ssh to {host} failed with status {}", output.status)
            } else {
                format!("ssh to {host} failed: {stderr}")
            },
        });
    }

    serde_json::from_slice(&output.stdout).map_err(|error| PeerCommandError {
        host: host.to_string(),
        error_kind: "peer_protocol_error",
        message: format!("parse peer response from {host}: {error}"),
    })
}

fn run_ssh_command<T: Serialize>(
    argv: &[String],
    request: Option<&T>,
) -> std::result::Result<std::process::Output, String> {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if request.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn {}: {error}", argv[0]))?;

    if let Some(request) = request {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "open ssh stdin".to_string())?;
        serde_json::to_writer(&mut *stdin, request)
            .map_err(|error| format!("serialize peer request: {error}"))?;
        stdin
            .write_all(b"\n")
            .map_err(|error| format!("write peer request: {error}"))?;
    }

    child
        .wait_with_output()
        .map_err(|error| format!("wait for ssh: {error}"))
}

fn parse_ssh_url_target(original: &str, rest: &str) -> Result<SshTarget> {
    let path_start = rest.find('/').unwrap_or(rest.len());
    if path_start != rest.len() {
        bail!("SSH target URL must not include a path: {original:?}");
    }
    let authority = &rest[..path_start];
    if authority.is_empty() {
        bail!("SSH target URL must include a host: {original:?}");
    }
    let (host_spec, port) = split_host_port(authority)?;
    validate_host_spec(&host_spec)?;
    Ok(SshTarget {
        original: original.to_string(),
        host_spec,
        port,
    })
}

fn split_host_port(value: &str) -> Result<(String, Option<u16>)> {
    let Some((host, port_str)) = value.rsplit_once(':') else {
        return Ok((value.to_string(), None));
    };
    if host.is_empty() || port_str.is_empty() {
        bail!("invalid SSH target {value:?}");
    }
    if port_str.chars().all(|ch| ch.is_ascii_digit()) {
        let port = port_str
            .parse::<u16>()
            .with_context(|| format!("invalid SSH port {port_str:?}"))?;
        return Ok((host.to_string(), Some(port)));
    }
    if value.contains('@') {
        bail!("invalid SSH target port {port_str:?} in {value:?}");
    }
    Ok((value.to_string(), None))
}

fn reject_shell_metacharacters(value: &str) -> Result<()> {
    if value.chars().any(char::is_whitespace) {
        bail!("SSH target must not contain whitespace");
    }
    let forbidden = [';', '|', '&', '`', '$', '(', ')', '<', '>', '"', '\''];
    if value.chars().any(|ch| forbidden.contains(&ch)) {
        bail!("SSH target contains unsupported shell metacharacters");
    }
    Ok(())
}

fn validate_host_spec(host_spec: &str) -> Result<()> {
    if host_spec.is_empty() {
        bail!("SSH target must include a host");
    }
    if host_spec.starts_with('-') {
        bail!("SSH target must not start with '-'");
    }
    Ok(())
}

fn classify_peer_failure(stderr: &str) -> &'static str {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("mmr: command not found")
        || lower.contains("mmr: not found")
        || lower.contains("command not found")
        || lower.contains("no such file or directory")
        || lower.contains("unknown command")
        || lower.contains("unsupported peer protocol")
    {
        return "peer_mmr_unavailable";
    }
    "peer_ssh_failed"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_target_accepts_alias_user_host_and_port() {
        assert_eq!(
            parse_ssh_target("studio").expect("alias"),
            SshTarget {
                original: "studio".to_string(),
                host_spec: "studio".to_string(),
                port: None,
            }
        );
        assert_eq!(
            parse_ssh_target("mish@studio:2222").expect("user host port"),
            SshTarget {
                original: "mish@studio:2222".to_string(),
                host_spec: "mish@studio".to_string(),
                port: Some(2222),
            }
        );
        assert_eq!(
            parse_ssh_target("ssh://mish@studio:22").expect("ssh url"),
            SshTarget {
                original: "ssh://mish@studio:22".to_string(),
                host_spec: "mish@studio".to_string(),
                port: Some(22),
            }
        );
    }

    #[test]
    fn parse_ssh_target_rejects_shell_fragments() {
        assert!(parse_ssh_target("studio;rm").is_err());
        assert!(parse_ssh_target("studio && other").is_err());
        assert!(parse_ssh_target("-oProxyCommand=sh").is_err());
        assert!(parse_ssh_target("mish@studio:notaport").is_err());
        assert!(parse_ssh_target("http://studio").is_err());
    }

    #[test]
    fn ssh_argv_uses_batch_mode_timeout_port_and_fixed_remote_args() {
        let target = parse_ssh_target("mish@studio:2222").expect("target");
        let argv = ssh_argv(&target, &["mmr", "peer", "status", "--json"]);
        assert_eq!(argv[0], "ssh");
        assert!(argv.contains(&"BatchMode=yes".to_string()));
        assert!(argv.contains(&"ConnectTimeout=5".to_string()));
        assert!(argv.windows(2).any(|items| items == ["-p", "2222"]));
        assert!(argv.windows(2).any(|items| items == ["--", "mish@studio"]));
        assert_eq!(&argv[argv.len() - 4..], ["mmr", "peer", "status", "--json"]);
    }
}
