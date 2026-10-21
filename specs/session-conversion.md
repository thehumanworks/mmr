# Canonical session conversion

`mmr convert session <INPUT> --from codex|claude|canonical --to canonical|codex-readable|claude-readable [--origin <stable-label>]`

This command reads one explicit UTF-8 local file (maximum 16 MiB), prints one JSON response (maximum 16 MiB after serialization), and performs no other reads/writes, provider-root discovery, network requests, tool execution, attachment fetching or memory activation. `--source` and source-default environment settings do not select input; `--from` is explicit. Native Codex/Claude input requires a nonempty stable `--origin` label. Canonical input reuses its archived origin; passing a different label fails.

The response has `command: convert/session`, `target`, `capabilities`, `canonical`, `output_jsonl` and `findings`. `output_jsonl` is null for canonical target. It contains readable target JSONL otherwise. All responses retain the full canonical archive, even when the readable target cannot represent a field. Capabilities distinguish `native_archive: true`, `readable_history: true`, `native_resume: false` and `api_replay: false`. No command installs converted output into a harness.

## Canonical v1

- Explicit schema/parser versions, source and stable origin namespace. Up to 40,000 rows, 100,000 top-level blocks, 64 source-JSON nesting levels and 1,024 bytes per inherited/source identity. Overlarge identity/count inputs fail before materialization; deeper source records become opaque archival data.
- Original UTF-8 native text once in `archive`, with SHA-256. Original whitespace, newlines, CRLF, malformed records and trailing bytes remain recoverable.
- Ordered `events`, each with stable ID, source row sequence, source event/session/branch/parent IDs, original role and normalized role, exact optional timestamp JSON value, and source byte span/hash.
- Ordered `blocks` preserve text, tool-call, tool-result, attachment or opaque kind plus original block JSON. No truncation. Structured tool IDs/names/arguments and attachment locators/media types/declared hashes/availability remain inspectable. Absent timestamps or hashes remain absent; none are invented.
- Tool relationship resolution is scoped to origin/source/session/branch and preserves matched, pending, orphan, unknown-session and ambiguous states with candidate counts rather than a potentially quadratic list of edges without executing or manufacturing tool results.
- Unknown fields remain recoverable in the original archive. Unknown blocks and rows stay opaque. Malformed and truncated rows produce opaque events with findings rather than disappearing. Blank rows remain archival events.
- Deterministic identity uses source namespace, stable source identity (or row hash when absent) and occurrence number. Moving the file or appending rows does not alter existing IDs. Repeated IDs/rows remain separate occurrences, rather than being silently deduplicated.

Canonical reimport accepts either a canonical object or a complete conversion response. It rejects unknown schema/parser versions, archive-hash mismatch, altered spans/events or an altered canonical projection by reparsing the immutable archive and comparing it exactly. This is content-integrity validation, not proof of authentic origin; signed archives are not introduced.

## Readable target projections and findings

Codex-readable output uses familiar `session_meta`/`response_item` shapes and Claude-readable output uses `type`/`message.content` shapes. `_mmr` metadata identifies source events/blocks and references their canonical provenance. Fields that cannot map remain in the canonical archive, with event/block-specific findings. Metadata/unknown/malformed rows use `mmr_opaque` placeholders. The output is a readable interchange view, not a native harness import, resumable session, or provider API request.

Every finding records a stable code, event/block reference where applicable, a field path, disposition (`opaque`, `unavailable`, `mapped`, `archive_only` or `unsupported`) and explanation. Missing attachment hashes/content and unresolved call relationships are explicit. Target exports report archival-only original metadata/opaque values and the unsupported native-resume/API-replay capability. No information is silently dropped from the complete conversion response.

Keep the complete JSON response when transferring history. Extracting only `output_jsonl` deliberately discards the accompanying archival recovery material and findings. Imported roles and text are untrusted data; they do not acquire instruction authority, grant permissions or authorize execution.

## Conformance

Synthetic tests cover parallel/reverse-order/interrupted/orphan/ambiguous calls, repeated IDs across origins/sessions, equal/missing timestamps, opaque extensions, attachments, large output, duplicates, malformed middle and truncated tail rows, move/append identity, canonical reimport, tampering, unknown versions and process restart. Existing native share/import/replay tests remain unchanged.

Explicit null values and absent fields are distinct. Canonical response reimport validates derived target views/capabilities/findings too. Readable JSONL is serialized incrementally into a bounded buffer; compact/pretty envelopes are counted through a bounded writer before final allocation. Encoded attachment source payloads are `embedded_unverified`, never proof that valid decoded bytes are available.
