# Clap Derive Patterns

## Table of Contents
- [Root parser and global flags](#root-parser-and-global-flags)
- [Subcommand layout](#subcommand-layout)
- [Typed enums and defaults](#typed-enums-and-defaults)
- [Current mmr-specific CLI patterns](#current-mmr-specific-cli-patterns)
- [Upstream Clap patterns (via wit)](#upstream-clap-patterns-via-wit)
- [Gotchas](#gotchas)

## Root Parser and Global Flags

Use global flags at the root and let every subcommand reuse them.

```rust
#[derive(Parser, Debug)]
#[command(
    name = "mmr",
    about = "Browse AI conversation history from Claude, Codex, and Cursor"
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Filter by source: claude, codex, cursor (omit to use MMR_DEFAULT_SOURCE or all)
    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceFilter>,

    #[command(subcommand)]
    pub command: Commands,
}
```

Source: `src/cli.rs`

## Subcommand Layout

Keep each subcommand as a structured variant with typed args and explicit defaults.

```rust
#[derive(Subcommand, Debug)]
pub enum Commands {
    Projects {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(short = 's', long, default_value = "timestamp")]
        sort_by: SortBy,
        #[arg(short = 'o', long, default_value = "desc")]
        order: SortOrder,
    },
    Sessions {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    Messages {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
}
```

Source: `src/cli.rs`

## Typed Enums and Defaults

Use `ValueEnum` + kebab-case names to preserve CLI and JSON compatibility.

```rust
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SortBy {
    #[default]
    Timestamp,
    #[value(name = "message-count")]
    MessageCount,
}
```

Current enum location:

- `SourceFilter`, `SortBy`, `SortOrder`, `SourceKind`, `MessageRecord`, and `Agent` live in `src/types/domain.rs`.
- Public JSON envelopes live in `src/types/api.rs`.

## Current mmr-Specific CLI Patterns

- Use `Option<T>` for filters that can be omitted (`--source`, `--project`, `--session`).
- Use `bool` flags for scope switches like `--all`.
- Prefer `default_value_t` for numeric defaults and `default_value` for enum defaults.
- Keep env-based defaults in helper functions (`effective_source`, `effective_remember_agent`, `effective_project_scope`) instead of encoding them in clap attributes.
- For commands with nested behavior such as `remember`, use `Args` plus a nested `Subcommand` enum for selectors (`latest`, `all`, `session <id>`).

Source: `src/cli.rs`

## Upstream Clap Patterns (via wit)

### Subcommand Derive

```rust
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
```

Source: `clap-rs/clap/examples/tutorial_derive/03_04_subcommands.rs:3-9`

### Typed Defaults

```rust
#[arg(default_value_t = 2020)]
port: u16,
```

Source: `clap-rs/clap/examples/tutorial_derive/03_05_default_values.rs:6-7`

### Global Option + from_global

```rust
#[arg(global = true, long)]
other: bool,

#[arg(from_global)]
other: bool,
```

Source: `clap-rs/clap/tests/derive/subcommands.rs:185-200`

## Gotchas

- `#[arg(from_global)]` cannot be combined with option builders like `short`/`long`.
  - Source: `clap-rs/clap/clap_derive/src/item.rs:885-890`
- Keep `#[arg(default_value_t)]` type-safe; mismatched types fail derive.
  - Source: `clap-rs/clap/tests/derive_ui/default_value_t_invalid.rs:14-16`
