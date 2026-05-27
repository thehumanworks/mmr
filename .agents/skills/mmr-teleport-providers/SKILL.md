---
name: mmr-teleport-providers
description: Native mmr teleport bundles across codex, claude, cursor, grok, and pi. Use when changing pack/apply/resume/export, provider profiles, artifact paths, or teleport contract tests.
---

## Layout

- Registry: `src/teleport/provider.rs` (`profile_for`, `collect_native_files_for_pack`, `native_write_targets`)
- Profiles: `src/teleport/providers/{codex,claude,cursor,grok,pi}.rs`
- Bundle artifact paths: `native/<provider>/…` (legacy `transcript.native.jsonl` still accepted on verify)

## Verification

```bash
cargo test --test cli_contract teleport
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Testing patterns

- Use `TestFixture::seeded()` temp `HOME` with all five provider fixtures.
- Hyphen-encoded `--project` values must use `--project=-Users-…` as a single argv (clap).
- Pi/Grok apply/resume may need `--force` when seeded native files are newer than bundle `last_timestamp`.

## Docs

- Contract: `specs/teleport.md`
- User guide + matrix: `docs/mmr-teleport.md`
