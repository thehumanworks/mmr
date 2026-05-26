use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host_spec: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCommandPlan {
    pub probe_remote_mmr: Vec<String>,
    pub stream_apply: Vec<String>,
    pub mkdir_inbox: Vec<String>,
    pub scp_bundle: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshErrorKind {
    AuthConnect,
    Transfer,
    RemoteApply,
}

impl SshErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthConnect => "ssh_auth_connect",
            Self::Transfer => "ssh_transfer",
            Self::RemoteApply => "remote_apply",
        }
    }
}

pub fn parse_ssh_target(to: &str) -> Result<SshTarget, String> {
    let trimmed = to.trim();
    if trimmed.is_empty() {
        return Err("--to must name an SSH host (for example user@macbook)".to_string());
    }
    if trimmed.contains("://") {
        return Err(format!(
            "teleport send over SSH expects user@host; got transport-specific URL {trimmed:?}"
        ));
    }
    if trimmed.starts_with("http:") || trimmed.starts_with("https:") || trimmed.starts_with("file:")
    {
        return Err(format!(
            "teleport send over SSH expects user@host; got incompatible target {trimmed:?}"
        ));
    }
    Ok(SshTarget {
        host_spec: trimmed.to_string(),
    })
}

pub fn remote_inbox_dir(bundle_id: &str) -> String {
    format!("~/.mmr/teleport/inbox/{bundle_id}")
}

pub fn remote_inbox_bundle_path(bundle_id: &str) -> String {
    format!("~/.mmr/teleport/inbox/{bundle_id}/bundle.mmr")
}

pub fn remote_apply_command(bundle_id: &str) -> String {
    format!(
        "mmr teleport apply --to {}",
        remote_inbox_bundle_path(bundle_id)
    )
}

pub fn build_ssh_command_plan(
    target: &SshTarget,
    bundle_id: &str,
    local_bundle_path: &Path,
) -> SshCommandPlan {
    let host = target.host_spec.as_str();
    let remote_bundle = remote_inbox_bundle_path(bundle_id);
    SshCommandPlan {
        probe_remote_mmr: ssh_probe_mmr_argv(host),
        stream_apply: ssh_stream_apply_argv(host),
        mkdir_inbox: ssh_mkdir_inbox_argv(host, bundle_id),
        scp_bundle: scp_bundle_argv(host, local_bundle_path, &remote_bundle),
    }
}

pub fn ssh_base_args(host: &str) -> Vec<String> {
    vec![
        "ssh".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        host.to_string(),
    ]
}

pub fn ssh_probe_mmr_argv(host: &str) -> Vec<String> {
    let mut argv = ssh_base_args(host);
    argv.push(
        "command -v mmr >/dev/null 2>&1 && mmr --version >/dev/null 2>&1 && echo ok".to_string(),
    );
    argv
}

pub fn ssh_stream_apply_argv(host: &str) -> Vec<String> {
    let mut argv = ssh_base_args(host);
    argv.push("mmr teleport apply --to -".to_string());
    argv
}

pub fn ssh_mkdir_inbox_argv(host: &str, bundle_id: &str) -> Vec<String> {
    let mut argv = ssh_base_args(host);
    argv.push(format!("mkdir -p {}", remote_inbox_dir(bundle_id)));
    argv
}

pub fn scp_bundle_argv(host: &str, local_bundle_path: &Path, remote_path: &str) -> Vec<String> {
    vec![
        "scp".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        local_bundle_path.display().to_string(),
        format!("{host}:{remote_path}"),
    ]
}

pub fn classify_ssh_stderr(stderr: &str) -> Option<SshErrorKind> {
    let lower = stderr.to_lowercase();
    if lower.contains("permission denied")
        || lower.contains("connection refused")
        || lower.contains("connection timed out")
        || lower.contains("host key verification failed")
        || lower.contains("could not resolve hostname")
        || lower.contains("no route to host")
        || lower.contains("network is unreachable")
        || lower.contains("authentication failed")
        || lower.contains("kex_exchange_identification")
    {
        return Some(SshErrorKind::AuthConnect);
    }
    None
}

pub fn classify_scp_stderr(stderr: &str) -> Option<SshErrorKind> {
    if let Some(kind) = classify_ssh_stderr(stderr) {
        return Some(kind);
    }
    let lower = stderr.to_lowercase();
    if lower.contains("scp:") || lower.contains("lost connection") {
        return Some(SshErrorKind::Transfer);
    }
    None
}

pub fn classify_remote_apply_failure(stderr: &str, stdout: &str) -> SshErrorKind {
    if classify_ssh_stderr(stderr).is_some() {
        return SshErrorKind::AuthConnect;
    }
    if stdout.contains("\"status\":\"failed\"")
        || stderr.contains("teleport apply")
        || !stderr.trim().is_empty()
    {
        return SshErrorKind::RemoteApply;
    }
    SshErrorKind::RemoteApply
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_ssh_target_accepts_user_host() {
        let target = parse_ssh_target("bob@macbook").expect("valid ssh target");
        assert_eq!(target.host_spec, "bob@macbook");
    }

    #[test]
    fn parse_ssh_target_rejects_urls() {
        assert!(parse_ssh_target("http://100.0.0.1:8765").is_err());
        assert!(parse_ssh_target("file://~/inbox").is_err());
    }

    #[test]
    fn ssh_stream_apply_argv_uses_stdin_locator() {
        let argv = ssh_stream_apply_argv("alice@laptop");
        assert_eq!(
            argv.last().map(String::as_str),
            Some("mmr teleport apply --to -")
        );
        assert_eq!(argv.first().map(String::as_str), Some("ssh"));
    }

    #[test]
    fn scp_bundle_argv_targets_remote_inbox_path() {
        let local = PathBuf::from("/tmp/bundle.mmr");
        let argv = scp_bundle_argv(
            "bob@macbook",
            &local,
            "~/.mmr/teleport/inbox/tp:v1:abc/bundle.mmr",
        );
        assert_eq!(argv[0], "scp");
        assert_eq!(
            argv.last().map(String::as_str),
            Some("bob@macbook:~/.mmr/teleport/inbox/tp:v1:abc/bundle.mmr")
        );
    }

    #[test]
    fn classify_ssh_stderr_detects_auth_connect_failures() {
        assert_eq!(
            classify_ssh_stderr("Permission denied (publickey)."),
            Some(SshErrorKind::AuthConnect)
        );
        assert_eq!(
            classify_ssh_stderr("ssh: connect to host macbook port 22: Connection refused"),
            Some(SshErrorKind::AuthConnect)
        );
    }

    #[test]
    fn classify_scp_stderr_detects_transfer_failures() {
        assert_eq!(
            classify_scp_stderr("scp: /tmp/bundle.mmr: No such file or directory"),
            Some(SshErrorKind::Transfer)
        );
    }

    #[test]
    fn classify_remote_apply_failure_prefers_auth_errors() {
        assert_eq!(
            classify_remote_apply_failure("Permission denied (publickey).", ""),
            SshErrorKind::AuthConnect
        );
        assert_eq!(
            classify_remote_apply_failure("", "{\"status\":\"failed\"}"),
            SshErrorKind::RemoteApply
        );
    }

    #[test]
    fn build_ssh_command_plan_includes_probe_stream_and_fallback() {
        let plan = build_ssh_command_plan(
            &SshTarget {
                host_spec: "bob@macbook".to_string(),
            },
            "tp:v1:deadbeef",
            Path::new("/tmp/bundle.mmr"),
        );
        assert!(plan.probe_remote_mmr.iter().any(|arg| arg.contains("mmr")));
        assert_eq!(
            plan.stream_apply.last().map(String::as_str),
            Some("mmr teleport apply --to -")
        );
        assert!(plan.mkdir_inbox.last().unwrap().contains("tp:v1:deadbeef"));
        assert!(plan.scp_bundle.last().unwrap().contains("bundle.mmr"));
    }

    #[test]
    fn remote_apply_command_reports_exact_inbox_locator() {
        assert_eq!(
            remote_apply_command("tp:v1:abc"),
            "mmr teleport apply --to ~/.mmr/teleport/inbox/tp:v1:abc/bundle.mmr"
        );
    }
}
