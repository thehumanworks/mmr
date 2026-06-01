---
name: mmr-native-bundle-providers
description: Native mmr session bundle profiles across codex, claude, cursor, grok, and pi. Use when changing share/import provider profiles, artifact paths, or native bundle contract tests.
---

## Layout

- Registry: `src/teleport/provider.rs` (`profile_for`, `collect_native_files_for_pack`, `native_write_targets`)
- Profiles: `src/teleport/providers/{codex,claude,cursor,grok,pi}.rs`
- Bundle artifact paths: `native/<provider>/...`

## Public Command Surface

- Source-side handoff: `mmr share session ...`
- Destination-side handoff: `mmr import session ...` and `mmr import bundle ...`
- Provider event ingestion: `mmr --source <source> ingest events ...`
- Peer reads: `--remote <ssh-target>` on read/query commands.

Do not add compatibility aliases for removed command names or flags unless the
maintainer explicitly asks for them.

## Verification

```bash
cargo test --test cli_contract share -- --nocapture
cargo test --test cli_contract import -- --nocapture
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Testing Patterns

- Use `TestFixture::seeded()` temp `HOME` with all five provider fixtures.
- Hyphen-encoded `--project` values must use `--project=-Users-...` as a single argv when clap requires it.
- Pi/Grok apply paths may need `--force` when seeded native files are newer than bundle `last_timestamp`.

## Docs

- Contract: `specs/session-sharing.md`
- User guide + matrix: `docs/mmr-session-sharing.md`
