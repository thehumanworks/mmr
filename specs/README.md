# Specs

`./specs` is the canonical source of truth for product and behavior specifications in this repository.

When implementing or reviewing behavior:

- Check relevant specs before changing code, tests, docs, or ADRs.
- Align implementation and verification with the specs.
- If a spec conflicts with secondary documentation, prefer the spec.
- Update secondary documentation as part of the change when practical.

## Index

- [Messages command](messages.md)
- [Remember command](remember.md)

## Related references

- `docs/references/schemas/` documents the raw per-source transcript layouts that `mmr` normalizes into the public API shapes described by these specs.
