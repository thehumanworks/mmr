---
gdd_version: "1.0"
goal_id: "remote-share-import-command-surface"
title: "Remote share/import command surface"
status: "done"
confidence_floor: 90
created: "2026-06-01"
updated: "2026-06-01"
---

# Goal: Replace transport-first peer/session movement commands with a simple remote/read, share, import, and ingest command surface.

> Single source of truth for this goal. Loaded into a **fresh agent context** each
> session. The agent resumes solely from the state recorded in this file — if it
> isn't written here, it didn't happen.

---

## 1. Operating Protocol · read first, every session

This section is the rulebook. It is invariant and does not change between sessions.

**Boot sequence (every fresh context):**
1. Read this document top to bottom. If `gdd_status.py` is available, run it to
   compute the resume point and surface any invariant violations.
2. Evaluate **Exit Conditions** (§4). If any fires, stop and report it explicitly
   to the user — do not start work.
3. The next action is the first unchecked **Task** (§5) whose dependencies are all ticked.
4. Work the task. Satisfy its **Verification Contract** *before* ticking it.
5. Append to the live sections (§6–8) as you go. Re-evaluate Exit Conditions
   after every task. Bump frontmatter `updated`.

**Invariants — NEVER edit these (they define scope):**
- The set and wording of **Definition of Done** items (§3).
- The set and wording of **Tasks** and their **Verification Contracts** (§5).
- You MAY tick checkboxes and update **Confidence Scores**. You MAY NOT add,
  remove, reword, split, or merge any DoD item or Task.
- **Scope is fixed.** Reducing or increasing scope autonomously is not accepted.
  If scope must change, record the proposal in **Decisions** (§6) and fire the
  `SCOPE-CHANGE` exit condition (§4) — stop and surface it to the user.

**Confidence floor (= `confidence_floor`, currently 90):**
- A Task may be ticked done only when its Confidence Score ≥ floor.
- Below floor = "unsure this works / not all BDD scenarios + tests covered."
- If you cannot reach the floor: keep working (add tests, cover missing BDD
  scenarios). If you still cannot, do not fake the number — leave it unchecked
  and fire `CONFIDENCE-STALL`.

**Whole-goal done:** every DoD item ticked AND every Task ticked with Confidence ≥
floor. This is the primary Exit Condition.

**Live sections** (§6 Decisions, §7 Learnings, §8 Skills) are append-only working
memory — update continuously, never delete prior entries.

---

## 2. References

Everything the agent needs before/while working. Read first.

- User decision: no backwards compatibility unless explicitly requested.
- Prior live proof: `goals/2026-06-01-live-mac-mini-peer-validation.md` — Mac
  Mini peer access works over SSH/Tailscale when remote `mmr` is up to date.
- Current peer goal: `goals/2026-06-01-explicit-ssh-peer-context.md` — existing
  peer protocol and `--host`/`teleport pull` MVP.
- CLI entry point: `src/cli.rs` — public clap command surface and routing.
- Peer transport: `src/peer.rs` — SSH target parsing, fixed argv, JSON peer RPC.
- Teleport implementation: `src/teleport/` — native bundle pack/read/apply/send
  mechanics to be re-exposed as `share`/`import`.
- API types: `src/types/api.rs` — message origin and peer result output.
- Query service: `src/messages/service.rs` — local list/read/context/recall data
  source for remote parity.
- Contract tests: `tests/cli_contract.rs` — user-facing CLI behavior.
- Docs to update: `docs/mmr-command-taxonomy.md`, `docs/mmr-teleport.md`,
  `specs/teleport.md`, `README.md`, `.agents/skills/mmr/SKILL.md`,
  `.agents/skills/mmr-teleport-providers/SKILL.md`.

---

## 3. Definition of Done · INVARIANT

Only **measurable, verifiable** entries — each tickable by observation or command.

- [x] `--remote <ssh-target>` is the only public peer-read flag, available on the
      scoped read/query commands selected in this goal; `--host` is removed from
      the public CLI. — *verify by:* `cargo run -- --help`, command-specific
      `--help`, and negative clap tests proving `--host` is rejected.
- [x] Read-only remote commands feel like local commands with an added location:
      local-only behavior remains unchanged when `--remote` is absent, and
      local-plus-remote merged results include remote origin metadata when
      `--remote` is present. — *verify by:* fake SSH contract tests and live
      `mini` smoke tests for `list`, `read`, `context`, `recall`, and
      `summarize` surfaces covered by §5.
- [x] The public `teleport` namespace is removed and replaced by literal
      direction-oriented commands: `mmr share ...` for source-side sharing and
      `mmr import ...` for destination-side session/bundle import. — *verify by:*
      help snapshots/contract tests showing `share` and session/bundle `import`
      exist and `teleport` is rejected.
- [x] The old top-level normalized-event `mmr import` behavior is renamed to
      `mmr ingest events`; `import` means session/bundle material only. — *verify
      by:* contract tests for `ingest events`, contract tests for session/bundle
      `import`, and negative tests proving old event-import argv no longer
      parses under top-level `import`.
- [x] No backwards-compatibility aliases, deprecated shims, hidden public
      fallbacks, or old command spellings are retained unless the user explicitly
      changes scope. — *verify by:* `rg` over CLI/docs/tests for removed public
      names and contract tests for rejected legacy invocations.
- [x] User docs, command taxonomy, specs, and bundled skills describe the new
      command model and remove `teleport`, `--host`, and old event-import
      guidance. — *verify by:* `rg` checks plus manual review of updated docs.
- [x] Full local QA passes. — *verify by:* `cargo fmt`, `cargo test`,
      `cargo clippy --all-targets --all-features -- -D warnings`, and
      `cargo build --release`.
- [x] Live SSH/Tailscale validation against Mac Mini passes with the new surface.
      — *verify by:* running peer status, representative remote read/query
      commands, and a read-only session/bundle import from `mini`.
- [x] Completed implementation is committed and pushed to the configured GitHub
      remote when verification is clean and unrelated worktree changes are not
      staged. — *verify by:* `git status --short`, `git log -1 --oneline`, and
      `git push origin main`.

---

## 4. Exit Conditions

The goal terminates when **any** condition holds. On exit, state which condition
fired — explicitly — in the response to the user.

- **`DONE`** — all §3 items ticked and all §5 tasks ≥ confidence floor. *(primary)*
- **`BLOCKED-DEP`** — SSH access to the Mac Mini, GitHub push access, or a
  required local build/test dependency is unavailable after one direct retry.
  Exit without the blocked step; name it explicitly.
- **`SCOPE-CHANGE`** — work cannot complete without preserving any old public
  command/flag spelling, adding a registry/daemon/discovery system, changing
  native provider bundle semantics, or dropping one of the command families in
  §3. Do not change scope. Record the proposal in §6 and exit to the user.
- **`CONFIDENCE-STALL`** — any task remains below confidence floor after three
  honest implementation/verification attempts. Exit, report the task and the gap.
- **`BUDGET`** — two focused implementation sessions or 6 wall-clock hours are
  exhausted before DONE. Exit and report progress.

---

## 5. Tasks · INVARIANT

Ordered, dependency-aware units of work that together satisfy the DoD.

**Confidence rubric** *(0–100, floor = 90)*
- `95–100` verified by passing tests + all BDD scenarios covered + manually confirmed.
- `90–94` verified by tests, BDD scenarios covered, minor edge uncertainty. *(done-eligible)*
- `70–89` plausible but tests incomplete or BDD scenarios not all covered. **Keep working.**
- `<70` unsure it works. **Not done-eligible.** Consider `CONFIDENCE-STALL`.

Task status is the `[ ]`/`[x]` at the end of each task heading. Tick it only when
the Verification Contract passes and Confidence ≥ floor.

---

### T1 · Finalize command contract and remove compatibility assumptions · [x]

**Steps**
- [x] Inventory current `list`, `read`, `context`, `recall`, `summarize`,
      `import`, and `teleport` command shapes in `src/cli.rs` and docs.
- [x] Write or update contract tests that describe the target CLI surface before
      implementation where practical.
- [x] Decide exact selectors and defaults for `share session`, `import session`,
      `import bundle`, and `ingest events` within this fixed scope.

**Verification Contract**
- *Check:* A fresh agent can read tests/docs and know the target CLI contract
  before implementation details.
- *Method:* `cargo test <targeted contract tests>` where added, plus manual
  inspection of the task notes appended to §6 if decisions were needed.
- *Expected:* New contract tests fail only because implementation is not wired
  yet, not because the command contract is ambiguous.
- *BDD scenarios covered:* no compatibility aliases; `--remote` is the sole peer
  flag; `teleport` is not public; event import is renamed to `ingest events`.

**Confidence:** 96 / 90 · **Depends on:** none

---

### T2 · Implement `--remote` as the unified read/query location modifier · [x]

**Steps**
- [x] Replace public `--host` args with `--remote` args on scoped read/query
      commands.
- [x] Extend hidden peer protocol as needed so `list projects`, `list sessions`,
      `read project`, `read session`, `read source`, `context project`,
      `context source`, `recall`, and `summarize project/session/source` can use
      explicit SSH peers where the command semantics require it.
- [x] Preserve local-only output shape when `--remote` is absent.
- [x] Preserve strict failure semantics when a named remote fails.

**Verification Contract**
- *Check:* Remote read/query behavior matches local command shape with remote
  origin metadata and no host registry/discovery.
- *Method:* fake SSH tests in `tests/cli_contract.rs`, parser/argv unit tests in
  `src/peer.rs`, and `cargo test remote` or equivalent targeted test filters.
- *Expected:* Local-only tests remain unchanged; remote tests show merged results,
  `origin.host`, `origin.transport: ssh`, remote mmr version, and structured
  peer failures.
- *BDD scenarios covered:* local only; local plus one remote; local plus multiple
  remotes; stale remote `mmr`; SSH failure; invalid target; pagination
  `next_command` uses `--remote`; `--host` is rejected.

**Confidence:** 95 / 90 · **Depends on:** T1

---

### T3 · Replace `teleport` with `share` for source-side session sharing · [x]

**Steps**
- [x] Add public `mmr share session <selector>` command.
- [x] Map source-side SSH, file inbox, and one-shot HTTP behavior from existing
      teleport send/serve code into `share`.
- [x] Remove public `mmr teleport send` and `mmr teleport serve` command paths.
- [x] Keep native bundle internals private/reusable without preserving the old
      public namespace.

**Verification Contract**
- *Check:* A user on the source machine can share a selected session without
  seeing or invoking `teleport`.
- *Method:* contract tests for SSH dry-run, file inbox write, HTTP one-shot
  serving, and help output; negative test for `mmr teleport`.
- *Expected:* `mmr share session latest --to <target>` and
  `mmr share session latest --via http` work; `mmr teleport ...` is rejected.
- *BDD scenarios covered:* share latest; share explicit session; file inbox;
  SSH remote with and without remote mmr; HTTP locator; invalid selector;
  removed `teleport` namespace.

**Confidence:** 95 / 90 · **Depends on:** T1

---

### T4 · Replace destination-side session movement with `import` and rename event import to `ingest` · [x]

**Steps**
- [x] Add `mmr import session --from <remote> --session <selector> --project <project>`
      for SSH pull of a native bundle from a peer.
- [x] Add `mmr import bundle <locator>` for local bundle path, inbox entry, or
      one-shot HTTP locator.
- [x] Support explicit `--read-only`, `--apply`, and `--force` behavior without
      keeping old command spellings.
- [x] Move normalized-event import behavior to `mmr ingest events`.
- [x] Remove old top-level event-import meaning from `mmr import`.

**Verification Contract**
- *Check:* Destination-side import is literal and direction-oriented, and event
  ingestion is no longer conflated with session import.
- *Method:* fake SSH bundle tests, local bundle read/apply tests, inbox/HTTP
  receive tests, event-ingest tests, and negative tests for old event-import argv.
- *Expected:* `import session` can pull/read/apply a native bundle, `import bundle`
  can read/apply existing locators, `ingest events` imports normalized events,
  and old `teleport pull/read/apply/receive/resume/export/pack/inspect` public
  invocations fail.
- *BDD scenarios covered:* read-only remote import; applied remote import;
  read-only bundle import; applied bundle import; forced apply; event ingest;
  corrupt locator; stale remote; removed teleport subcommands.

**Confidence:** 95 / 90 · **Depends on:** T1, T3

---

### T5 · Update docs, specs, skills, and command help to the clean surface · [x]

**Steps**
- [x] Update command taxonomy, teleport/session movement docs, specs, README, and
      bundled skills to use `--remote`, `share`, `import`, and `ingest`.
- [x] Remove public references to `teleport`, `--host`, and top-level event
      `import` except historical notes inside completed goal files.
- [x] Ensure `--help` examples show the new workflow from first principles.

**Verification Contract**
- *Check:* Documentation no longer teaches legacy public names and examples line
  up with implemented help.
- *Method:* `rg -n "teleport|--host|mmr import" docs README.md specs .agents src tests`
  followed by manual review of intentional internal/historical matches.
- *Expected:* Remaining matches are private module names, internal implementation
  identifiers, or completed historical goal artifacts; public docs/help use only
  the new command surface.
- *BDD scenarios covered:* new-user path for remote read; source-side share;
  destination-side import; event ingest; no backwards compatibility.

**Confidence:** 95 / 90 · **Depends on:** T2, T3, T4

---

### T6 · Run full verification, live Mac Mini validation, commit, and push · [x]

**Steps**
- [x] Run local formatting, unit, integration, lint, and release build checks.
- [x] Run live SSH/Tailscale validation against `mini` for representative
      `--remote`, `share`, and `import --read-only` flows.
- [x] Review `git status --short` and stage only goal-related files.
- [x] Commit and push to the configured GitHub remote when verification is clean.

**Verification Contract**
- *Check:* The implementation works locally, works against the real Mac Mini
  peer, and is safely published.
- *Method:* `cargo fmt`, `cargo test`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo build --release`, live `mini` commands, `git status --short`,
  `git log -1 --oneline`, and `git push origin main`.
- *Expected:* All checks pass; live commands return `status: ok`; no unrelated
  files are staged; pushed commit appears on `origin/main`.
- *BDD scenarios covered:* full QA loop; live peer read; live read-only session
  import; source-side share smoke where non-mutating or safely dry-run; publish.

**Confidence:** 96 / 90 · **Depends on:** T5

---

## 6. Decisions · LIVE (append-only)

Starts empty. One subsection per meaningful decision/concession needing visibility.
**Scope impact must be `none`**; if a choice changes scope it is a `SCOPE-CHANGE`
exit, not a decision.

*(none yet)*

### 2026-06-01 · Remote target is a raw SSH target, not stored config

- Context: The user explicitly rejected a trusted-host registry and wanted
  `mmr` to trust whatever SSH target string the caller passes.
- Decision: Public read/query commands accept `--remote <ssh-target>` and import
  accepts `--from <ssh-target>`; the value is passed directly to SSH peer calls.
- Alternatives rejected: known-hosts inventory, Tailscale discovery, project or
  user config for trusted hosts.
- Why it needs surfacing: Future work must not add config friction unless scope
  changes.
- Scope impact: none

### 2026-06-01 · Public commands are share/import/ingest; teleport stays private

- Context: `teleport` was too abstract for the user-visible workflow, but the
  existing native bundle implementation is still useful.
- Decision: Remove the public `teleport` namespace; expose source-side movement
  as `share session`, destination-side movement as `import session` and
  `import bundle`, and normalized event ingestion as `ingest events`.
- Alternatives rejected: keeping aliases, deprecated shims, or a public
  `teleport` compatibility layer.
- Why it needs surfacing: Internal module names may still say `teleport`, but
  docs/help/tests must not teach it as a public command.
- Scope impact: none

### 2026-06-01 · Live summarize can use a local OpenAI-compatible mock

- Context: The environment did not expose `OPENAI_API_KEY`, but the goal still
  required live `mini` coverage for summarize remote fetch behavior.
- Decision: Validate `summarize project --remote mini` by pointing
  `OPENAI_BASE_URL` at a one-shot local Chat Completions mock while fetching the
  transcript from `mini` over SSH.
- Alternatives rejected: skipping live summarize, or using a real provider key
  from ambient credentials.
- Why it needs surfacing: This proves the remote transcript path without making
  external provider credentials part of the goal.
- Scope impact: none

---

## 7. Learnings · LIVE (append-only)

Compact flash cards. Format: **Trigger → Wrong action → Revision → Correct action**,
plus an **impact** score `1–5` (risk aversion: 5 = ignoring it is high-risk/costly).

*(none yet)*

- **Mac Mini project responses omitted `aliases` → Treating peer schema as
  identical to local structs → Add serde defaults for optional/omitted peer
  fields → Remote response DTOs must tolerate fields hidden by
  `skip_serializing_if` when deserializing older or empty peer output**; impact:
  4/5.
- **SSH share probe used `mmr --version` → Assuming root CLI already had a
  version flag → Add clap version metadata and a contract test → Any command
  used by peer probes must have an explicit CLI contract test**; impact: 4/5.
- **Docs verification used `rg "mmr import"` → Treating all matches as legacy
  noise → Manually classify matches → `mmr import session` and
  `mmr import bundle` are desired public docs, while top-level event import is
  the removed legacy spelling**; impact: 3/5.
- **Subagent review found stale public wording/env behavior → Treating full QA as
  enough → Fix review findings and add regression tests before commit → A passing
  subagent review is required evidence, not a formality**; impact: 4/5.

---

## 8. Skills · LIVE (append-only)

Generalisable, reusable workflows learned or created via the **skill-creator** skill.
One bullet each.

*(none yet)*
