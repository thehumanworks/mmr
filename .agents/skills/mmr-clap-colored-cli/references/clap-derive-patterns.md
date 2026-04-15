# Clap Derive Patterns

## Table of Contents
- [Root Parser + Global Flags](#root-parser--global-flags)
- [Subcommand Layout](#subcommand-layout)
- [Sort Enums and Defaults](#sort-enums-and-defaults)
- [Nested Args for Remember](#nested-args-for-remember)
- [Upstream Clap Patterns (via wit)](#upstream-clap-patterns-via-wit)
- [Gotchas](#gotchas)

Primary sources: `src/cli.rs` and `src/types/domain.rs`.

## Root Parser + Global Flags

Use global flags at the root and let every subcommand reuse them.

```rust
#[derive(Parser, Debug)]
#[command(
    name = "mmr",
    about = "Browse AI conversation history from Claude, Codex, and Cursor"
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    #[arg(long, global = true)]
    pub pretty: bool,

    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceFilter>,

    #[command(subcommand)]
    pub command: Commands,
}
```

Use root-level global flags for output formatting and cross-command source filtering.

## Subcommand Layout

Keep each subcommand as a structured variant with typed args.

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
    },
    Messages {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Export {
        #[arg(long)]
        project: Option<String>,
    },
}
```

Keep subcommands as typed enum variants with per-command defaults and optional filters rather than parsing raw strings downstream.

## Sort Enums and Defaults

Use `ValueEnum` + kebab-case names to preserve CLI compatibility and serde naming.

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

Pair this with `SortOrder` and `SourceFilter`, which follow the same derive pattern.

## Nested Args for Remember

Use a dedicated `Args` struct when one subcommand owns a family of related flags and subcommands.

```rust
#[derive(Args, Debug)]
pub struct RememberArgs {
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,
    #[arg(long, value_enum, global = true)]
    agent: Option<Agent>,
    #[arg(long, global = true)]
    instructions: Option<String>,
    #[command(subcommand)]
    selection: Option<RememberSelectorCommand>,
}
```

This keeps the root `Commands::Remember(RememberArgs)` variant small while still allowing typed selectors like `remember all` and `remember session <id>`.

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
