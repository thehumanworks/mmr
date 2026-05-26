# mmr teleport execution plan

Date: 2026-05-26
Goal: ship native Codex session teleport (NHL-321 -> NHL-331) with validated E2E proof.

## Ticket path

| Phase | Linear | Scope | Status |
|-------|--------|-------|--------|
| Spec | NHL-321 TELEPORT-000 | `specs/teleport.md` contract | Done |
| Core | NHL-322 TELEPORT-001 | `pack` / `inspect` / `apply` | Done |
| Discovery | NHL-323 TELEPORT-002 | Latest-session selection for pack | Done |
| Apply | NHL-324 TELEPORT-003 | Remap, `--force`, resume hints | Done |
| Readability | NHL-325 TELEPORT-004 | Post-apply `mmr sessions` / `messages` | Done |
| SSH | NHL-326 TELEPORT-005 | `send --to user@host`, inbox fallback | Done |
| File | NHL-327 TELEPORT-006 | `file://` inbox send/receive | Done |
| HTTP | NHL-328 TELEPORT-007 | `serve` + `receive mmtp://...` | Done |
| Resume/export | NHL-329 TELEPORT-008 | `resume`, `export --as` | Done |
| Docs | NHL-330 TELEPORT-009 | `docs/mmr-teleport.md`, CLI help | Done |
| Validation | NHL-331 TELEPORT-010 | E2E proof, benchmarks, 0.2.0 bump | This ticket |

## NHL-331 deliverables

1. Teleport benchmarks in `tests/cli_benchmark.rs` (opt-in `#[ignore]`).
2. Validation record: `docs/mmr-teleport-validation.md`.
3. Crate version `0.2.0` in `Cargo.toml` / `Cargo.lock`.
4. Spec index link to validation doc.

## Final QA gates

```bash
cargo fmt
cargo test
cargo test --test cli_contract teleport
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## Explicitly out of scope

- NPM publish
- `shared-safe` bundles
- Multi-source native teleport beyond Codex
