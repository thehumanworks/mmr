# Specs

`./specs` is the canonical source of truth for product and behavior specifications in this repository.

When implementing or reviewing behavior:

- Check relevant specs before changing code, tests, docs, or ADRs.
- Align implementation and verification with the specs.
- If a spec conflicts with secondary documentation, prefer the spec.
- Update secondary documentation as part of the change when practical.

## Index

- [Messages command](messages.md)

## Related references

- [Session lookup invariants](../docs/references/session-lookup-invariants.md)
- [Codex message schema](../docs/references/schemas/codex/message_schema.md)
- [Claude message schema](../docs/references/schemas/claude/message_schema.md)
- [Grok message schema](../docs/references/schemas/grok/message_schema.md)
- [Pi message schema](../docs/references/schemas/pi/message_schema.md)
