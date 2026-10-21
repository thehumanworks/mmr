//! Bounded local subprocess transport for the synchronous dream runner API.

use std::cell::Cell;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::process::{ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::watch;

use super::{
    DreamRequest, DreamRunner, DreamRunnerOutput, ENV_DREAM_COMMAND, EvidenceAccess, non_empty,
    parse_command_parts, parse_runner_output,
};

/// A shared, one-way in-process cancellation request, not a stop acknowledgement.
#[derive(Debug, Clone)]
pub struct CommandCancellation {
    requested: watch::Sender<bool>,
}

impl Default for CommandCancellation {
    fn default() -> Self {
        let (requested, _) = watch::channel(false);
        Self { requested }
    }
}

impl CommandCancellation {
    /// Request cancellation for every clone. This handle cannot be reset.
    pub fn request_cancel(&self) {
        self.requested.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.requested.borrow()
    }

    fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            return Err(CommandRunnerError::Cancelled.into());
        }
        Ok(())
    }

    async fn cancelled(&self) {
        let mut receiver = self.requested.subscribe();
        loop {
            // Copy the boolean so no watch borrow survives across an await.
            let requested = *receiver.borrow_and_update();
            if requested || receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

/// Resource budgets for one command invocation, including its pipe I/O.
#[derive(Debug, Clone, Copy)]
pub struct CommandRunnerLimits {
    pub execution_timeout: Duration,
    pub cleanup_timeout: Duration,
    pub max_request_bytes: usize,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl Default for CommandRunnerLimits {
    fn default() -> Self {
        Self {
            execution_timeout: Duration::from_secs(60),
            cleanup_timeout: Duration::from_secs(2),
            max_request_bytes: 16 * 1024 * 1024,
            max_stdout_bytes: 8 * 1024 * 1024,
            max_stderr_bytes: 2 * 1024 * 1024,
        }
    }
}

impl CommandRunnerLimits {
    fn validate(self) -> Result<()> {
        if self.execution_timeout.is_zero()
            || self.cleanup_timeout.is_zero()
            || self.max_request_bytes == 0
            || self.max_stdout_bytes == 0
            || self.max_stderr_bytes == 0
            || Instant::now().checked_add(self.execution_timeout).is_none()
            || Instant::now().checked_add(self.cleanup_timeout).is_none()
        {
            return Err(CommandRunnerError::InvalidLimits.into());
        }
        Ok(())
    }
}

/// Safe error details: command arguments and captured output are never included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRunnerError {
    InvalidLimits,
    RequestLimit { limit: usize },
    OutputLimit { stream: &'static str, limit: usize },
    Deadline,
    Cancelled,
    CleanupDeadline,
    ChildOwnershipLost,
    UnsupportedExitInspection,
    Io { operation: &'static str },
    UnsuccessfulExit { code: Option<i32> },
    InvalidOutput,
}

impl fmt::Display for CommandRunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => write!(
                formatter,
                "dream command limits must be positive and finite"
            ),
            Self::RequestLimit { limit } => write!(
                formatter,
                "dream command request_limit exceeded ({limit} bytes)"
            ),
            Self::OutputLimit { stream, limit } => write!(
                formatter,
                "dream command output_limit exceeded on {stream} ({limit} bytes)"
            ),
            Self::Deadline => write!(formatter, "dream command execution deadline exceeded"),
            Self::Cancelled => write!(formatter, "dream command cancelled"),
            Self::CleanupDeadline => write!(formatter, "dream command cleanup deadline exceeded"),
            Self::ChildOwnershipLost => write!(
                formatter,
                "dream command child ownership was lost to another reaper"
            ),
            Self::UnsupportedExitInspection => write!(
                formatter,
                "dream command requires Unix waitid with WNOWAIT support"
            ),
            Self::Io { operation } => write!(formatter, "dream command failed to {operation}"),
            Self::UnsuccessfulExit { code } => {
                write!(formatter, "dream command failed with exit code {code:?}")
            }
            Self::InvalidOutput => write!(formatter, "dream command returned invalid output"),
        }
    }
}

impl std::error::Error for CommandRunnerError {}

#[derive(Debug, Clone)]
pub struct CommandDreamRunner {
    program: OsString,
    args: Vec<OsString>,
    limits: CommandRunnerLimits,
    clean_environment: bool,
}

impl CommandDreamRunner {
    /// Construct a command without introducing shell parsing or interpretation.
    pub fn new(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            limits: CommandRunnerLimits::default(),
            clean_environment: false,
        }
    }

    pub fn with_limits(mut self, limits: CommandRunnerLimits) -> Result<Self> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }

    /// Opt in to an empty inherited environment and a fixed system PATH.
    /// This is configuration isolation, not a filesystem or network sandbox.
    pub fn with_clean_environment(mut self) -> Self {
        self.clean_environment = true;
        self
    }

    pub fn from_env() -> Result<Self> {
        let command = std::env::var(ENV_DREAM_COMMAND)
            .ok()
            .and_then(|value| non_empty(Some(&value)).map(str::to_string))
            .ok_or_else(|| anyhow!("{ENV_DREAM_COMMAND} is not set"))?;
        let (program, args) = parse_command_parts(&command)?;
        Ok(Self::new(program, args))
    }

    async fn run_command(
        &self,
        request: &[u8],
        deadline: Instant,
        cancellation: &CommandCancellation,
    ) -> Result<Vec<u8>> {
        self.run_command_with_group_cleanup(
            request,
            deadline,
            cancellation,
            ProcessGroup::terminate,
        )
        .await
    }

    async fn run_command_with_group_cleanup(
        &self,
        request: &[u8],
        deadline: Instant,
        cancellation: &CommandCancellation,
        terminate_group: impl FnOnce(&mut ProcessGroup) -> Result<()>,
    ) -> Result<Vec<u8>> {
        cancellation.check()?;
        let mut command = Command::new(&self.program);
        if self.clean_environment {
            command.env_clear().env("PATH", "/usr/bin:/bin");
        }
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        // A dedicated process group contains ordinary descendants, including
        // children holding inherited stdout/stderr after the leader exits.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|_| CommandRunnerError::Io {
            operation: "spawn child",
        })?;
        let mut group = ProcessGroup::for_child(&child);
        let stdin = child.stdin.take().ok_or(CommandRunnerError::Io {
            operation: "open stdin",
        })?;
        let stdout = child.stdout.take().ok_or(CommandRunnerError::Io {
            operation: "open stdout",
        })?;
        let stderr = child.stderr.take().ok_or(CommandRunnerError::Io {
            operation: "open stderr",
        })?;
        let operation = async {
            let write = async move {
                let mut stdin = stdin;
                stdin
                    .write_all(request)
                    .await
                    .map_err(|_| CommandRunnerError::Io {
                        operation: "write request",
                    })?;
                stdin.shutdown().await.map_err(|_| CommandRunnerError::Io {
                    operation: "close stdin",
                })?;
                Ok::<_, anyhow::Error>(())
            };
            let (_, stdout, _, ()) = tokio::try_join!(
                write,
                read_bounded(stdout, self.limits.max_stdout_bytes, "stdout", true),
                read_bounded(stderr, self.limits.max_stderr_bytes, "stderr", false),
                wait_for_exit(&mut child, &group.ownership),
            )?;
            Ok(stdout)
        };
        // These are owned futures, not detached read/write tasks: timeout or an
        // early error drops all three pipe handles before bounded cleanup.
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(CommandRunnerError::Cancelled.into()),
            result = tokio::time::timeout_at(deadline.into(), operation) => {
                result.unwrap_or_else(|_| Err(CommandRunnerError::Deadline.into()))
            }
        };
        // Consume the group identity before final reaping. Even if signaling
        // fails, always attempt bounded direct-child cleanup. An external
        // reaper invalidates both group and direct-child PID ownership.
        let group_result = terminate_group(&mut group);
        if group.ownership.get() == ChildOwnership::Lost {
            return Err(CommandRunnerError::ChildOwnershipLost.into());
        }
        let status =
            cleanup_direct_child(&mut child, self.limits.cleanup_timeout, group_result).await?;
        if result.is_ok() && !status.success() {
            return Err(CommandRunnerError::UnsuccessfulExit {
                code: status.code(),
            }
            .into());
        }
        result
    }
}

impl CommandDreamRunner {
    /// Run with a cooperative request; return only after bounded child cleanup.
    /// Cancellation may race completed output and is not a durable commit fence.
    pub fn run_with_cancellation(
        &self,
        request: &DreamRequest,
        cancellation: &CommandCancellation,
    ) -> Result<DreamRunnerOutput> {
        cancellation.check()?;
        if request.evidence_access != EvidenceAccess::SharedSafe {
            anyhow::bail!("command dream runner requires shared-safe evidence");
        }
        self.limits.validate()?;
        let request_bytes = request_bytes(request, self.limits.max_request_bytes)?;
        cancellation.check()?;
        let deadline = Instant::now() + self.limits.execution_timeout;
        // The public trait is synchronous and is also called from Tokio hosts.
        // Keep our current-thread runtime on its own scoped OS thread so nested
        // runtime use does not panic. Async callers should still use
        // spawn_blocking, because this synchronous API waits for completion.
        let stdout = thread::scope(|scope| {
            thread::Builder::new()
                .name("mmr-dream-command".to_string())
                .spawn_scoped(scope, || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| CommandRunnerError::Io {
                            operation: "create runtime",
                        })?;
                    runtime.block_on(self.run_command(&request_bytes, deadline, cancellation))
                })
                .map_err(|_| CommandRunnerError::Io {
                    operation: "start worker",
                })?
                .join()
                .map_err(|_| CommandRunnerError::Io {
                    operation: "join worker",
                })?
        })?;
        cancellation.check()?;
        let stdout = String::from_utf8(stdout).map_err(|_| CommandRunnerError::InvalidOutput)?;
        let output = parse_runner_output(&stdout, request.evidence_refs())
            .map_err(|_| CommandRunnerError::InvalidOutput)?;
        cancellation.check()?;
        Ok(output)
    }
}

impl DreamRunner for CommandDreamRunner {
    fn run(&self, request: &DreamRequest) -> Result<DreamRunnerOutput> {
        self.run_with_cancellation(request, &CommandCancellation::default())
    }
}

fn request_bytes(request: &DreamRequest, limit: usize) -> Result<Vec<u8>> {
    struct BoundedRequest {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for BoundedRequest {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("request limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = BoundedRequest {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, request)
        .and_then(|_| output.write_all(b"\n").map_err(serde_json::Error::io))
        .map_err(|_| CommandRunnerError::RequestLimit { limit })?;
    Ok(output.bytes)
}

async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
    stream: &'static str,
    retain: bool,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| CommandRunnerError::Io {
                operation: "read output",
            })?;
        if count == 0 {
            return Ok(output);
        }
        if count > limit.saturating_sub(total) {
            return Err(CommandRunnerError::OutputLimit { stream, limit }.into());
        }
        total += count;
        if retain {
            output.extend_from_slice(&buffer[..count]);
        }
    }
}

// Keep the Unix group leader waitable until group termination. Reaping it while
// descendants retain a pipe would release its PID/group ID for reuse before a
// later timeout signals the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildOwnership {
    Running,
    Exited,
    Lost,
}

#[cfg(all(unix, not(target_os = "redox")))]
fn inspect_exit(id: u32) -> io::Result<bool> {
    // SAFETY: siginfo_t is a C POD output structure initialized to zero; id
    // identifies our child. WNOWAIT inspects without consuming its exit status.
    let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            id as libc::id_t,
            &mut information,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == 0 {
        Ok(information.si_signo != 0)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "redox")]
fn inspect_exit(_id: u32) -> io::Result<bool> {
    Err(io::Error::from_raw_os_error(libc::ENOSYS))
}

#[cfg(unix)]
fn inspection_error(error: &io::Error, ownership: &Cell<ChildOwnership>) -> CommandRunnerError {
    match error.raw_os_error() {
        Some(libc::ECHILD) => {
            ownership.set(ChildOwnership::Lost);
            CommandRunnerError::ChildOwnershipLost
        }
        Some(libc::EINVAL | libc::ENOSYS) => CommandRunnerError::UnsupportedExitInspection,
        _ => CommandRunnerError::Io {
            operation: "inspect child exit",
        },
    }
}

#[cfg(unix)]
async fn wait_for_exit(child: &mut Child, ownership: &Cell<ChildOwnership>) -> Result<()> {
    let id = child.id().ok_or(CommandRunnerError::Io {
        operation: "identify child",
    })?;
    loop {
        match inspect_exit(id) {
            Ok(true) => {
                ownership.set(ChildOwnership::Exited);
                return Ok(());
            }
            Ok(false) => {}
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
            Err(error) => return Err(inspection_error(&error, ownership).into()),
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[cfg(not(unix))]
async fn wait_for_exit(child: &mut Child, ownership: &Cell<ChildOwnership>) -> Result<()> {
    child.wait().await.map_err(|_| CommandRunnerError::Io {
        operation: "wait for child",
    })?;
    ownership.set(ChildOwnership::Exited);
    Ok(())
}

async fn cleanup_direct_child(
    child: &mut Child,
    timeout: Duration,
    group_result: Result<()>,
) -> Result<ExitStatus> {
    let _ = child.start_kill();
    // Failure/deadline while reaping takes precedence because cleanup itself
    // could not be confirmed. Otherwise report group failure after reaping.
    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| CommandRunnerError::CleanupDeadline)?
        .map_err(|_| CommandRunnerError::Io {
            operation: "reap child",
        })?;
    group_result?;
    Ok(status)
}

struct ProcessGroup {
    #[cfg(unix)]
    id: Option<i32>,
    ownership: Cell<ChildOwnership>,
}

impl ProcessGroup {
    fn for_child(_child: &Child) -> Self {
        Self {
            #[cfg(unix)]
            id: Some(_child.id().expect("new child has an id") as i32),
            ownership: Cell::new(ChildOwnership::Running),
        }
    }

    fn terminate(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            // Consume before any final reaping; Drop must never signal a reused
            // identity after a direct-child wait, including failure paths.
            let Some(id) = self.id.take() else {
                return Ok(());
            };
            if self.ownership.get() == ChildOwnership::Lost {
                return Err(CommandRunnerError::ChildOwnershipLost.into());
            }
            // Recheck ownership before signaling. Another child reaper is not
            // supported, but ECHILD must safely disable all PID-based kills.
            let mut inspected = false;
            for _ in 0..3 {
                match inspect_exit(id as u32) {
                    Ok(true) => {
                        self.ownership.set(ChildOwnership::Exited);
                        inspected = true;
                        break;
                    }
                    Ok(false) => {
                        inspected = true;
                        break;
                    }
                    Err(error) if error.raw_os_error() == Some(libc::EINTR) => continue,
                    Err(error) => return Err(inspection_error(&error, &self.ownership).into()),
                }
            }
            if !inspected {
                return Err(CommandRunnerError::Io {
                    operation: "inspect child exit",
                }
                .into());
            }
            // SAFETY: waitid confirmed this leader is still our waitable child
            // in a dedicated process group; negative PID targets that group.
            let result = unsafe { libc::kill(-id, libc::SIGKILL) };
            if result == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            // Darwin returns EPERM when only an unreaped zombie remains in
            // the group. Any ordinary live same-UID member is signallable.
            #[cfg(target_vendor = "apple")]
            if error.raw_os_error() == Some(libc::EPERM)
                && self.ownership.get() == ChildOwnership::Exited
            {
                return Ok(());
            }
            Err(CommandRunnerError::Io {
                operation: "terminate process group",
            }
            .into())
        }
        #[cfg(not(unix))]
        Ok(())
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn command_cancellation_signal_failure_still_reaps_direct_child() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let marker = tmp.path().join("started");
        let runner = CommandDreamRunner::new(
            "/bin/sh",
            [
                OsString::from("-c"),
                OsString::from("printf '%s' \"$$\" > \"$1\"; exec sleep 30"),
                OsString::from("synthetic"),
                marker.as_os_str().to_owned(),
            ],
        );
        let cancellation = CommandCancellation::default();
        let requester = cancellation.clone();
        let request = async {
            tokio::time::timeout(Duration::from_secs(3), async {
                while !marker.exists() {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            })
            .await
            .expect("synthetic child started");
            requester.request_cancel();
        };
        let child_id = Cell::new(None);
        let run = runner.run_command_with_group_cleanup(
            b"{}\n",
            Instant::now() + Duration::from_secs(5),
            &cancellation,
            |group| {
                // Inject failure at the same boundary as group signaling and
                // consume the ID, so Drop cannot turn it into a success.
                child_id.set(group.id.take());
                Err(CommandRunnerError::Io {
                    operation: "terminate process group",
                }
                .into())
            },
        );
        let (result, ()) = tokio::join!(run, request);
        assert!(cancellation.is_cancelled());
        let err = result.expect_err("injected cleanup failure");
        assert_eq!(
            err.downcast_ref::<CommandRunnerError>(),
            Some(&CommandRunnerError::Io {
                operation: "terminate process group"
            })
        );
        let id = child_id.get().expect("started child ID");
        let mut status = 0;
        // SAFETY: this synthetic PID belonged to the runner; the check cannot
        // signal a process and verifies that direct cleanup consumed its exit.
        assert_eq!(unsafe { libc::waitpid(id, &mut status, libc::WNOHANG) }, -1);
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[tokio::test]
    async fn command_runner_signal_failure_still_reaps_direct_child() {
        let mut child = Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("synthetic child");
        let failure: Result<()> = Err(CommandRunnerError::Io {
            operation: "terminate process group",
        }
        .into());
        let err = cleanup_direct_child(&mut child, Duration::from_secs(1), failure)
            .await
            .expect_err("signal failure");
        assert_eq!(
            err.downcast_ref::<CommandRunnerError>(),
            Some(&CommandRunnerError::Io {
                operation: "terminate process group"
            })
        );
        assert!(child.try_wait().expect("wait result").is_some());
    }

    #[test]
    fn command_runner_exit_inspection_errors_are_safe_and_explicit() {
        let ownership = Cell::new(ChildOwnership::Running);
        assert_eq!(
            inspection_error(&io::Error::from_raw_os_error(libc::ENOSYS), &ownership),
            CommandRunnerError::UnsupportedExitInspection
        );
        assert_eq!(
            inspection_error(&io::Error::from_raw_os_error(libc::EINVAL), &ownership),
            CommandRunnerError::UnsupportedExitInspection
        );
        assert_eq!(
            inspection_error(&io::Error::from_raw_os_error(libc::ECHILD), &ownership),
            CommandRunnerError::ChildOwnershipLost
        );
        assert_eq!(ownership.get(), ChildOwnership::Lost);
        let mut group = ProcessGroup {
            id: Some(i32::MAX),
            ownership,
        };
        assert!(group.terminate().is_err());
        assert!(group.id.is_none());
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    #[tokio::test]
    async fn command_cancellation_survives_concurrent_requests_before_subscribe() {
        let cancellation = CommandCancellation::default();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let clone = cancellation.clone();
                std::thread::spawn(move || {
                    for _ in 0..8 {
                        clone.request_cancel();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("requester joined");
        }
        assert!(cancellation.is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), cancellation.cancelled())
            .await
            .expect("sticky request wakes late subscriber");
    }
}
