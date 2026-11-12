# Specs

`./specs` is the canonical source of truth for product and behavior specifications in this repository.

When implementing or reviewing behavior:

- Check relevant specs before changing code, tests, docs, or ADRs.
- Align implementation and verification with the specs.
- If a spec conflicts with secondary documentation, prefer the spec.
- Update secondary documentation as part of the change when practical.

## Index

- [Command taxonomy](../docs/mmr-command-taxonomy.md)
- [Recall and read session behavior](messages.md)
- [Session sharing](session-sharing.md)
- [Session sharing user guide](../docs/mmr-session-sharing.md)
- [Session sharing validation](../docs/mmr-session-sharing-validation.md)
- [MCP server](../docs/mmr-mcp.md)
- [Nix packaging](../docs/mmr-nix.md)
