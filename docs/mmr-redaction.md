# mmr redaction

Status: implemented for NHL-272
Date: 2026-05-24

`mmr` redacts before any remote sync path can send normalized memory out of the
local store. The MVP baseline is deterministic and local: known secret patterns
block sync, while coarse PII spans are masked for inspection and future sync
payload generation.

## Commands

Scan the linked current project:

```bash
mmr redact scan
```

Explain the latest redaction result for one event:

```bash
mmr redact explain evt:v1:...
```

Preview sync safety without contacting a remote:

```bash
mmr sync --dry-run
```

Full remote sync is still owned by NHL-277. In NHL-272, `sync --dry-run` is a
read-only safety projection: it runs the local redaction policy in memory,
reports which events would be blocked, and never rewrites redaction runs,
redaction spans, or event sync status.

## Policy

The default policy id is `redaction-policy:v1:default`.

Deterministic findings block sync for:

- common API key and token shapes
- private-key blocks
- `.env`-style credential assignments
- high-entropy suspicious tokens

Coarse PII findings are redacted but do not claim complete PII coverage:

- email addresses
- phone numbers
- street-address-like spans

The optional `openai/privacy-filter` layer is represented behind a stable
detector interface, but no model runtime is bundled in the MVP. When that layer
is unavailable, command JSON reports `pii_coverage.status = "degraded"` and
sync safety continues to rely on deterministic blocking plus coarse PII masks.
Under degraded coverage, `sync --dry-run` treats every event as not syncable and
omits payload previews entirely so false negatives cannot become raw dry-run or
future sync payloads by default.
This matches the model-card guidance that privacy filtering is a privacy aid,
not an anonymization guarantee, and should be one layer in a broader policy.

## Storage

Each scan writes:

- a `redaction_runs` row per event and active policy
- `redaction_spans` rows for concrete byte ranges
- event `sync_status = "blocked"` when any blocking finding remains
- event `sync_status = "redacted"` when the event passes policy

The scan path is idempotent for an event and policy: rerunning replaces spans
for the deterministic run id instead of appending duplicate findings.

Raw `events.content_text` and `search_documents.document_text` are local-only
source material. Remote sync code must read redacted projections derived from
redaction spans, not raw search documents. NHL-272 enforces this in the dry-run
surface by omitting payload previews whenever the active policy is degraded or
blocking.

Future sync code must not treat `events.sync_status = "redacted"` as sufficient
permission to upload. It must also evaluate the active policy coverage and block
degraded-policy events unless an explicit, versioned override exists.

## Limits

False-positive allowlists and hard purge are policy decisions, not silent
defaults. The MVP intentionally does not auto-delete local raw evidence and does
not allow a blocked deterministic secret or degraded-policy event to sync. A
later explicit override flow must be scoped to project, policy version, finding
hash, and human-readable reason before it can unblock sync.
