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

## Explicit trusted Rust adapters

A Rust consumer can register `HistoryAdapter` implementations in an `AdapterRegistry` and call registry-aware parse/convert functions. The adapter returns a `RecordProjection` containing only source identity/mapping fields, ordered blocks and per-record findings. Archives, spans, sequence, event IDs, inherited session/branch context, budgets and tool relationships belong to the common engine. Adapter-provided relationship metadata is rejected; the engine resolves it itself.

A registered descriptor snapshots a stable source ID, adapter version, readable-history capability and declared unsupported-content categories. Source IDs/versions are 1–64 ASCII alphanumeric/period/underscore/hyphen bytes. Duplicate source IDs fail, including duplicate built-ins. This conversion engine cannot honor native-resume or API-replay claims, so registration rejects adapters declaring either. Unsupported categories are exposed in descriptors and generate explicit session findings. A canonical-only adapter cannot produce a readable target projection.

Canonical block kinds are `text`, `tool_call`, `tool_result`, `attachment` and `opaque`. Unknown kinds must map to `opaque`; their original data remains in `source` and the archive. An unsupported-content declaration may name a canonical kind or a provider-specific category (for example `vendor-state`). The adapter retains those categories opaquely, and the engine rejects an interpreted block whose kind matches a declaration. Provider-specific category recognition is the trusted adapter's responsibility; the engine cannot infer source semantics. Attachment availability accepts only `reference_only` or `embedded_unverified`, never a claim of verified bytes. Returned projections are depth/count/byte checked before identity serialization or inherited-context cloning, and cumulative event materialization remains bounded. Mapped identity, record-type and role strings are limited to 1,024 bytes. Arbitrary work inside the callback is outside these limits.

The existing CLI still registers only Codex/Claude and accepts the same `--from` values. No executable/plugin discovery, config scanning or network is added. Custom adapters are available only through the explicit Rust registry API. Trusted adapter code executes with its caller's privileges; registration and projection checks do not sandbox it or bound arbitrary work done inside a callback.

Canonical schema1 uses the existing `parser_version` field as the adapter version. Built-in versions remain `mmr-session-v1`, preserving all current Codex/Claude output bytes and event IDs. Custom source identifiers serialize as strings alongside `codex` and `claude`. Reimport looks up the exact registered source and requires its version to match before reparsing/validating the archive. Missing adapter, version mismatch or inconsistent projection fails closed. Changing mapping or declared capabilities requires an adapter version change.

Rust compatibility: `NativeSource::Codex` and `NativeSource::Claude` constructors and their serialized strings remain unchanged. Supporting `Custom(String)` means exhaustive Rust matches must handle that variant and `NativeSource` no longer implements `Copy`; callers retaining a value after a by-value call must clone it. No strings are leaked or interned to emulate Copy. Previously shipped binaries cannot understand custom source IDs; default/current binaries fail unsupported without explicit registration.

An external integration-test crate must register a synthetic third adapter using only public APIs, exercise raw/null/unknown/attachment/tool conformance, reject missing/mismatched/duplicate registrations and forbidden capability claims, and prove six pre-change built-in response hashes unchanged. All existing conversion bounds and native/store regression tests remain required.
