# mmr session sharing validation

This record names the proof surfaces for remote reads, source-side sharing,
destination-side import, and provider event ingestion.

## Targeted Contract Tests

```bash
cargo test --test cli_contract remote -- --nocapture
cargo test --test cli_contract share -- --nocapture
cargo test --test cli_contract import -- --nocapture
```

Expected coverage:

- public `--remote` works on read/query surfaces and `--host` is rejected.
- public transport-name namespace is rejected.
- `share session` writes file inbox bundles, reports SSH plans, and starts a
  one-shot HTTP locator.
- `import session` pulls from a fake SSH peer in read-only and apply modes.
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
mmr list sessions --remote mini --project /Users/mish/projects/mmr --limit 3
mmr read project --remote mini --project /Users/mish/projects/mmr --limit 3
mmr context project --remote mini --project /Users/mish/projects/mmr --limit 3
mmr recall --remote mini --project /Users/mish/projects/mmr
mmr import session --from mini --session latest --project /Users/mish/projects/mmr --read-only
mmr share session latest --project /Users/mish/projects/mmr --to mini --dry-run
```

Notes:

- `peer status --host` is a hidden peer-protocol diagnostic and intentionally
  keeps its implementation flag.
- User-facing read/query commands use `--remote`.
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
