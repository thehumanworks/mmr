# mmr teleport validation record

Status: TELEPORT-010 (NHL-331)
Date: 2026-05-26
Version: 0.2.0

This document records end-to-end proof surfaces for `mmr teleport` and how to
rerun them. Canonical behavior lives in [specs/teleport.md](../specs/teleport.md);
user workflows are in [mmr-teleport.md](mmr-teleport.md).

## Two-machine abstraction (fixtures)

Automated tests simulate source and target machines with isolated `HOME`
roots:

| Role | Fixture | Purpose |
|------|---------|---------|
| Source | Temp `HOME` with seeded Codex session (`sess-codex-1`, project `/Users/test/codex-proj`) | `pack`, `send`, `serve` |
| Target | Empty temp `HOME` (no pre-existing Codex history) | `apply`, `receive`, post-apply `mmr messages` |
| Shared | Temp directory outside both homes | Bundle file path or `file://` inbox |

Real SSH proof uses the same commands against live hosts; only transport differs.

## Proof surfaces

### 1. Local pack -> inspect -> apply -> mmr readability

Covers TELEPORT-001, TELEPORT-003, TELEPORT-004.

```bash
cargo test --test cli_contract teleport_bundle_pack_inspect_apply_round_trip
cargo test --test cli_contract teleport_apply_makes_session_visible_to_mmr_queries
```

Contract: stdout JSON `status: "ok"`; applied session visible via
`mmr messages --source codex --project <target> --session sess-codex-1`.

### 2. File inbox send -> receive (Syncthing-style)

Covers TELEPORT-006.

```bash
cargo test --test cli_contract teleport_send_file_writes_atomic_inbox_layout
cargo test --test cli_contract teleport_receive_valid_inbox_applies_and_second_receive_is_idempotent
```

Contract: atomic inbox layout (`bundle.mmr`, `bundle.sha256`, `ready`); receive
applies on target `HOME` with `--project` remap.

### 3. HTTP one-shot serve -> receive (loopback)

Covers TELEPORT-007.

```bash
cargo test --test cli_contract teleport_serve_receive_http_loopback_applies_and_serve_exits
cargo test --test cli_contract teleport_serve_invalid_token_does_not_consume_bundle
```

Contract: `mmtp://127.0.0.1:.../<token>` download, hash verify, apply; invalid
token does not consume bundle.

### 4. SSH send (dry-run and inbox fallback)

Covers TELEPORT-005.

```bash
cargo test --test cli_contract teleport_send_dry_run_reports_ssh_plan_without_remote_contact
cargo test --test cli_contract teleport_send_stages_bundle_when_remote_mmr_is_missing
```

Real host proof (manual, two machines):

```bash
# Source
mmr teleport send --session <id> --project /path/on/source --to user@target-host

# Target (after partial send, if remote mmr missing)
mmr teleport apply --to ~/.mmr/teleport/inbox/<bundle_id>/bundle.mmr --project /path/on/target
```

### 5. Resume and export

Covers TELEPORT-008.

```bash
cargo test --test cli_contract teleport_resume
cargo test --test cli_contract teleport_export
```

### 6. Full teleport contract sweep

```bash
cargo test --test cli_contract teleport
```

### 7. Opt-in benchmarks

Deterministic timing on fixture data (stderr `BENCH ... elapsed_ms=...`):

```bash
cargo test --test cli_benchmark -- --ignored --nocapture
```

Benchmarks cover:

- `benchmark_teleport_pack_inspect_apply_readability`
- `benchmark_teleport_file_send_receive_two_machine`
- `benchmark_teleport_http_loopback_receive`

## Final QA gates (release)

Run before tagging 0.2.0:

```bash
cargo fmt
cargo test
cargo test --test cli_contract teleport
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Built artifact: `target/release/mmr`.

## Out of scope (NHL-331)

- NPM publish
- `shared-safe` fidelity bundles
- Non-Codex native teleport sources
