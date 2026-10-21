# Explicit local assimilation jobs

`mmr assimilate project` and `mmr assimilate source` still return stateless prompt/runbook/evidence responses. The separate `assimilate jobs` namespace persists a small opt-in queue and runs a finite local command invocation.

Every job operation uses an explicit JSON config. Enqueue/run without config, or with `enabled: false`, returns `{"command":"enqueue","status":"disabled"}` (or `command: run`) before opening a Store, sealing sessions or creating a runner. Status/show/cancel require config and remain usable when execution is disabled. Explicit global `--source` is a usage error for jobs; ambient source/model/runner/key settings do not configure this namespace. Provider roots are never scanned or ingested.

Start with execution disabled and replace the paths/project ID with explicit values from the intended Store. The command must accept one `DreamRequest` JSON on stdin and return one bounded `DreamRunnerOutput` JSON. Set `enabled` only after reviewing that command and the selected evidence scope.

```json
{
  "version": 1,
  "enabled": false,
  "database_path": "/absolute/path/to/store.sqlite3",
  "project_id": "existing-store-project-id",
  "policy_version": "reviewed-projection-v1",
  "command": {
    "program": "/absolute/path/to/trusted-runner",
    "args": [],
    "version": "reviewed-runner-v1"
  },
  "budgets": {
    "max_sessions": 8,
    "max_events": 2000,
    "max_evidence_bytes": 2097152,
    "max_request_bytes": 2097152,
    "max_output_bytes": 262144,
    "max_stderr_bytes": 65536,
    "execution_timeout_ms": 60000,
    "cleanup_timeout_ms": 2000,
    "invocation_timeout_ms": 120000,
    "poll_interval_ms": 25,
    "lease_ms": 65000,
    "max_claim_attempts": 3
  }
}
```

```bash
mmr assimilate jobs enqueue --config ./jobs.json --session session-id-a --session session-id-b
mmr assimilate jobs enqueue --config ./jobs.json --session session-id-a --attest-complete reviewed-complete
mmr assimilate jobs run --config ./jobs.json --max-jobs 1
mmr assimilate jobs status --config ./jobs.json --limit 20
mmr assimilate jobs show JOB_ID --config ./jobs.json
mmr assimilate jobs cancel JOB_ID --config ./jobs.json
```

Use Store session IDs in the intended order. Sessions must be sealed as complete; imported lifecycle text and model claims cannot seal them. `--attest-complete LABEL` is a trusted caller's explicit completion decision. Duplicate enqueue returns the existing job, including its terminal or unknown state. Run defaults to one job and accepts at most eight; status defaults to twenty summaries and accepts at most one hundred. Explicit show includes the effective bounded settings, policy fingerprint, included/omitted evidence IDs and optional pending receipt; those details and proposal bodies do not appear in status listings. JSON stdout respects `--pretty`.

The current shared-safe projection selects narrative `user_turn`, `assistant_turn` and `compaction` content. Tool/unknown rows and unsafe metadata become categorical omissions. Raw source references, blob data and session metadata are never sent to the command; freeform historical roles and content remain untrusted data. Inspect show's omissions to assess coverage. Changing explicit policy/settings creates a distinct job key; this is not an automatic reconciliation or retry mechanism for an unknown job.

The explicit command is trusted code running with the operator's privileges. The worker clears its inherited environment, but this is not a filesystem/network sandbox. Do not put secrets in program arguments. Config errors and stored diagnostics are categorical; program/argv and captured arbitrary diagnostics are not copied into job records. Use synthetic/public histories for experiments and approve the exact scope before allowing a command to transmit private histories.

The worker commits execution intent before launching. A crash before that barrier may allow a bounded reclaimed claim. A crash or lost lease after it produces an unknown outcome requiring reconciliation; the job is never automatically retried. A lease fences database writes, not external effects or an orphan command after owner death. `cancel_requested` records a durable cancellation request; `may_have_executed` and `cleanup_confirmed` distinguish possible external execution from acknowledged owned cleanup. Inspect these fields rather than assuming a cancellation request stopped the external process. Never kill a PID read from the database.

Only current evidence can produce a pending/rejected proposal receipt. This workflow does not activate model-suggested memory, publish learned memory, install a daemon/scheduler or automatically select new sessions. An external scheduler may invoke the finite run command using this same explicit config.

Config schema, state transitions, bounds and crash limits are specified in `specs/assimilation-jobs.md`. Tests in `tests/assimilation_jobs_cli.rs` use temporary Stores, poisoned synthetic provider roots and fake local commands. No real credentials, histories or models are needed.
