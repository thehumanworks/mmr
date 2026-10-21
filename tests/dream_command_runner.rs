//! Local subprocess fixtures; no provider accounts or session history are used.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use mmr::dream::{
    CommandCancellation, CommandDreamRunner, CommandRunnerError, CommandRunnerLimits,
    DreamEvidence, DreamRequest, DreamRunner, EvidenceAccess,
};

const VALID: &str = r#"{"observations":[{"kind":"pattern","claim":"Synthetic only.","confidence":0.5,"evidence_refs":["mmr://event/fixture"]}]}"#;

fn request() -> DreamRequest {
    DreamRequest {
        project_id: "fixture-project".into(),
        provider: "command".into(),
        model: None,
        evidence_access: EvidenceAccess::SharedSafe,
        evidence_hash: "fixture-hash".into(),
        evidence: vec![DreamEvidence {
            evidence_ref: "mmr://event/fixture".into(),
            project_id: "fixture-project".into(),
            source: "fixture".into(),
            role: "user".into(),
            event_type: "user_turn".into(),
            timestamp: "2026-10-21T00:00:00Z".into(),
            content_text: "Synthetic evidence.".into(),
        }],
    }
}

fn limits() -> CommandRunnerLimits {
    CommandRunnerLimits {
        execution_timeout: Duration::from_secs(5),
        cleanup_timeout: Duration::from_secs(1),
        ..CommandRunnerLimits::default()
    }
}

fn shell(script: &str, arguments: &[&str], limits: CommandRunnerLimits) -> CommandDreamRunner {
    CommandDreamRunner::new(
        "/bin/sh",
        ["-c", script, "fixture"]
            .into_iter()
            .chain(arguments.iter().copied()),
    )
    .with_limits(limits)
    .expect("valid test limits")
}

fn assert_reaped(path: &Path) {
    let pid: u32 = fs::read_to_string(path)
        .expect("fixture child PID")
        .trim()
        .parse()
        .expect("PID");
    let output = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .output()
        .expect("check fixture child");
    assert!(!output.status.success(), "direct child was not reaped");
}

#[test]
fn command_runner_drains_verbose_streams_while_writing_request() {
    let mut request = request();
    request.evidence[0].content_text = "q".repeat(2 * 1024 * 1024);
    // Output precedes request consumption, reproducing the original circular
    // wait. Both stdout and stderr exceed ordinary pipe capacities.
    let script = r#"
printf '%s' '{"observations":[{"kind":"pattern","claim":"Synthetic only.","confidence":0.5,"evidence_refs":["mmr://event/fixture"]}],"diagnostics":{"warnings":["'
dd if=/dev/zero bs=1024 count=1024 2>/dev/null | tr '\000' x
printf '%s' '"]}}'
dd if=/dev/zero bs=1024 count=1024 2>/dev/null | tr '\000' x >&2
cat >/dev/null
"#;
    let started = Instant::now();
    let result = shell(script, &[], limits())
        .run(&request)
        .expect("large output");
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.diagnostics.warnings[0].len(), 1024 * 1024);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn command_runner_budget_failure_reaps_child_and_hides_output() {
    for stream in ["stdout", "stderr"] {
        let tmp = tempfile::tempdir().expect("temp dir");
        let pid = tmp.path().join("pid");
        let script = format!(
            "printf '%s' \"$$\" > \"$1\"; printf '%s' 'synthetic-secret-sentinel' {}; exec sleep 30",
            if stream == "stderr" { ">&2" } else { "" }
        );
        let limits = CommandRunnerLimits {
            max_stdout_bytes: 8,
            max_stderr_bytes: 8,
            ..limits()
        };
        let err = shell(&script, &[pid.to_str().expect("pid path")], limits)
            .run(&request())
            .expect_err("output budget");
        assert_eq!(
            err.downcast_ref::<CommandRunnerError>(),
            Some(&CommandRunnerError::OutputLimit { stream, limit: 8 })
        );
        assert!(!format!("{err:#}").contains("synthetic-secret-sentinel"));
        assert_reaped(&pid);
    }
    shell(
        &format!("cat >/dev/null; printf '%s' '{VALID}'"),
        &[],
        limits(),
    )
    .run(&request())
    .expect("next invocation remains usable");
}

#[test]
fn command_runner_bounds_request_before_spawning() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let marker = tmp.path().join("spawned");
    let runner = shell(
        "touch \"$1\"",
        &[marker.to_str().expect("marker")],
        CommandRunnerLimits {
            max_request_bytes: 64,
            ..limits()
        },
    );
    let err = runner.run(&request()).expect_err("request budget");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::RequestLimit { limit: 64 })
    );
    assert!(!marker.exists());
}

#[test]
fn command_runner_deadline_includes_blocked_stdin_and_reaps_child() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid = tmp.path().join("pid");
    let mut request = request();
    request.evidence[0].content_text = "q".repeat(2 * 1024 * 1024);
    let runner = shell(
        "printf '%s' \"$$\" > \"$1\"; exec sleep 30",
        &[pid.to_str().expect("pid")],
        CommandRunnerLimits {
            execution_timeout: Duration::from_millis(250),
            ..limits()
        },
    );
    let started = Instant::now();
    let err = runner.run(&request).expect_err("blocked stdin deadline");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::Deadline)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_reaped(&pid);
}

#[test]
fn command_runner_closes_inherited_pipe_handles_and_terminates_descendants() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid = tmp.path().join("pid");
    let marker = tmp.path().join("descendant-completed");
    // Leader exits successfully while a descendant retains stdout/stderr. A
    // completion marker detects surviving execution without confusing a
    // terminated orphan awaiting OS reaping with a running process.
    let script = "cat >/dev/null; printf '%s' \"$$\" > \"$1\"; (sleep 1; touch \"$2\") & exit 0";
    let runner = shell(
        script,
        &[pid.to_str().expect("pid"), marker.to_str().expect("marker")],
        CommandRunnerLimits {
            execution_timeout: Duration::from_millis(250),
            ..limits()
        },
    );
    let started = Instant::now();
    let err = runner.run(&request()).expect_err("inherited pipe deadline");
    // A race may close stdin before writing; both paths must kill the group.
    assert!(matches!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(
            CommandRunnerError::Deadline
                | CommandRunnerError::Io {
                    operation: "write request"
                }
        )
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_reaped(&pid);
    std::thread::sleep(Duration::from_millis(1100));
    assert!(!marker.exists(), "descendant survived cleanup");
}

#[test]
fn command_runner_reports_early_exit_and_invalid_output_without_echoing_secrets() {
    for script in [
        "cat >/dev/null; printf '%s' 'synthetic-secret-sentinel' >&2; exit 7",
        "cat >/dev/null; printf '%s' 'synthetic-secret-sentinel'",
    ] {
        let err = shell(script, &[], limits())
            .run(&request())
            .expect_err("bad output");
        assert!(!format!("{err:#}").contains("synthetic-secret-sentinel"));
        assert!(matches!(
            err.downcast_ref::<CommandRunnerError>(),
            Some(
                CommandRunnerError::UnsuccessfulExit { code: Some(7) }
                    | CommandRunnerError::InvalidOutput
            )
        ));
    }
}

#[tokio::test]
async fn command_runner_can_be_called_from_a_tokio_runtime() {
    shell(
        &format!("cat >/dev/null; printf '%s' '{VALID}'"),
        &[],
        limits(),
    )
    .run(&request())
    .expect("nested runtime caller");
}

#[test]
fn command_runner_rejects_invalid_budgets() {
    let result = CommandDreamRunner::new("unused", std::iter::empty::<&str>()).with_limits(
        CommandRunnerLimits {
            execution_timeout: Duration::ZERO,
            ..limits()
        },
    );
    assert!(matches!(
        result
            .expect_err("zero timeout")
            .downcast_ref::<CommandRunnerError>(),
        Some(CommandRunnerError::InvalidLimits)
    ));
}

#[test]
#[cfg(not(target_os = "redox"))]
fn command_runner_retains_leader_pid_until_group_cleanup() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid_path = tmp.path().join("pid");
    let script = "cat >/dev/null; printf '%s' \"$$\" > \"$1\"; sleep 30 & exit 0";
    let runner = shell(
        script,
        &[pid_path.to_str().expect("pid")],
        CommandRunnerLimits {
            execution_timeout: Duration::from_millis(750),
            ..limits()
        },
    );
    let handle = std::thread::spawn(move || runner.run(&request()));
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut waitable = false;
    while Instant::now() < deadline {
        if let Ok(pid_text) = fs::read_to_string(&pid_path) {
            if let Ok(pid) = pid_text.parse::<u32>() {
                // SAFETY: only inspect this synthetic child, without reaping.
                let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        pid as libc::id_t,
                        &mut information,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result == 0 && information.si_signo != 0 {
                    waitable = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        waitable,
        "leader must remain waitable until owned group cleanup"
    );
    let err = handle
        .join()
        .expect("runner thread")
        .expect_err("pipe deadline");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::Deadline)
    );
    assert_reaped(&pid_path);
}

fn wait_for_marker(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(path.exists(), "synthetic child did not reach marker");
}

fn wait_for_pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(value) = fs::read_to_string(path) {
            if let Ok(pid) = value.trim().parse() {
                return pid;
            }
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not record a complete PID"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(not(target_os = "redox"))]
fn wait_for_exited_leader(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        // SAFETY: inspect this synthetic owned child without consuming status.
        let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut information,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 && information.si_signo != 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "fixture leader did not remain waitable"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn command_cancellation_prevents_spawn_and_stays_sticky() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let marker = tmp.path().join("spawned");
    let cancellation = CommandCancellation::default();
    let clone = cancellation.clone();
    assert!(!cancellation.is_cancelled());
    clone.request_cancel();
    cancellation.request_cancel();
    assert!(cancellation.is_cancelled() && clone.is_cancelled());
    let runner = shell(
        "cat >/dev/null; touch \"$1\"; printf '%s' \"$2\"",
        &[marker.to_str().expect("marker"), VALID],
        limits(),
    );
    for token in [&cancellation, &clone] {
        let err = runner
            .run_with_cancellation(&request(), token)
            .expect_err("pre-cancel");
        assert_eq!(
            err.downcast_ref::<CommandRunnerError>(),
            Some(&CommandRunnerError::Cancelled)
        );
    }
    assert!(!marker.exists(), "pre-cancelled run spawned child");
    assert_eq!(
        runner
            .run(&request())
            .expect("marker positive control")
            .observations
            .len(),
        1
    );
    assert!(marker.exists(), "uncancelled fixture must create marker");
}

#[test]
fn command_cancellation_interrupts_blocked_stdin_and_reaps() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid = tmp.path().join("pid");
    let runner = shell(
        "printf '%s' \"$$\" > \"$1\"; exec sleep 30",
        &[pid.to_str().expect("pid")],
        limits(),
    );
    let cancellation = CommandCancellation::default();
    let worker_token = cancellation.clone();
    let mut input = request();
    input.evidence[0].content_text = "q".repeat(2 * 1024 * 1024);
    let handle = std::thread::spawn(move || runner.run_with_cancellation(&input, &worker_token));
    wait_for_pid(&pid);
    let started = Instant::now();
    cancellation.request_cancel();
    let err = handle
        .join()
        .expect("join caller")
        .expect_err("cancelled stdin");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::Cancelled)
    );
    let elapsed = started.elapsed();
    eprintln!("CANCEL_ACK_MS={:.3}", elapsed.as_secs_f64() * 1000.0);
    assert!(elapsed < Duration::from_secs(2));
    assert_reaped(&pid);
}

#[test]
fn command_cancellation_drains_verbose_streams_then_joins_cleanup() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid = tmp.path().join("pid");
    let ready = tmp.path().join("ready");
    let script = r#"printf '%s' "$$" > "$1"; dd if=/dev/zero bs=1024 count=128 2>/dev/null; dd if=/dev/zero bs=1024 count=128 2>/dev/null | tr '\000' x >&2; touch "$2"; exec sleep 30"#;
    let runner = shell(
        script,
        &[pid.to_str().expect("pid"), ready.to_str().expect("ready")],
        limits(),
    );
    let cancellation = CommandCancellation::default();
    let worker_token = cancellation.clone();
    let handle =
        std::thread::spawn(move || runner.run_with_cancellation(&request(), &worker_token));
    wait_for_marker(&ready);
    cancellation.request_cancel();
    let err = handle
        .join()
        .expect("join caller")
        .expect_err("cancelled streams");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::Cancelled)
    );
    assert!(!format!("{err:#}").contains("xxxxx"));
    assert_reaped(&pid);
}

#[test]
#[cfg(not(target_os = "redox"))]
fn command_cancellation_stops_inherited_pipe_descendants() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid = tmp.path().join("pid");
    let marker = tmp.path().join("descendant-completed");
    let script = "cat >/dev/null; printf '%s' \"$$\" > \"$1\"; (sleep 1; touch \"$2\") & exit 0";
    let runner = shell(
        script,
        &[pid.to_str().expect("pid"), marker.to_str().expect("marker")],
        limits(),
    );
    let cancellation = CommandCancellation::default();
    let worker_token = cancellation.clone();
    let handle =
        std::thread::spawn(move || runner.run_with_cancellation(&request(), &worker_token));
    let leader = wait_for_pid(&pid);
    wait_for_exited_leader(leader);
    cancellation.request_cancel();
    let err = handle
        .join()
        .expect("join caller")
        .expect_err("cancelled inherited pipes");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::Cancelled)
    );
    assert_reaped(&pid);
    std::thread::sleep(Duration::from_millis(1100));
    assert!(
        !marker.exists(),
        "descendant survived acknowledged cancellation"
    );
}

#[test]
fn command_cancellation_isolated_handles_and_success_compatibility() {
    let cancelled = CommandCancellation::default();
    cancelled.request_cancel();
    let unrelated = CommandCancellation::default();
    let runner = shell(
        &format!("cat >/dev/null; printf '%s' '{VALID}'"),
        &[],
        limits(),
    );
    assert_eq!(
        runner
            .run_with_cancellation(&request(), &unrelated)
            .expect("unrelated run")
            .observations
            .len(),
        1
    );
    assert_eq!(
        runner
            .run(&request())
            .expect("legacy API")
            .observations
            .len(),
        1
    );
    assert!(!unrelated.is_cancelled());
}

#[tokio::test]
async fn command_cancellation_wakes_a_nested_runtime_from_another_runtime() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pid = tmp.path().join("pid");
    let runner = shell(
        "printf '%s' \"$$\" > \"$1\"; exec sleep 30",
        &[pid.to_str().expect("pid")],
        limits(),
    );
    let cancellation = CommandCancellation::default();
    let other = cancellation.clone();
    let signal_pid = pid.clone();
    let canceller = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("other runtime")
            .block_on(async {
                wait_for_pid(&signal_pid);
                for _ in 0..16 {
                    other.request_cancel();
                    tokio::task::yield_now().await;
                }
            });
    });
    let err = runner
        .run_with_cancellation(&request(), &cancellation)
        .expect_err("cross-runtime cancel");
    canceller.join().expect("canceller joined");
    assert_eq!(
        err.downcast_ref::<CommandRunnerError>(),
        Some(&CommandRunnerError::Cancelled)
    );
    assert_reaped(&pid);
}
