# mmr session sharing validation

This record names the proof surfaces for source-side sharing, destination-side
import, the peer status check, and provider event ingestion.

## Targeted Contract Tests

```bash
cargo test --test peer_import_loopback_contract -- --nocapture
cargo test --test cli_contract peer -- --nocapture
cargo test --test cli_contract share -- --nocapture
cargo test --test cli_contract import -- --nocapture
```

Expected coverage:

- `--remote` and `--host` are rejected on read/query surfaces.
- `peer status --host` reports structured failures for SSH, missing `mmr`,
  incompatible protocol and a missing `peer` subcommand.
- public transport-name namespace is rejected.
- `share session` writes file inbox bundles, reports SSH plans, and starts a
  one-shot HTTP locator.
- `import session --from` and `peer status --host` run the real `mmr peer`
  server through a fake `ssh` against a second seeded home, in read-only and
  apply modes.
- `import bundle` reads and applies paths, stdin, and locator-shaped inputs.
- `ingest events` imports normalized source history.
- old top-level event-import argv is rejected.

## Full Local QA

Run the repository verification loop before claiming completion:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

The benchmark suite remains opt-in:

```bash
cargo test --test cli_benchmark -- --ignored --nocapture
```

## Live Peer Smoke

Use a trusted SSH/Tailscale target with an up-to-date `mmr` on `PATH`.

```bash
mmr peer status --host mini
mmr import session --from mini --session latest --project /Users/mish/projects/mmr --read-only
mmr share session latest --project /Users/mish/projects/mmr --to mini --dry-run
```

Notes:

- `peer status --host` is a hidden peer-protocol diagnostic and intentionally
  keeps its implementation flag.
- Read/query commands are local only.
- The live import smoke is read-only so it does not mutate native provider files
  on the destination.
- The live share smoke uses `--dry-run` unless the operator explicitly wants to
  write to a remote inbox.

## Documentation Check

Run the removed-name search from the active goal document across public docs,
skills, source, and tests.

Expected remaining matches are limited to private implementation module names,
hidden peer protocol names, negative contract tests, or historical completed
goal artifacts outside the public docs.
