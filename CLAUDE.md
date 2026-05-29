# CLAUDE.md

Agent guidance for this repository lives in [`AGENTS.md`](./AGENTS.md). Read it first.

## Goal-first workflow (required)

Every interaction with this codebase must start by capturing the user's request
as a goal-driven prompt document under `goals/`. Before writing or changing code,
create `goals/<YYYY-MM-DD>-<kebab-title>.md` with YAML frontmatter (`title`,
`description`, `date`, `status`) and a body that states the outcome, the surface
touched, the validation plan, and the definition of done. Drive the work from
that document and keep its `status` current (`in-progress` → `done`, or `blocked`
with the smallest missing fact). See `goals/` for examples, and `AGENTS.md` for
the full contract, build/test commands, and the mandatory verification loop.
