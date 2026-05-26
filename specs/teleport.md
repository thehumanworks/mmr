# Teleport Command

Status: accepted for NHL-321 (TELEPORT-000)
Date: 2026-05-26
Linear issue: NHL-321

This document locks the `mmr teleport` CLI contract before implementation.
Downstream tickets should treat it as the source of truth unless a later ADR or
spec explicitly supersedes it.

## Implementation status

**Shipped in NHL-322 (TELEPORT-001):** local native Codex bundle primitives only:

- `mmr teleport pack` — create a deterministic `.mmr` bundle for one selected Codex session
- `mmr teleport inspect` — validate hashes and describe a bundle
- `mmr teleport apply` — verify, cache, and write native Codex session files locally

**Shipped in NHL-324 (TELEPORT-003):** hardened native Codex apply proof:

- `mmr teleport apply` writes native Codex JSONL under `~/.codex/sessions/…`, preserving
  relative layout from bundle metadata when available
- Overwrite protection compares existing transcript timestamps against bundle
  `session.last_timestamp`; newer local transcripts require `--force`
- Path remap applies manifest rules plus `--project`, rewriting native `session_meta.cwd`
- Apply JSON reports documented Codex resume command with
  `resume.status: "visible_but_not_resumable"` when agent resume is best-effort only

**Shipped in NHL-326 (TELEPORT-005):** SSH send transport for native Codex bundles:

- `mmr teleport send --session <id> --to <ssh-host>` packs locally then transfers over ordinary SSH
- When remote `mmr` is on `PATH`, streams bundle bytes into `mmr teleport apply --to -`
- When remote `mmr` is missing, copies `bundle.mmr` into
  `~/.mmr/teleport/inbox/<bundle_id>/` and reports the exact remote `mmr teleport apply --to …`
  command in JSON (`next_command`) with `status: "partial"`
- `--dry-run` packs (or dry-run packs) for bundle metadata and returns planned SSH/SCP argv without
  contacting the remote host
- Send JSON reports `command: "teleport/send"`, `transport: "ssh"`, bundle metadata, and
  `remote_apply` status; failures include `error_kind` values such as `ssh_auth_connect`,
  `ssh_transfer`, and `remote_apply`

**Shipped in NHL-327 (TELEPORT-006):** file destination transport and local receive:

- `mmr teleport send --session <id> --to file:///path/to/inbox` writes a native `bundle.mmr`
  into `<inbox>/<bundle_id>/` using atomic `bundle.mmr.partial` → `bundle.mmr`, then
  `bundle.sha256`, then an empty `ready` marker
- `--transport file` requires a `file://` target; `--transport auto` infers `file` from `file://`
  and `ssh` from `user@host` targets; `--transport http` remains rejected
- `mmr teleport receive` accepts a positional locator or `--to` for `file://` bundle paths,
  inbox bundle directories, or plain local paths; incomplete inbox entries (no `ready`, or
  `bundle.mmr.partial` only) return `status: "ok"` with an empty `staged` list; valid ready
  bundles delegate to `apply_bundle` (idempotent re-receive returns `skipped`)
- Send JSON reports `transport: "file"` with `inbox_path`, `bundle_path`, `ready_path`, and
  related bundle metadata; receive JSON reports `transport: "file"`, `locator`, `staged`, and
  optional nested `apply`

**Shipped in NHL-328 (TELEPORT-007):** one-shot HTTP receive URL:

- `mmr teleport serve --session <id>` packs one native Codex bundle, binds a local HTTP listener,
  prints exactly one startup JSON object (`command: "teleport/serve"`, `transport: "http"`,
  `listen_url`, `token`, bundle metadata, `expires_at`, `bind_addr`, `session`), warns on stderr,
  then blocks until one valid download or `--timeout` (default 600s)
- `mmr teleport receive <mmtp://host:port/token>` (or `http://host:port/token`) downloads the
  bundle over plain HTTP, verifies `X-MMR-Bundle-Sha256`, caches locally, and delegates to apply
- Invalid tokens return HTTP 403 without consuming the bundle; a second receive after a successful
  download fails explicitly once the one-shot server has exited
- Bind resolution uses `--bind` / `--to host:port`, else `MMR_TELEPORT_BIND`, else Tailscale IPv4
  when available, else `127.0.0.1:0`

**Shipped in NHL-329 (TELEPORT-008):** smart resume and `--as` transform convention:

- `mmr teleport resume <ref>` applies a native Codex bundle (unless `--dry-run`) and returns JSON
  with nested `apply` and `agent` resume guidance; default `--as same` resolves to the bundle
  manifest source; unsupported cross-agent `--as` values return `status: "unsupported"` on stdout
  with exit code 3
- `mmr teleport export <ref> --to <path> --as codex` (or `--as same` on a Codex bundle) writes the
  native Codex transcript artifact to `--to`; unsupported cross-agent `--as` values return
  `status: "unsupported"` with exit code 3
- Fidelity/output tokens such as `--as native`, `--as shared-safe`, or `--as json` are usage errors
  on `resume` and `export` (exit 2 structured JSON)
- Top-level `mmr export` remains a history query over local sources; `teleport export` transforms a
  bundle artifact using `--as` and `--to`

NHL-322 constraints:

- **Codex + `native` fidelity only** for pack/apply (no Claude/Cursor/Grok/Pi apply yet)
- **No network/transport** except SSH `send` (TELEPORT-005), local `file://` inbox
  send/receive (TELEPORT-006), and one-shot HTTP `serve`/`receive` (TELEPORT-007); no background
  daemon or shared-safe bundles yet
- **No `shared-safe` bundles**, or store-import hook yet (`resume` and `teleport export` shipped in TELEPORT-008)
- Bundle artifact is a single JSON `.mmr` file (not `bundle.tar.zst`) until a later ticket adds compression/transport wrapping

**Target contract below (NHL-321 and later Linear tickets):** the remainder of this
spec — including `shared-safe` fidelity, `send` / `receive` / `resume`, transport
semantics, multi-provider manifest profiles, tar.zst layout, and store import on
apply — describes intended future behavior. Unless an implementation-status note says
otherwise, treat those sections as design intent, not current CLI behavior.

## Product Thesis

Teleport moves **exactly one selected coding-agent session** between the
user's own machines so work can continue in the same provider on another host.

Teleport is **selected-session handoff**, not:

- `mmr sync` (ongoing, project-scoped, redacted memory-fabric reconciliation)
- `mmr link` (first-run store setup and import)
- host-wide history backfill
- a background daemon or always-on relay

The existing fast local discovery commands remain unchanged and stay the primary
read path:

- `mmr projects`
- `mmr sessions`
- `mmr messages`
- `mmr export`

## Goals

- Move one selected session as an immutable, content-addressed bundle.
- Support Codex restore first; keep manifest and apply hooks extensible for
  Claude and Cursor later.
- Work without a required cloud account, bucket, GitHub remote, or host-wide sync.
- Bundle transport inside mmr (HTTP one-shot, SSH/SCP pipe, `file://` inbox).
- Preserve machine-readable JSON on stdout; human diagnostics on stderr only.

## Non-Goals

- No automatic teleport during `link`, `sync`, or import.
- No multi-project or all-sessions backfill by default.
- No guarantee that every provider can resume an in-flight agent thread on the
  target host (see `resume` semantics below).
- No replacement for Memory Fabric sync projections or learned-memory hydration.
- No required third-party sync product (Syncthing, iCloud, rclone) — only
  optional `file://` adapters when the user already uses them.

## Terminology

- **Selected session**: the single session chosen by `--session`, or the latest
  session in the resolved project scope when `--session` is omitted.
- **Bundle**: immutable compressed artifact (`*.tar.zst`) plus sidecar metadata
  (`manifest.json`, optional `sha256` file) describing one selected session.
- **Native fidelity**: provider on-disk transcript and restore artifacts required
  for agent continuation; may include secrets, tool I/O, and machine-local paths.
- **Shared-safe fidelity**: redacted, citation-safe projection aligned with sync
  and dream evidence rules; not sufficient for agent-native resume.
- **Transport**: how a bundle moves between hosts (`http`, `ssh`, `file`, or a
  local filesystem path).
- **Inbox**: receiver-local staging directory under `~/.mmr/teleport/inbox/`.
- **Apply**: write bundle contents into target-local agent storage and optionally
  re-import into the linked mmr store for the target project.
- **Resume**: UX wrapper that runs `apply` then invokes the provider's
  documented continuation path when known.

## Flag Semantics (Locked)

Each flag has **one domain per subcommand**. Domains never overlap on the same
invocation. Supplying a flag outside its allowed domain is invalid (exit code
`2`, stderr names the flag and subcommand).

### `--to`

`--to` always names the **destination side of movement**. Its meaning depends on
the subcommand:

| Subcommand | `--to` meaning | Example |
|------------|----------------|---------|
| `pack` | Output path for the bundle artifact | `--to ./out/session.tar.zst` |
| `inspect` | Bundle path or inbox directory to read | `--to ~/.mmr/teleport/inbox/abc/` |
| `apply` | Bundle path, inbox directory, or `-` for stdin | `--to ./session.tar.zst` |
| `send` | Destination host, transport target, or local staging path | `--to bob@macbook`, `--to http://100.x.x.x:PORT`, `--to file://~/Sync/mmr-inbox` |
| `receive` | HTTP bind `host:port`, or `file://` inbox root | `--to 100.x.x.x:8765`, `--to file://~/.mmr/teleport/inbox` |
| `resume` | Local bundle/inbox path, or remote destination (send mode) | `--to ./session.tar.zst` or `--to bob@macbook` |

#### Bundle path resolution (`inspect`, `apply`, `resume` local mode)

Bundle input is specified in **exactly one** of these forms:

1. Positional `[bundle-path]`
2. `--to <path>`

Rules:

- **Both positional and `--to` present → invalid** (exit `2`). Do not merge or
  pick a winner; stderr must say only one bundle locator is allowed.
- **Neither present → invalid** (exit `2`) unless `resume`/`send` is initiating
  remote handoff (see subcommand sections).
- Positional and `--to` are equivalent when only one is supplied; implementations
  MUST normalize to a single internal path before IO.

#### `--to` omission defaults

| Subcommand | When `--to` omitted |
|------------|---------------------|
| `pack` | Content-addressed path under `~/.mmr/teleport/bundles/<bundle_id>/bundle.tar.zst`, unless `--to` is set |
| `inspect`, `apply` | Invalid unless positional `[bundle-path]` is present |
| `send` | Invalid unless `--dry-run` (dry-run still requires `--to` for transport preview) |
| `receive` | HTTP: bind `MMR_TELEPORT_BIND:8765` (or detected Tailscale IP). File: requires `--transport file`; uses `MMR_TELEPORT_INBOX` when `--to` omitted |
| `resume` | Invalid in local apply mode; valid in remote send mode only when `--session` scopes the outbound handoff |

#### `--to` vs `--transport`

`--transport` selects **how** bytes move. `--to` selects **where** they go. They
must agree.

| `--transport` | Required `--to` shape | Invalid combination (exit `2`) |
|---------------|----------------------|--------------------------------|
| `auto` (default) | Any; transport inferred from `--to` (see Transport Semantics) | — |
| `ssh` | `user@host` or `user@host:path` (no `http://`, no `file://`) | `--to http://…`, `--to file://…` |
| `http` | Send: `http://host:port/…`. Receive: `host:port` bind address | `--to bob@host`, bare filesystem path on send |
| `file` | `file://…` directory inbox | `--to user@host`, `http://…` |

When `--transport` is explicit and not `auto`, it **wins** over scheme inference
from `--to`. If the pair is incompatible, fail before opening sockets or writing
files.

`--transport` is valid only on `send` and `receive`. On other subcommands it is
**invalid** (exit `2`).

### `--as`

`--as` is **not** a global output-format flag. It applies only on subcommands
where its domain is declared below. For human-readable rendering, use `-O`
(see Output contract).

| Subcommand | `--as` domain | Allowed values | Default |
|------------|---------------|----------------|---------|
| `pack`, `send` | Bundle fidelity / representation | `native`, `shared-safe` | `native` |
| `resume` | Target agent for continuation | `same`, `codex`, `claude`, `cursor`, `grok`, `pi` | `same` |
| `inspect`, `apply`, `receive` | — | **not allowed** | — |

Invalid `--as` handling:

- `inspect --as …`, `apply --as …`, or `receive --as …` → exit `2`; stderr:
  `--as is not valid for teleport <subcommand>; use -O for output format`.
- `pack --as same` (agent-domain value on fidelity command) → exit `2`.
- `resume --as native` (fidelity-domain value on agent command) → exit `2`.
- `resume --as json` or any output-format token → exit `2`; use `-O md` instead.

Fidelity rules (`pack`, `send` only):

- `--as native` includes provider-native artifacts required for agent handoff.
- `--as shared-safe` omits native artifacts; normalized export is required in the
  bundle. `send` with `--as shared-safe` MUST NOT invoke remote agent resume.
- `resume` requires a bundle with `fidelity: "native"` in the manifest. Attempting
`resume` on a `shared-safe` bundle → exit `2`. On `resume`, `--as` selects the
agent CLI only (`same` or a concrete provider).

Agent rules (`resume` only):

- `--as same` uses manifest `source` and fails if that agent CLI is unavailable.
- `--as` never selects HTTP vs SSH; use `--to` with `--transport`.

### `-O` (output format)

Teleport follows the existing mmr pattern (`remember -O md`):

- `-O json` (default): stdout is the command JSON object only.
- `-O md`: stdout is **still** a JSON object; add a top-level string field `text`
  containing a Markdown summary for humans. Scripts MUST ignore `text` when they
  only need structured fields.

`-O` never changes fidelity, transport, or agent selection.

**`pack` archive output** always goes to a filesystem path (`--to` or the default
bundles directory). `pack` does not write raw archive bytes to stdout; stdout
carries JSON only. There is no `--stdout` flag.

Pipe-friendly transfer uses a file path or `apply --to -` on the receiver (see
Transport Semantics).

### Session and project scope

Teleport reuses existing CLI scope rules:

- `--source` accepts only `claude`, `codex`, `cursor`, `grok`, or `pi`.
- Omitting `--source` means all supported sources unless `MMR_DEFAULT_SOURCE`
  supplies a default.
- `--project` scopes session selection; omitting it uses cwd auto-discovery for
  `sessions`/`messages` unless `MMR_AUTO_DISCOVER_PROJECT=0`.
- `--session <id>` selects the handoff session; omitting it selects the latest
  session in scope (same notion as `mmr remember` default).
- `--latest` explicitly selects the latest session in scope; it is equivalent to
  omitting `--session` and MUST NOT be combined with `--session` (exit `2`,
  structured teleport failure JSON).
- Exactly one session per invocation; repeated `--session` values are invalid.

### Happy-path inference (locked)

When the user runs a wrapper without explicit provider flags:

1. Read the selected session from local history (`source`, `session_id`,
   `project_name`, native refs).
2. Infer **source provider** from `ApiSession.source`.
3. Infer **target provider** as the same provider on the destination machine.
4. Build or consume a **`native` fidelity** bundle by default.
5. Resolve target project path via manifest aliases plus receiver `--project`
   override.

Example happy path:

```bash
# Machine A (Codex session in cwd project)
mmr teleport send --session sess-abc --to bob@macbook

# Machine B
mmr teleport resume --to ~/.mmr/teleport/inbox/<bundle_id>/bundle.tar.zst
```

No `--source`, `--as` (on `send`; fidelity defaults to `native`), or `--transport`
flags are required when the session already identifies Codex → Codex handoff.

## Command Surface

### Layering

| Layer | Commands | Role |
|-------|----------|------|
| Primitives | `pack`, `inspect`, `apply` | Create, validate, or install a bundle |
| UX wrappers | `send`, `receive`, `resume` | Orchestrate transport + primitives |

Wrappers MUST delegate to primitives internally so behavior stays testable without
network fixtures.

### Command Matrix

Global flags inherited from mmr unless noted: `--pretty`, `--source`, `--project`,
`-O json|md`.

| Command | Positional args | Primary flags | Mutates local agent state | Mutates mmr store | Opens network |
|---------|-----------------|---------------|---------------------------|-------------------|---------------|
| `teleport pack` | — | `--session`, `--latest`, `--to`, `--as native\|shared-safe`, `--dry-run` | No | No | No |
| `teleport inspect` | `[bundle-path]` | `--to`, `-O json\|md`, `--verbose` | No | No | No |
| `teleport apply` | `[bundle-path]` | `--to`, `--project`, `--dry-run`, `--force`, `--skip-store-import` | Yes | Optional | No |
| `teleport send` | — | `--session`, `--to`, `--as native\|shared-safe`, `--transport auto\|http\|ssh\|file`, `--timeout`, `--dry-run` | No | No | Yes |
| `teleport receive` | — | `--to`, `--transport http\|file` (default `http`), `--timeout`, `--once` | No (stages only) | No | Optional |
| `teleport resume` | `[bundle-path]` | `--to`, `--as same\|…`, `--project`, `--dry-run`, `--no-agent-exec` | Yes | Optional | If sending |

Positional `[bundle-path]` and `--to` on `inspect`, `apply`, and `resume` (local
mode) are **mutually exclusive**; see Bundle path resolution.

### Primitive Semantics

#### `mmr teleport pack`

Creates a bundle for the selected session.

Responsibilities:

- Resolve selected session and native source files.
- Build `manifest.json` with content hashes and required artifact list.
- Include native transcript artifacts for `--as native` (default).
- Include redacted normalized export for `--as shared-safe`.
- Emit JSON status on stdout (see Output contract).

Default output path when `--to` is omitted:

`~/.mmr/teleport/bundles/<bundle_id>/bundle.tar.zst`

#### `mmr teleport inspect`

Validates and describes a bundle without applying it.

Responsibilities:

- Verify manifest schema version, hashes, and required members.
- Report restore readiness, fidelity, parser version, and secret-scan summary.
- Never write agent files or store rows.

#### `mmr teleport apply`

Installs a bundle on the local machine.

Responsibilities:

- Verify bundle integrity (manifest + optional `sha256` sidecar).
- Write native provider files into the provider's session directory when native
  artifacts are present.
- Optionally import normalized events into the linked mmr store for `--project`
  unless `--skip-store-import` is set.
- Apply path remap rules from the manifest before native write.
- Idempotent by default: existing identical content hashes → `status: "skipped"`.
  `--force` replaces native files and re-imports store events.

### Wrapper Semantics

#### `mmr teleport send`

Equivalent to: `pack` → transport → remote or local staging.

Default orchestration when `--to user@host` is set:

1. `pack` to a temp bundle (`--as native` unless overridden).
2. Copy via SSH/SCP to `~/.mmr/teleport/inbox/<bundle_id>/` on the target.
3. Optionally run remote `mmr teleport apply --to <inbox path>` when the remote
   mmr binary is compatible (manifest `min_mmr_version`).

When `--to http://…` or `--transport http`, start a one-shot HTTP GET server that
serves the bundle once or until `--timeout` elapses.

When `--to file://…`, write bundle + `ready` marker atomically into the inbox
directory for folder-sync transports.

#### `mmr teleport receive`

Stages incoming bundles only. **Never applies** them; use `apply` or `resume`
explicitly on the staged path.

Modes ( `--transport` required; no `auto` on `receive`):

- `--transport http` (default when flag omitted): bind `--to host:port` and accept
  one bundle download per invocation (or until `--timeout`). Omitted `--to` uses
  `MMR_TELEPORT_BIND` and port `8765`.
- `--transport file --to file://…`: watch the inbox directory for `ready`
  markers. Omitted `--to` uses `MMR_TELEPORT_INBOX`.
- `--once`: exit after first successful staging.

There is no `--apply`, `--listen`, or hidden auto-apply path in v1.

#### `mmr teleport resume`

User-facing handoff completion.

Equivalent to:

1. `apply` (local bundle path, inbox path, or staged receive result).
2. Unless `--no-agent-exec`, invoke provider continuation when documented:
   - Codex: emit exact recommended CLI command on stderr and optionally execute
     when `--as same` or `--as codex`.
3. Print JSON `{ "apply": …, "agent": … }` summarizing store import and agent
   step outcome.

`resume` MUST NOT claim success for agent thread continuation unless the provider
step exits zero or the manifest marks `agent_resume: "verified"`.

## Bundle Format

### Layout

```text
bundle.tar.zst
├── manifest.json
├── native/
│   └── … provider-specific files …
├── normalized/
│   └── messages.json          # optional; ApiMessagesResponse subset
├── summary/
│   └── continuity.md          # optional; stateless brief
└── hints/
    └── restore.json             # optional; provider resume hints
```

Sidecar files outside the archive:

```text
<bundle_id>/
├── bundle.tar.zst
├── bundle.sha256
└── ready                        # empty marker; atomic rename after write
```

### `manifest.json` (v1)

```json
{
  "mmr_teleport_manifest_version": 1,
  "bundle_id": "tp:v1:…",
  "created_at": "2026-05-26T12:00:00Z",
  "source_host": "alice-mac",
  "mmr_version": "0.1.0",
  "min_mmr_version": "0.1.0",
  "source": "codex",
  "parser_version": "codex-rollout-v1",
  "fidelity": "native",
  "session": {
    "source_session_id": "sess-abc",
    "message_count": 42,
    "first_timestamp": "…",
    "last_timestamp": "…",
    "partial_tail": false
  },
  "project": {
    "canonical_path": "/Users/alice/dev/mmr",
    "aliases": ["-Users-alice-dev-mmr"],
    "path_remap": {
      "/Users/alice/dev/mmr": "${TARGET_PROJECT}"
    }
  },
  "artifacts": [
    {
      "path": "native/rollout.jsonl",
      "required": true,
      "sha256": "…",
      "kind": "native_transcript"
    }
  ],
  "capabilities": ["codex-native-apply", "store-import"],
  "restore": {
    "agent_resume": "best_effort",
    "documented_command": "codex exec resume <id>"
  }
}
```

Rules:

- `bundle_id` and per-artifact hashes are stable across re-packs of identical
  content.
- Required artifacts depend on `--as` / `fidelity`:
  - `native`: at least one `native_transcript` artifact is required.
  - `shared-safe`: native artifacts are omitted; normalized export is required.
- `partial_tail: true` warns receivers not to assume session completeness.
- Future Claude/Cursor support extends `source`, `artifacts[].kind`, and
  `capabilities[]` without bumping envelope version when possible.

## Fidelity Model

### Native (default for handoff)

Use for agent continuation on a trusted personal machine.

Includes:

- Provider-native session files (for Codex: rollout JSONL and companion metadata)
- Raw tool inputs/outputs and local paths present in the transcript
- Restore hints and documented agent CLI commands

Excludes by default:

- Unrelated sessions from the same project
- Learned memory, dream candidates, and remote sync manifests
- Full mmr SQLite database files

Security: native bundles bypass sync redaction. Commands MUST run a pre-flight
secret scan and include findings in JSON (see Security Model).

### Shared-safe

Use when moving a session excerpt between machines or people where sync-grade
redaction is required.

Includes:

- Redacted normalized messages (same deterministic rules as sync/dream
  shared-safe evidence)
- Manifest metadata and citation-safe summaries
- No blocking-grade secret patterns in payload (scan MUST fail pack/send if
  any remain)

Excludes:

- Native provider files required for agent resume
- Tool payloads blocked by `safe_projection_blocker`

`resume` on a `shared-safe` bundle is invalid (exit `2`). Use `apply
--skip-store-import` only when importing normalized preview material.

### Choosing fidelity

| Intent | `--as` on pack/send | `resume` |
|--------|---------------------|----------|
| Continue Codex on laptop | `native` (default) | `resume --as same` |
| Share redacted transcript | `shared-safe` | not supported (manifest fidelity) |
| Inspect before sending | either fidelity | `inspect` first |

## Transport Semantics

Core artifact is always transport-agnostic: **`pack` → bundle file → `apply`**.

### `--transport`

| Value | Behavior |
|-------|----------|
| `auto` (default) | Infer from `--to`: `user@host` → `ssh`; `http(s)://…` → `http`; `file://…` → `file`; local filesystem path on `send` → write-only staging (no network) | — |
| `ssh` | SCP/SSH stream to remote inbox; optional remote `apply` | Must pair with SSH-shaped `--to` |
| `http` | One-shot sender server, or `receive --transport http --to host:port` | Must pair with HTTP-shaped `--to` |
| `file` | Atomic write + `ready` marker into directory inbox | Must pair with `file://` `--to` |

Environment overrides:

- `MMR_TELEPORT_TRANSPORT=ssh|http|file|auto`
- `MMR_TELEPORT_INBOX=~/.mmr/teleport/inbox`
- `MMR_TELEPORT_BIND=<ip>` default listen bind address

### HTTP one-shot

Sender:

- Generates single-use token (32+ bytes, constant-time compare).
- Binds to `MMR_TELEPORT_BIND` or Tailscale IPv4; never `0.0.0.0` unless
  `--allow-insecure-bind` (discouraged, stderr warning).
- Serves `GET /v1/bundle/<bundle_id>` with `Authorization: Bearer <token>` or
  `?token=` query param (query param logs a stderr warning).
- Exits after one successful download or `--timeout` (default 10m).

Receiver:

- `mmr teleport receive --transport http --to 100.x.x.x:8765` binds and waits
  for a sender pull (or automation pushing to that endpoint per implementation).
- Pairing with `send --transport http --to http://…` is documented out-of-band;
  JSON includes URLs and tokens for scripts.

Limits:

- No mid-transfer resume in v1; large bundles rely on compressed tar.zst over a
  stable connection.
- Retries receive the same bytes only while the sender is still listening.

### SSH / SCP

- Copy to `~/.mmr/teleport/inbox/<bundle_id>/` using atomic `*.partial` → rename.
- Remote `apply` requires compatible `mmr` on `PATH` and matching
  `min_mmr_version`.
- Pipe-friendly form MUST be supported (archive bytes on stdin; JSON on stdout
  after apply):

```bash
cat bundle.tar.zst | mmr teleport apply --to -
```

### `file://` inbox

For Syncthing, iCloud Drive, rclone-mounted folders, or USB copy.

- Writer creates `bundle.tar.zst.partial`, then `bundle.sha256`, then empty
  `ready` file.
- Reader ignores directories without `ready`.
- Duplicate `bundle_id` in inbox → idempotent `apply` status `skipped` unless
  `--force`.

## Output contract

**All teleport subcommands emit exactly one JSON object on stdout** on success,
skip, or command-level failure (when argv parsed successfully). This matches the
repo-wide CLI rule: machine-readable JSON on stdout; diagnostics on stderr.

Rules:

- `-O json` (default): stdout is only the JSON object documented below.
- `-O md`: stdout is still the same JSON object, plus a required string field
  `text` containing a Markdown rendering for humans (same pattern as
  `remember -O md`). Parsers MUST NOT assume stdout is raw Markdown.
- `--pretty` indents JSON; it does not change fields.
- Progress, warnings, secret advisories, and agent manual steps go to **stderr**
  only and MUST NOT appear before or after the JSON payload on stdout.

Fatal clap/usage errors before subcommand dispatch MAY leave stdout empty; stderr
carries the error.

## JSON response shapes

Common fields:

```json
{
  "command": "teleport/pack",
  "status": "ok",
  "bundle_id": "tp:v1:…",
  "session": {
    "source": "codex",
    "source_session_id": "sess-abc",
    "project_name": "/Users/alice/dev/mmr"
  },
  "fidelity": "native",
  "transport": null
}
```

`status` enum: `ok`, `skipped`, `partial`, `failed`.

### `teleport/pack`

```json
{
  "command": "teleport/pack",
  "status": "ok",
  "bundle_id": "tp:v1:…",
  "bundle_path": "/Users/alice/.mmr/teleport/bundles/tp:v1:…/bundle.tar.zst",
  "bytes": 1048576,
  "sha256": "…",
  "fidelity": "native",
  "session": { "source": "codex", "source_session_id": "sess-abc" },
  "artifacts": [{ "path": "native/rollout.jsonl", "sha256": "…", "required": true }],
  "scan": {
    "blocking_findings": 0,
    "redacted_findings": 2,
    "pii_coverage": "degraded"
  },
  "dry_run": false
}
```

### `teleport/inspect`

```json
{
  "command": "teleport/inspect",
  "status": "ok",
  "bundle_id": "tp:v1:…",
  "manifest_version": 1,
  "fidelity": "native",
  "restore_ready": true,
  "apply_ready": true,
  "resume_ready": "best_effort",
  "warnings": [],
  "artifacts": [],
  "scan": { "blocking_findings": 0, "redacted_findings": 0 }
}
```

With `-O md`, the same object includes `"text": "# Teleport inspect\n…"`.

### `teleport/apply`

```json
{
  "command": "teleport/apply",
  "status": "ok",
  "bundle_id": "tp:v1:…",
  "target_project": "/Users/bob/dev/mmr",
  "native": {
    "written": true,
    "paths": ["~/.codex/sessions/…"]
  },
  "store": {
    "imported_events": 42,
    "skipped_events": 0
  },
  "path_remap_applied": true,
  "resume": {
    "provider": "codex",
    "documented_command": "codex exec resume sess-abc",
    "agent_resume": "best_effort",
    "status": "visible_but_not_resumable"
  },
  "dry_run": false
}
```

### `teleport/send`

```json
{
  "command": "teleport/send",
  "status": "ok",
  "bundle_id": "tp:v1:…",
  "transport": "ssh",
  "to": "bob@macbook",
  "remote_inbox": "/Users/bob/.mmr/teleport/inbox/tp:v1:…/",
  "remote_apply": {
    "attempted": true,
    "status": "ok"
  },
  "fidelity": "native",
  "sensitivity": "full_native"
}
```

HTTP send adds:

```json
{
  "listen_url": "http://100.x.x.x:8765/v1/bundle/tp:v1:…",
  "token": "…",
  "expires_at": "…"
}
```

Token MUST be treated as secret; scripts should read it from JSON, not logs.

### `teleport/receive`

```json
{
  "command": "teleport/receive",
  "status": "ok",
  "transport": "http",
  "staged": [
    {
      "bundle_id": "tp:v1:…",
      "inbox_path": "/Users/bob/.mmr/teleport/inbox/tp:v1:…/bundle.tar.zst",
      "sha256": "…"
    }
  ]
}
```

### `teleport/resume`

```json
{
  "command": "teleport/resume",
  "status": "ok",
  "apply": {
    "status": "ok",
    "bundle_id": "tp:v1:…"
  },
  "agent": {
    "provider": "codex",
    "requested_as": "same",
    "executed": true,
    "exit_code": 0,
    "command": "codex exec resume …"
  }
}
```

When agent execution is skipped or unavailable:

```json
{
  "agent": {
    "provider": "codex",
    "executed": false,
    "manual_steps": ["…"]
  }
}
```

## Failure Modes

| Condition | Exit code | stdout `status` | stderr guidance |
|-----------|-----------|-----------------|-----------------|
| Positional `[bundle-path]` and `--to` both set | 2 | `failed` | Only one bundle locator allowed |
| `--transport` on `pack`, `inspect`, `apply`, or `resume` (local) | 2 | `failed` | `--transport` valid only on send/receive |
| `--transport` conflicts with `--to` shape | 2 | `failed` | Show valid pairs from transport table |
| `--as` on `inspect`, `apply`, or `receive` | 2 | `failed` | Use `-O` for output format |
| `--as` domain mismatch (e.g. `pack --as same`) | 2 | `failed` | List allowed values for subcommand |
| `-O md` with invalid value | 2 | (empty) | clap usage error |
| Session not found in scope | 2 | `failed` | Suggest `mmr sessions --project …` |
| Multiple sessions matched | 2 | `failed` | Require explicit `--session` |
| Bundle hash mismatch | 3 | `failed` | Do not apply; re-send |
| Manifest version unsupported | 3 | `failed` | Upgrade mmr on receiver |
| Missing required native artifact | 3 | `failed` | Re-pack with `--as native` |
| `shared-safe` scan finds blocking secret | 4 | `failed` | Fix transcript or use `--as native` on trusted path only |
| Target project unresolved | 2 | `failed` | Pass `--project` or link project |
| Duplicate session, no `--force` | 0 | `skipped` | Explain idempotent skip |
| Remote mmr missing or too old | 3 | `partial` | Stage only; run local `apply` |
| HTTP timeout before download | 3 | `failed` | Re-run `send` or use `ssh` |
| Inbox partial (no `ready`) | 0 | `ok` with empty staged | Wait for writer |
| Agent resume CLI failed | 5 | `partial` | `apply` succeeded; manual steps printed |
| `resume` on `shared-safe` bundle | 2 | `failed` | Native fidelity required for agent handoff |

Progress and warnings MUST NOT corrupt stdout JSON; use stderr.

## Security Model

Threat model: moving bundles between **the user's own machines**. Not designed
for anonymous internet endpoints or multi-tenant tailnets without extra controls.

### Data sensitivity

| Fidelity | Secret exposure | Intended use |
|----------|-----------------|--------------|
| `native` | Full transcript secrets possible | Personal device handoff |
| `shared-safe` | Deterministic redaction; blocking patterns fail closed | Safer excerpt transfer |

Commands that pack or send `native` bundles MUST:

1. Run deterministic secret scan (reuse redaction detector).
2. Include `scan` summary in JSON.
3. Print a stderr warning: `teleport: native bundle may contain secrets`.

`native` bundles MUST NOT be uploaded to Memory Fabric sync remotes by teleport
commands.

### Transport security

| Transport | Confidentiality | Integrity | AuthN |
|-----------|-----------------|-----------|-------|
| SSH | SSH transport encryption | hash verified on apply | SSH host/user keys |
| HTTP | Tailscale mesh or TLS when configured | manifest + sha256 | single-use bearer token |
| `file://` | depends on folder sync product | sha256 + ready marker | physical/sync ACLs |

HTTP defaults:

- Prefer Tailscale interface bind address.
- Single-use token, max `--timeout`.
- Constant-time token compare.
- Optional `--encrypt age:<recipient>` in a later ticket; v1 documents gap.

At-rest:

- Inbox directories may contain native secrets; warn when under cloud-synced
  folders.

### Integrity and idempotency

- Apply verifies artifact hashes against manifest before write.
- Re-applying the same `bundle_id` with identical hashes → `skipped`.
- `--force` required to overwrite existing native files or re-import store events.

### Separation from sync

| Concern | `mmr sync` | `mmr teleport` |
|---------|------------|----------------|
| Scope | linked project events | one selected session |
| Payload | redacted replay records | native or shared-safe bundle |
| Cadence | repeatable reconciliation | one-shot handoff |
| Remote | GitHub/file memory store | user transport only |
| Agent resume | not a goal | primary goal (`native`) |

Teleport MUST NOT call sync export paths for native payloads.

## Relationship to Existing Commands

| Existing command | Interaction |
|------------------|-------------|
| `projects`, `sessions`, `messages`, `export` | Unchanged; used to discover `--session`. Top-level `mmr export` queries local history; `teleport export` transforms a bundle artifact with `--as` and `--to` |
| `remember` / `summary` | Optional bundle input via `summary/continuity.md`; not required |
| `link` | Receiver SHOULD link target project before store import |
| `import` / capture adapters | `apply` SHOULD reuse `SourceAdapter::import_session` |
| `sync` | Independent; no auto-trigger |
| `redact scan` | Teleport MAY reuse detector; does not mutate store |

Post-apply store visibility:

- After native apply, run capture import (default) so `mmr messages --session …`
  reflects the teleported session on the target host.
- `--skip-store-import` leaves only provider-native files updated.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `MMR_TELEPORT_TRANSPORT` | Default `--transport` (`auto`) |
| `MMR_TELEPORT_INBOX` | Default inbox directory |
| `MMR_TELEPORT_BIND` | Default HTTP bind address |
| `MMR_AUTO_DISCOVER_PROJECT` | Same semantics as read commands |
| `MMR_DEFAULT_SOURCE` | Same semantics as read commands |

## Verification Expectations (Future Implementation)

This spec ticket does not require code changes. Implementation tickets MUST add:

- Fixture bundles under `tests/fixtures/teleport/`
- Round-trip `pack → inspect → apply` tests without network
- Contract tests for JSON shapes in this document
- Codex-first proof: native apply → provider lists session → documented resume
  path attempted in integration test or manual gate

## Implementation Sequencing (Informative)

1. Bundle manifest + `pack` / `inspect` / `apply`
2. Codex native apply adapter + path remap
3. Store import hook via existing capture adapters
4. `file://` inbox adapter
5. `send` / `receive` over SSH
6. HTTP one-shot transport
7. `resume` agent exec for Codex
8. Claude/Cursor manifest profiles

Transport work MUST NOT precede bundle round-trip correctness.

## Operator workflows (TELEPORT-009)

User-facing copy-paste examples live in [docs/mmr-teleport.md](../docs/mmr-teleport.md).
The subsections below lock workflow intent to **current shipped behavior** (NHL-322
through NHL-329).

### Workflow 1: Same Tailnet / LAN one-shot HTTP

```bash
# Machine A
mmr teleport serve --session sess-abc --project /path/to/project

# Machine B (use listen_url from serve stdout JSON)
mmr teleport receive 'mmtp://100.x.x.x:PORT/TOKEN' --project /path/to/project
```

`serve` prints one startup JSON object then blocks until download or timeout.
Invalid tokens return HTTP 403 without consuming the bundle. A second receive
after success fails once the one-shot server exits.

### Workflow 2: SSH machine-to-machine

```bash
mmr teleport send --session sess-abc --project /path/to/project --to user@host
```

Remote `mmr` on `PATH` → stream apply. Missing remote `mmr` → stage under
`~/.mmr/teleport/inbox/<bundle_id>/` with `status: "partial"` and
`next_command` for manual `mmr teleport apply --to …`.

### Workflow 3: Shared folder / sync-backed inbox

```bash
mmr teleport send --session sess-abc --to 'file:///path/to/synced/inbox'
mmr teleport receive --to '/path/to/synced/inbox/tp:v1:…'
```

Atomic layout: `bundle.mmr.partial` → `bundle.mmr`, `bundle.sha256`, empty
`ready`. Incomplete entries return `ok` with empty `staged`.

### Workflow 4: Local pack / apply

```bash
mmr teleport pack --session sess-abc --to ./handoff.mmr
mmr teleport inspect ./handoff.mmr
mmr teleport apply --to ./handoff.mmr --project /path/to/project
```

Default pack path: `~/.mmr/teleport/bundles/<bundle_id>/bundle.mmr`.

### Workflow 5: Resume and export

```bash
mmr teleport resume ./handoff.mmr --project /path/to/project
mmr teleport export ./handoff.mmr --to ./out.jsonl --as codex
```

Cross-agent `--as` on `resume` / `export` → stdout `status: "unsupported"`, exit
**3**. Fidelity tokens (`native`, `shared-safe`, `json`) on those subcommands →
usage error, exit **2**.

## Safety (shipped behavior)

- **Handoff, not sync:** one selected session; independent of `mmr sync`.
- **Native Codex only** for pack/apply/send/serve/receive in this release.
- **Native bundles** may contain secrets, tool output, private paths, and raw
  transcript content; stderr warns on pack/send/serve.
- **`shared-safe` is not implemented** for pack, send, or native resume.
- **Unsupported transforms** (`resume --as claude`, `export --as claude`, etc.)
  return structured JSON with `status: "unsupported"` and exit code **3**.

## Troubleshooting (shipped behavior)

| Symptom | Likely cause | Action |
|---------|--------------|--------|
| Apply fails on project paths | Source `cwd` differs from target | `--project /target/path` on apply/receive/resume |
| Apply fails without `--force` | Local transcript newer than bundle | `--force` or skip if local copy is authoritative |
| SSH send `partial` | Remote `mmr` missing | Run `next_command` from JSON or manual `apply` |
| Inspect/apply/receive exit 3 | Corrupt bundle or hash mismatch | Re-pack and re-transfer; verify `bundle.sha256` |
| HTTP 403 / receive fails | Wrong or expired token | Re-run `serve`; use fresh `listen_url` while listening |
| Receive `ok`, empty `staged` | Inbox incomplete or unsynced | Wait for `ready` marker and folder sync |
| `resume --as native` exit 2 | Fidelity token on agent command | Use `--as same` or `--as codex`; fidelity is pack-time only |

## Fresh-session verification (NHL-321 – NHL-331)

Agents picking up teleport work should read implementation-status at the top of
this spec, then run:

```bash
cargo test --test cli_contract teleport
```

Ticket map: NHL-321 (spec) → NHL-322 (pack/inspect/apply) → NHL-324 (apply
hardening) → NHL-326 (SSH send) → NHL-327 (file inbox) → NHL-328 (HTTP serve) →
NHL-329 (resume/export) → NHL-330 (docs/help) → NHL-331 (follow-on). Confirm
each shipped note matches CLI behavior before extending transport or fidelity.
