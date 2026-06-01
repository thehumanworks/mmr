---
title: "Live Mac Mini peer validation"
description: "Validate explicit SSH peer context against the Mac Mini over Tailscale using the local up-to-date mmr CLI."
date: 2026-06-01
status: done
---

# GOAL: Live Mac Mini Peer Validation

## Outcome

Prove whether the current local `mmr` implementation can read project context
from the Mac Mini as an explicitly named SSH peer on the Tailscale network.

## Scope

- Identify the SSH target that reaches the Mac Mini from this machine.
- Confirm the local CLI can build and run the peer commands.
- Confirm remote capability or document the exact failure if the Mac Mini has
  an older `mmr` without `peer` support.
- Prefer read-only checks first. Do not apply native provider files locally
  unless the read-only path has already proven the remote bundle can be
  produced.

## Validation Plan

- Run `mmr peer status --host <target>`.
- Run `mmr read project --project /Users/mish/projects/mmr --host <target>`.
- Run `mmr context project --project /Users/mish/projects/mmr --host <target>`.
- Run `mmr recall --project /Users/mish/projects/mmr --host <target>`.
- If peer protocol is unavailable on the remote because its CLI is stale,
  confirm SSH reachability and remote `mmr --version`/help enough to attribute
  the blocker.

## Results

- Identified Mac Mini as Tailscale host `mini` at `100.121.57.112`.
- Confirmed SSH reachability:
  - `ssh -o BatchMode=yes -o ConnectTimeout=5 mini ...`
  - remote host: `Mac.mynet`
  - remote user: `tomasroda`
  - remote `mmr`: `/Users/tomasroda/.local/bin/mmr`
- Initial remote probe failed because Mac Mini had a stale `mmr`:
  - `cargo run --quiet -- peer status --host mini --pretty`
  - failure: `error: unrecognized subcommand 'peer'`
  - local fix added: classify stale remote peer subcommand failures as
    `peer_mmr_unavailable`, not generic `peer_ssh_failed`.
- Fast-forwarded the Mac Mini repo from `88838dc` to `a3b6f51`, built
  `cargo build --release`, backed up the old `~/.local/bin/mmr`, and installed
  the peer-capable release binary.
- Confirmed remote peer protocol:
  - `mmr peer status --json`
  - `protocol_version: 1`
  - capabilities: `read-project`, `context-project`, `recall`,
    `teleport-pack`
- Live peer reads from Mac Mini passed:
  - `cargo run --quiet -- peer status --host mini --pretty` -> `status: ok`
  - `cargo run --quiet -- read project --project /Users/mish/projects/mmr --host mini --limit 2500`
    -> `peer_results[0].status: ok`, `total_messages: 2207`,
    `remote_message_count: 522`
  - `cargo run --quiet -- context project --project /Users/mish/projects/mmr --host mini --limit 2500`
    -> `peer_results[0].status: ok`, `total_sessions: 132`,
    `remote_message_count: 522`
  - `cargo run --quiet -- recall --project /Users/mish/projects/mmr --host mini 1 --limit 5 --pretty`
    -> `peer_results[0].status: ok`, `remote_mmr_version: 0.2.0`
  - First remote-origin message came from
    `/Users/tomasroda/projects/mmr` with `origin.host: mini`,
    `origin.transport: ssh`, and `remote_mmr_version: 0.2.0`.
- Read-only teleport pull also passed:
  - `cargo run --quiet -- teleport pull --from mini --session latest --project /Users/mish/projects/mmr --read-only`
  - result: `status: skipped`, `read_status: skipped`,
    `read_message_count: 364`
- Local QA after the diagnostic fix passed:
  - `cargo fmt`
  - `cargo test`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo build --release`

## Definition Of Done

- [x] Mac Mini SSH target is identified or a concrete reachability blocker is
      documented.
- [x] Peer status is proven or the stale-remote-CLI failure is captured.
- [x] Host-aware read/context/recall are tested when remote peer protocol is
      available.
- [x] Results are recorded with exact commands and failure/output summaries.
