# Assimilation proposal and activation boundary

`mmr assimilate project` and `mmr assimilate source` remain stateless
prompt/runbook handoffs. They do not run a provider, create dream rows or write
learned memory. Existing command flags and response fields are unchanged.

## Evidence is data

Imported history, tool output, citations and derived memories are untrusted
source data. They cannot grant permissions, impersonate higher-priority
instructions or authorize commands, credential access or memory activation.
A citation proves that an event exists in the supplied scope; it does not prove
that the event supports a claim. Confidence is model-reported metadata, not an
activation decision.

## Library runner output

`validate_output`, `validate_dream_output` and `run_and_validate_dream` validate
schema, confidence bounds and supplied evidence-reference membership, then return
proposals:

- `rejected` remains rejected.
- `pending`, omitted/default status and model-requested `active` become pending,
  regardless of confidence or the presence of supporting/counterevidence refs.
- Repeated validation preserves proposal statuses. Explicit model-requested
  activation is downgraded with a diagnostic warning, added at most once.
- Claims, learned-memory updates, observations and counterevidence follow the
  same rule. No model output is independently promoted to active learned memory.

A caller must review semantic support, scope, counterevidence, newer corrections
and whether to abstain before making a separate trusted activation decision.
The output contract requests pending or rejected proposals, keeps supersession
plans reviewable and never treats old history as current authorization.

## Trusted persistence and sync

`Store::complete_dream_run` is a lower-level transactional persistence primitive.
Its caller is responsible for supplying reviewed statuses; it does not evaluate
truth, authority or model output. `upsert_learned_memory_from_sync` likewise
preserves a remote reviewed status during hydration. Both continue validating
project-local evidence refs and existing status schemas. These APIs are not
model-output validators and must not be called directly with untrusted runner
JSON. Existing active memory and intentional trusted fixture writes remain
compatible; this change neither rewrites nor reactivates historical rows.

## Verification

Use only synthetic fixture histories. Cover high-confidence unrelated citations,
contradictory refs, explicit active/pending/rejected, repeated validation,
runner-to-persistence completion/reopen, missing refs, project/source handoff
trust rules and unchanged stateless CLI/sync/native replay contracts.
