use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use super::TeleportStatus;
use super::error::TeleportFailure;
use super::file::{FileSendPlan, is_file_url, parse_file_url, write_bundle_to_inbox};
use super::manifest::TeleportFidelity;
use super::pack::{PackOptions, PackResponse, PackSessionSummary, pack_session};
use super::ssh::{
    SshCommandPlan, SshErrorKind, SshTarget, build_ssh_command_plan, classify_remote_apply_failure,
    classify_scp_stderr, classify_ssh_stderr, parse_ssh_target, remote_apply_command,
    remote_inbox_bundle_path, remote_inbox_dir, ssh_probe_mmr_argv, ssh_stream_apply_argv,
};
use crate::messages::service::QueryService;
use crate::types::SourceFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendTransport {
    Auto,
    Ssh,
    File,
}

impl SendTransport {
    pub fn from_arg(value: &str) -> Result<Self, TeleportFailure> {
        match value {
            "auto" => Ok(Self::Auto),
            "ssh" => Ok(Self::Ssh),
            "file" => Ok(Self::File),
            "http" => Err(TeleportFailure::usage(
                "teleport/send",
                "teleport send does not support HTTP transport yet",
            )),
            other => Err(TeleportFailure::usage(
                "teleport/send",
                format!("unsupported --transport value {other:?}; expected auto, ssh, or file"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedSendTransport {
    Ssh,
    File,
}

#[derive(Debug, Clone)]
pub struct SendOptions {
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub source_filter: Option<SourceFilter>,
    pub to: String,
    pub transport: SendTransport,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct SendRemoteApplySummary {
    pub attempted: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendPlannedCommands {
    pub probe_remote_mmr: Vec<String>,
    pub stream_apply: Vec<String>,
    pub mkdir_inbox: Vec<String>,
    pub scp_bundle: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub bundle_id: String,
    pub bundle_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub transport: &'static str,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbox_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_inbox: Option<String>,
    pub session: PackSessionSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_apply: Option<SendRemoteApplySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fidelity: Option<TeleportFidelity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_commands: Option<SendPlannedCommands>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_inbox: Option<FileSendPlan>,
    pub dry_run: bool,
}

pub fn send_session(
    service: &QueryService,
    options: SendOptions,
) -> Result<SendResponse, TeleportFailure> {
    let to = options.to.trim();
    if to.is_empty() {
        return Err(TeleportFailure::usage(
            "teleport/send",
            "--to is required for teleport send",
        ));
    }

    let resolved = resolve_send_transport(to, options.transport)?;

    let pack = pack_session(
        service,
        PackOptions {
            session_id: options.session_id,
            project: options.project,
            source_filter: options.source_filter,
            output_path: None,
            fidelity: TeleportFidelity::Native,
            dry_run: options.dry_run,
        },
    )?;

    match resolved {
        ResolvedSendTransport::File => send_via_file(to, pack, options.dry_run),
        ResolvedSendTransport::Ssh => send_via_ssh(to, pack, options.dry_run),
    }
}

fn resolve_send_transport(
    to: &str,
    transport: SendTransport,
) -> Result<ResolvedSendTransport, TeleportFailure> {
    let is_file = is_file_url(to);
    match transport {
        SendTransport::Auto => {
            if is_file {
                Ok(ResolvedSendTransport::File)
            } else {
                Ok(ResolvedSendTransport::Ssh)
            }
        }
        SendTransport::File => {
            if !is_file {
                return Err(TeleportFailure::usage(
                    "teleport/send",
                    format!("--transport file requires a file:// target; got {to:?}"),
                ));
            }
            Ok(ResolvedSendTransport::File)
        }
        SendTransport::Ssh => {
            if is_file {
                return Err(TeleportFailure::usage(
                    "teleport/send",
                    "--transport ssh is incompatible with file:// targets",
                ));
            }
            Ok(ResolvedSendTransport::Ssh)
        }
    }
}

fn send_via_file(
    to: &str,
    pack: PackResponse,
    dry_run: bool,
) -> Result<SendResponse, TeleportFailure> {
    let inbox_root = parse_file_url(to)?;
    let bundle_path = pack.bundle_path.clone().ok_or_else(|| {
        TeleportFailure::runtime("teleport/send", "pack did not report bundle_path")
    })?;
    let sha256 = match pack.sha256.clone() {
        Some(sha256) => sha256,
        None if dry_run => String::new(),
        None => {
            return Err(TeleportFailure::runtime(
                "teleport/send",
                "pack did not report sha256",
            ));
        }
    };
    let planned = write_bundle_to_inbox(
        &inbox_root,
        &pack.bundle_id,
        Path::new(&bundle_path),
        &sha256,
        dry_run,
    )?;

    Ok(SendResponse {
        command: "teleport/send",
        status: TeleportStatus::Ok,
        bundle_id: pack.bundle_id,
        bundle_path: planned.bundle_path.clone(),
        sha256: if sha256.is_empty() {
            None
        } else {
            Some(sha256)
        },
        bytes: pack.bytes,
        transport: "file",
        to: to.to_string(),
        inbox_path: Some(planned.inbox_path.clone()),
        ready_path: Some(planned.ready_path.clone()),
        remote_inbox: None,
        session: pack.session,
        remote_apply: None,
        fidelity: Some(pack.fidelity),
        next_command: None,
        planned_commands: None,
        planned_inbox: if dry_run { Some(planned) } else { None },
        dry_run,
    })
}

fn send_via_ssh(
    to: &str,
    pack: PackResponse,
    dry_run: bool,
) -> Result<SendResponse, TeleportFailure> {
    let target =
        parse_ssh_target(to).map_err(|message| TeleportFailure::usage("teleport/send", message))?;

    let bundle_path = pack.bundle_path.clone().ok_or_else(|| {
        TeleportFailure::runtime("teleport/send", "pack did not report bundle_path")
    })?;
    let bundle_path = PathBuf::from(bundle_path);
    let remote_inbox = remote_inbox_dir(&pack.bundle_id);
    let plan = build_ssh_command_plan(&target, &pack.bundle_id, &bundle_path);

    if dry_run {
        return Ok(build_ssh_send_response(
            pack,
            SendResponseContext {
                to: target.host_spec,
                remote_inbox,
                remote_apply: SendRemoteApplySummary {
                    attempted: false,
                    status: "not_attempted".to_string(),
                    mode: None,
                },
                next_command: None,
                planned_commands: Some(plan),
                dry_run: true,
                status: TeleportStatus::Ok,
            },
        ));
    }

    execute_ssh_send(target, pack, &bundle_path, &plan)
}

fn execute_ssh_send(
    target: SshTarget,
    pack: PackResponse,
    bundle_path: &Path,
    plan: &SshCommandPlan,
) -> Result<SendResponse, TeleportFailure> {
    let host = target.host_spec.as_str();
    let bundle_id = pack.bundle_id.clone();
    let remote_inbox = remote_inbox_dir(&bundle_id);

    let remote_has_mmr = probe_remote_mmr(host)?;

    if remote_has_mmr {
        match stream_apply(host, bundle_path) {
            Ok(remote_status) => {
                return Ok(build_ssh_send_response(
                    pack,
                    SendResponseContext {
                        to: host.to_string(),
                        remote_inbox,
                        remote_apply: SendRemoteApplySummary {
                            attempted: true,
                            status: remote_status,
                            mode: Some("stream_apply".to_string()),
                        },
                        next_command: None,
                        planned_commands: None,
                        dry_run: false,
                        status: TeleportStatus::Ok,
                    },
                ));
            }
            Err(failure) => return Err(failure),
        }
    }

    copy_bundle_to_remote_inbox(host, bundle_path, &bundle_id, plan)?;
    eprintln!(
        "share: remote mmr not found; staged bundle in {}",
        remote_inbox_bundle_path(&bundle_id)
    );
    Ok(build_ssh_send_response(
        pack,
        SendResponseContext {
            to: host.to_string(),
            remote_inbox,
            remote_apply: SendRemoteApplySummary {
                attempted: false,
                status: "not_attempted".to_string(),
                mode: Some("inbox_copy".to_string()),
            },
            next_command: Some(remote_apply_command(&bundle_id)),
            planned_commands: None,
            dry_run: false,
            status: TeleportStatus::Partial,
        },
    ))
}

struct SendResponseContext {
    to: String,
    remote_inbox: String,
    remote_apply: SendRemoteApplySummary,
    next_command: Option<String>,
    planned_commands: Option<SshCommandPlan>,
    dry_run: bool,
    status: TeleportStatus,
}

fn build_ssh_send_response(pack: PackResponse, ctx: SendResponseContext) -> SendResponse {
    SendResponse {
        command: "teleport/send",
        status: ctx.status,
        bundle_id: pack.bundle_id,
        bundle_path: pack.bundle_path.unwrap_or_default(),
        sha256: pack.sha256,
        bytes: pack.bytes,
        transport: "ssh",
        to: ctx.to,
        inbox_path: None,
        ready_path: None,
        remote_inbox: Some(ctx.remote_inbox),
        session: pack.session,
        remote_apply: Some(ctx.remote_apply),
        fidelity: Some(pack.fidelity),
        next_command: ctx.next_command,
        planned_commands: ctx.planned_commands.map(|plan| SendPlannedCommands {
            probe_remote_mmr: plan.probe_remote_mmr,
            stream_apply: plan.stream_apply,
            mkdir_inbox: plan.mkdir_inbox,
            scp_bundle: plan.scp_bundle,
        }),
        planned_inbox: None,
        dry_run: ctx.dry_run,
    }
}

fn probe_remote_mmr(host: &str) -> Result<bool, TeleportFailure> {
    let argv = ssh_probe_mmr_argv(host);
    let output = run_command(&argv[0], &argv[1..])
        .map_err(|error| ssh_command_failure("teleport/send", SshErrorKind::AuthConnect, error))?;
    if let Some(kind) = classify_ssh_stderr(&output.stderr) {
        return Err(ssh_failure(
            "teleport/send",
            kind,
            format!("ssh probe failed for {host}: {}", output.stderr.trim()),
        ));
    }
    if !output.status.success() {
        return Ok(false);
    }
    Ok(output.stdout.trim() == "ok")
}

fn stream_apply(host: &str, bundle_path: &Path) -> Result<String, TeleportFailure> {
    let bundle_bytes = fs::read(bundle_path).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!("read local bundle {}: {error}", bundle_path.display()),
        )
    })?;

    let argv = ssh_stream_apply_argv(host);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            ssh_command_failure(
                "teleport/send",
                SshErrorKind::AuthConnect,
                format!("spawn ssh stream apply: {error}"),
            )
        })?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(&bundle_bytes).map_err(|error| {
            TeleportFailure::runtime(
                "teleport/send",
                format!("write bundle bytes to ssh stdin: {error}"),
            )
            .with_error_kind(SshErrorKind::Transfer.as_str())
        })?;
    }

    let output = child.wait_with_output().map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!("wait for ssh stream apply: {error}"),
        )
        .with_error_kind(SshErrorKind::Transfer.as_str())
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Some(kind) = classify_ssh_stderr(&stderr) {
        return Err(ssh_failure(
            "teleport/send",
            kind,
            format!("ssh stream apply failed for {host}: {}", stderr.trim()),
        ));
    }

    if !output.status.success() {
        let kind = classify_remote_apply_failure(&stderr, &stdout);
        return Err(ssh_failure(
            "teleport/send",
            kind,
            format!(
                "remote mmr import bundle failed for {host}: {}",
                stderr.trim()
            ),
        ));
    }

    let apply_json: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|error| {
        TeleportFailure::runtime(
            "teleport/send",
            format!("parse remote apply JSON: {error}; stdout={stdout}"),
        )
        .with_error_kind(SshErrorKind::RemoteApply.as_str())
    })?;

    if apply_json.get("status").and_then(|value| value.as_str()) == Some("failed") {
        return Err(ssh_failure(
            "teleport/send",
            SshErrorKind::RemoteApply,
            format!(
                "remote mmr import bundle returned failed status: {}",
                apply_json
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown error")
            ),
        ));
    }

    Ok(apply_json
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("ok")
        .to_string())
}

fn copy_bundle_to_remote_inbox(
    host: &str,
    bundle_path: &Path,
    bundle_id: &str,
    plan: &SshCommandPlan,
) -> Result<(), TeleportFailure> {
    let mkdir = &plan.mkdir_inbox;
    let mkdir_output = run_command(&mkdir[0], &mkdir[1..])
        .map_err(|error| ssh_command_failure("teleport/send", SshErrorKind::AuthConnect, error))?;
    if let Some(kind) = classify_ssh_stderr(&mkdir_output.stderr) {
        return Err(ssh_failure(
            "teleport/send",
            kind,
            format!(
                "ssh mkdir inbox failed for {host}: {}",
                mkdir_output.stderr.trim()
            ),
        ));
    }
    if !mkdir_output.status.success() {
        return Err(ssh_failure(
            "teleport/send",
            SshErrorKind::Transfer,
            format!(
                "ssh mkdir inbox failed for {host}: {}",
                mkdir_output.stderr.trim()
            ),
        ));
    }

    let scp = &plan.scp_bundle;
    let scp_output = run_command(&scp[0], &scp[1..])
        .map_err(|error| ssh_command_failure("teleport/send", SshErrorKind::Transfer, error))?;
    if let Some(kind) = classify_scp_stderr(&scp_output.stderr) {
        return Err(ssh_failure(
            "teleport/send",
            kind,
            format!("scp bundle failed for {host}: {}", scp_output.stderr.trim()),
        ));
    }
    if !scp_output.status.success() {
        return Err(ssh_failure(
            "teleport/send",
            SshErrorKind::Transfer,
            format!("scp bundle failed for {host}: {}", scp_output.stderr.trim()),
        ));
    }

    let _ = bundle_id;
    let _ = bundle_path;
    Ok(())
}

struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_command(program: &str, args: &[String]) -> Result<CommandOutput, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("spawn {program}: {error}"))?;
    Ok(CommandOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn ssh_failure(
    command: &'static str,
    kind: SshErrorKind,
    message: impl Into<String>,
) -> TeleportFailure {
    TeleportFailure::runtime(command, message).with_error_kind(kind.as_str())
}

fn ssh_command_failure(
    command: &'static str,
    kind: SshErrorKind,
    message: impl Into<String>,
) -> TeleportFailure {
    TeleportFailure::runtime(command, message).with_error_kind(kind.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_send_transport_auto_infers_file_from_url() {
        assert_eq!(
            resolve_send_transport("file:///tmp/inbox", SendTransport::Auto).expect("file"),
            ResolvedSendTransport::File
        );
        assert_eq!(
            resolve_send_transport("bob@macbook", SendTransport::Auto).expect("ssh"),
            ResolvedSendTransport::Ssh
        );
    }

    #[test]
    fn resolve_send_transport_file_rejects_non_file_target() {
        assert!(resolve_send_transport("bob@macbook", SendTransport::File).is_err());
    }

    #[test]
    fn resolve_send_transport_ssh_rejects_file_target() {
        assert!(resolve_send_transport("file:///tmp/inbox", SendTransport::Ssh).is_err());
    }
}
