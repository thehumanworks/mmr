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
- [Teleport command](teleport.md)
- [Teleport user guide](../docs/mmr-teleport.md)
- [Teleport E2E validation](../docs/mmr-teleport-validation.md)
- [Memory Fabric MVP contract](../docs/mmr-memory-fabric-mvp.md)
- [Memory Fabric quickstart and recovery](../docs/mmr-memory-fabric-quickstart.md)
- [Memory Fabric release gate](../docs/mmr-memory-fabric-release-gate.md)
- [Memory Fabric store](../docs/mmr-memory-fabric-store.md)
- [Source adapter framework](../docs/mmr-source-adapters.md)
- [Note command](../docs/mmr-note.md)
- [Redaction before sync](../docs/mmr-redaction.md)
- [Find command](../docs/mmr-search.md)
- [Codex importer](../docs/mmr-codex-importer.md)
- [Claude importer](../docs/mmr-claude-importer.md)
- [Cursor importer](../docs/mmr-cursor-importer.md)
- [Assimilation prompt/runbook handoff](../docs/mmr-dream-runner.md)
