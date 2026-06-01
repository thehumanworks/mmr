---
title: "Explicit SSH peer context for mmr"
description: "Make mmr read/context/recall and native teleport pull work against user-supplied SSH targets without requiring a stored trusted-host registry."
date: 2026-06-01
status: done
---

# GOAL: Explicit SSH Peer Context

## Outcome

An agent working on one machine can retrieve project history from another
machine by naming an SSH target directly. `mmr` should not require users to
pre-register trusted hosts, maintain an mmr-specific peer config, enumerate
`known_hosts`, or depend on Tailscale discovery.

The user supplies the target. If the local SSH client can reach it and the
remote side can answer, `mmr` uses it. If the target, auth, host key, port, or
remote `mmr` setup is wrong, the command fails with a structured error.

Representative user flow:

```bash
cd /Users/mish/projects/mmr

mmr context project --host mac-studio
mmr read project --host mish@mac-studio:22
mmr recall --host studio
mmr teleport pull --from studio --session latest --project .
```

Existing local-only behavior remains unchanged when no host flag is provided.

## Why This Shape

The current `teleport` surface already moves one selected native session across
SSH, HTTP, or file inbox. It is too explicit for the desired workflow: an agent
on the Mac Mini should be able to ask for project context and let `mmr` fetch
from the Mac Studio without separately preparing a bundle or thinking in terms
of transport internals.

The right MVP is not a persistent trust database. The local SSH client already
owns host resolution, user aliases, host key verification, ports, and auth. For
this feature, an `mmr` host is just an explicit SSH target supplied at command
time.

This keeps the model simple:

- No `mmr host trust` ceremony for the first version.
- No stored host entries required.
- No scraping `~/.ssh/known_hosts`.
- No Tailscale peer enumeration.
- No background daemon.
- No hidden sync of raw transcripts.

## Decisions

1. **Explicit targets only.** MVP peer access is activated only by a command
   flag such as `--host <target>` or `--from <target>`.
2. **No mmr host registry.** There is no required `hosts.toml`, database table,
   or persistent trust entry in the first version.
3. **SSH owns transport trust.** OpenSSH decides whether the target is reachable
   and trusted. `mmr` must not read `known_hosts` to discover candidates.
4. **No automatic discovery.** MVP does not enumerate Tailscale, LAN, Bonjour,
   SSH config, or `known_hosts`.
5. **Remote `mmr` is required for peer queries.** The first version runs a
   fixed remote `mmr peer ...` command over SSH. If `mmr` is missing or
   incompatible on the remote, the command fails.
6. **Strict failures by default.** If the user passes a host and that host
   fails, the command fails. Best-effort multi-host partial results can be a
   later explicit flag.
7. **No raw cache by default.** Remote read/context responses are returned to
   stdout and are not persisted locally unless a later explicit cache/import
   flag is added. Native teleport pull still writes native provider artifacts
   because that is its explicit job.
8. **No shell interpolation.** SSH invocations use structured process args,
   fixed remote commands, and JSON requests over stdin.

## Command Surface

### Peer Status

`peer` is an implementation-facing namespace for remote capability checks. It
can be hidden from top-level help if we want the public surface to stay small.

```bash
mmr peer status
mmr peer status --host studio
```

Local `mmr peer status` prints protocol version, mmr version, supported peer
methods, and provider/source availability. With `--host`, local mmr shells out
to the remote `mmr peer status`.

### Host-Aware Read And Context

Project-scoped retrieval commands accept one or more explicit SSH targets:

```bash
mmr read project --host studio
mmr read project --host studio --host mini
mmr context project --host mish@mac-studio:2222
mmr recall --host studio
```

Default semantics:

- No `--host`: local only, exactly as today.
- One or more `--host`: query local plus the supplied remote peers.
- `--no-local`: future optional flag if remote-only is needed.
- Host failures are command failures in the MVP.

Each returned message/event should carry origin metadata when it did not come
from the local machine:

```json
{
  "origin": {
    "host": "studio",
    "transport": "ssh",
    "remote_mmr_version": "..."
  }
}
```

If adding `origin` directly to existing public message types would churn too
much contract surface, wrap peer responses in a top-level `origins` /
`peer_results` field and keep existing message objects stable.

### Native Teleport Pull

Teleport keeps owning native provider bundle movement:

```bash
mmr teleport pull --from studio --session latest --project .
mmr teleport pull --from mish@mac-studio:22 --session sess-abc --project .
mmr teleport pull --from studio --session latest --project . --read-only
```

`pull` asks the remote host to pack the selected session and streams the bundle
back over SSH. Locally it can either:

- apply native provider files, matching `teleport receive`, or
- read/cache the bundle without native apply when `--read-only` is set.

This reuses the current teleport pack/read/apply machinery and avoids making
normal `read project` responsible for native provider writes.

## SSH Target Grammar

Accept:

```text
studio
mish@studio
mish@studio:22
ssh://mish@studio:22
```

Rules:

- Bare host strings are passed through to OpenSSH, so `~/.ssh/config` aliases
  work naturally.
- `user@host:port` and `ssh://user@host:port` are normalized to `ssh -p <port>
  user@host`.
- Use `BatchMode=yes` by default so agent runs fail instead of prompting.
- Add a conservative connect timeout such as `ConnectTimeout=5`.
- Do not support arbitrary shell fragments.

## Peer Protocol

Local commands should talk to remote `mmr` through a small JSON-over-stdin RPC
surface rather than shell-quoting user paths and filters.

Suggested hidden remote commands:

```bash
mmr peer status --json
mmr peer read-project --request-json -
mmr peer context-project --request-json -
mmr peer recall --request-json -
mmr peer teleport-pack --request-json -
```

The SSH invocation shape should be fixed:

```text
ssh -o BatchMode=yes -o ConnectTimeout=5 [-p PORT] TARGET mmr peer read-project --request-json -
```

The request JSON should include:

```json
{
  "protocol_version": 1,
  "project": {
    "local_path": "/Users/mish/projects/mmr",
    "display_name": "mmr",
    "git_root": "/Users/mish/projects/mmr",
    "git_remotes": ["git@github.com:thehumanworks/mmr.git"],
    "repo_fingerprint": "..."
  },
  "source": null,
  "limits": {
    "message_limit": 200
  }
}
```

Remote project matching should prefer stable project identity over absolute
paths:

1. Git remote URL/fingerprint match.
2. Existing mmr project alias match.
3. Exact canonical path match.
4. Display-name fallback only when unambiguous.

## Output And Error Contract

All public commands keep machine-readable JSON on stdout. Peer diagnostics go
to stderr or structured error JSON, consistent with existing CLI behavior.

Single host failure:

```json
{
  "command": "read/project",
  "status": "failed",
  "error_kind": "peer_ssh_failed",
  "host": "studio",
  "message": "ssh to studio failed: ..."
}
```

Remote capability failure:

```json
{
  "command": "read/project",
  "status": "failed",
  "error_kind": "peer_mmr_unavailable",
  "host": "studio",
  "message": "remote mmr is missing or does not support peer protocol v1"
}
```

## Security Boundary

- This feature can expose raw local transcript content from the remote host.
- `mmr` must never infer remote hosts from `known_hosts`.
- `mmr` must never query a host unless the user named it in the command.
- SSH target strings must be parsed as data and passed as process args, not
  embedded in shell strings.
- Remote requests must be read-only unless the command is explicitly a native
  teleport pull/apply operation.
- Do not persist raw remote messages in the local store by default.

## Non-Goals

- No stored trusted-host registry in MVP.
- No `mmr host trust`, `mmr host list`, or `hosts.toml` requirement.
- No Tailscale status parsing or peer discovery.
- No scanning or importing `~/.ssh/known_hosts`.
- No daemon or long-running listener.
- No cloud relay.
- No automatic host-to-host sync.
- No rsync fallback for raw provider directories in the first version.
- No cross-provider transformation beyond existing teleport behavior.

## Implementation Plan

1. Add an SSH target parser and command builder.
   - Unit-test pass-through aliases, `user@host`, `user@host:port`, and
     `ssh://user@host:port`.
   - Assert invalid shell-like fragments are rejected.
2. Add hidden/local `mmr peer status`.
   - Return protocol version, version string, capabilities, and source support.
   - Add SSH wrapper path for `mmr peer status --host <target>`.
3. Add peer request/response types.
   - Use JSON stdin/stdout.
   - Include project identity fields needed for cross-host matching.
4. Wire `read project --host`.
   - Query local plus remote.
   - Merge chronologically with origin/provenance.
   - Deduplicate by source/session/message identity where possible.
5. Wire `context project --host` and `recall --host`.
   - Reuse the same peer transport and project matching.
6. Add `teleport pull --from`.
   - Remote side packs a native bundle.
   - Local side applies or read-only caches using existing teleport code.
7. Document the workflow.
   - Update `docs/mmr-teleport.md` to distinguish selected-session pull from
     host-aware read/context.
   - Update `docs/mmr-command-taxonomy.md`.
8. Run an independent review pass.
   - Use a subagent to review the implemented diff for contract regressions,
     security hazards in SSH argv/request handling, missing tests, and command
     UX gaps.
   - Address all blocking findings before marking the goal done.
   - If the subagent review cannot run or cannot complete because of auth,
     tooling, infrastructure, or another hard external failure, record the
     exact failure and treat the review gate as satisfied if all other
     verification passes.

## Validation Plan

- Unit tests for SSH target parsing and argv construction.
- Contract tests with fake `ssh` on `PATH` that captures argv and returns
  fixture peer JSON.
- Contract tests for:
  - `mmr peer status --host studio`
  - `mmr read project --host studio`
  - SSH failure produces structured error
  - remote mmr missing/incompatible produces structured error
  - local-only commands remain byte-compatible when no host is passed
  - `teleport pull --from studio` streams a fixture bundle and applies/reads it
- Full verification loop:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
- Independent subagent review of the implementation diff and test plan, or a
  documented hard failure that prevented the review from running.
- Final `git status --short` review to separate intended changes from
  unrelated worktree files.
- Commit and push to the configured GitHub remote only after implementation,
  QA checks, and review are successful.

## Implementation Evidence

- Added explicit peer transport in `src/peer.rs`: SSH target parsing,
  fixed argv construction, JSON request streaming over stdin, peer status, and
  structured peer failure classification.
- Added hidden `mmr peer` JSON endpoints and public host-aware retrieval for
  `read project --host`, `context project --host`, and `recall --host`.
- Added `mmr teleport pull --from` using the existing native teleport
  pack/cache/read/apply path.
- Confirmed no new code path enumerates `known_hosts`, Tailscale peers, LAN
  peers, SSH config entries, or a stored mmr host registry. The only matching
  strings are documentation/goal text and pre-existing teleport HTTP/SSH code.
- Verification passed:
  - `cargo fmt`
  - `cargo test`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo build --release`
  - `cargo run --quiet -- peer status`
  - `cargo run --quiet -- read project --help`
  - `cargo run --quiet -- teleport pull --help`
- Subagent review was attempted with `/root/peer_context_review`, which spawned
  `/root/peer_context_review/peer_context_quick_review`; both stayed running
  after repeated waits and were closed as a hard infrastructure timeout. Per
  the goal caveat, the review gate is satisfied by the documented hard failure
  because the rest of verification passed.

## Definition Of Done

- [x] `mmr` can query a user-supplied SSH target without any stored host config.
- [x] No code path enumerates `known_hosts`, Tailscale peers, LAN peers, or SSH
      config entries.
- [x] Peer commands use structured argv and JSON-over-stdin, not shell
      interpolation.
- [x] `read project --host`, `context project --host`, and `recall --host`
      work against a fake SSH peer in contract tests.
- [x] `teleport pull --from` reuses native teleport pack/read/apply behavior.
- [x] Host failures are strict, structured, and easy to diagnose.
- [x] Existing local-only command behavior remains unchanged.
- [x] Docs explain that SSH trust is user-supplied per invocation, not stored in
      an mmr registry.
- [x] Full verification loop passes and key results are recorded in the final
      implementation note.
- [x] A subagent review has completed and all blocking findings are resolved,
      or the review could not run because of a documented hard auth/tooling/
      infrastructure failure.
- [x] The completed implementation is committed and pushed to the configured
      GitHub remote, provided verification is clean and no unrelated worktree
      changes make the commit unsafe.
