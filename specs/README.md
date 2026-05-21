# Specs

`./specs` is the canonical source of truth for product and behavior specifications in this repository.

When implementing or reviewing behavior:

- Check relevant specs before changing code, tests, docs, or ADRs.
- Align implementation and verification with the specs.
- If a spec conflicts with secondary documentation, prefer the spec.
- Update secondary documentation as part of the change when practical.

## Index

- [Messages command](messages.md)

## Related References

- [`docs/references/session-lookup-invariants.md`](../docs/references/session-lookup-invariants.md) — `messages --session` scope rules and stderr hint behavior.
- [`docs/references/schemas/`](../docs/references/schemas/) — source-specific raw transcript layouts and `mmr` field mappings for Claude, Codex, Grok, and Pi.
