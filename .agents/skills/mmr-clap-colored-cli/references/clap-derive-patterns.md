# Clap Derive Patterns

## Table of Contents
- [Root Parser + Global Flags](#root-parser--global-flags)
- [Subcommand Layout](#subcommand-layout)
- [Nested Args for `remember`](#nested-args-for-remember)
- [Sort Enums and Defaults](#sort-enums-and-defaults)
- [Upstream Clap Patterns (via wit)](#upstream-clap-patterns-via-wit)
- [Gotchas](#gotchas)

## Root Parser + Global Flags

Use global flags at the root and let every subcommand reuse them.

```rust
#[derive(Parser, Debug)]
#[command(
    name = "mmr",
    about = "Browse AI conversation history from Claude, Codex, Cursor, Grok, and Pi"
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Filter by source: claude, codex, cursor, grok, pi
    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceFilter>,

    #[command(subcommand)]
    pub command: Commands,
}
```

Source: `src/cli.rs`

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
    },
    Sessions {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Messages {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, num_args = 0..=1, default_missing_value = "1")]
        latest: Option<NonZeroUsize>,
    },
}
```

Source: `src/cli.rs`

## Nested Args for `remember`

Use `Args` for shared flags and a nested subcommand when one command has its own selector surface.

```rust
#[derive(Args, Debug)]
pub struct RememberArgs {
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,
    #[arg(long, value_enum, global = true)]
    agent: Option<Agent>,
    #[command(subcommand)]
    selection: Option<RememberSelectorCommand>,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum RememberSelectorCommand {
    All,
    Session { session_id: String },
}
```

Why this matters:
- `agent` stays optional so runtime code can apply `MMR_DEFAULT_REMEMBER_AGENT`.
- The nested selector keeps `remember`, `remember all`, and `remember session <id>` on one command family without overloading flags.

Source: `src/cli.rs`

## Sort Enums and Defaults

Use `ValueEnum` + kebab-case names to preserve wire compatibility.

```rust
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[clap(rename_all = "kebab-case")]
pub enum SortBy {
    #[default]
    Timestamp,
    #[value(name = "message-count")]
    MessageCount,
}
```

Pair sort enums with explicit command defaults:

```rust
#[arg(short = 's', long, default_value = "timestamp")]
sort_by: SortBy,

#[arg(short = 'o', long, default_value = "asc")]
order: SortOrder,
```

Source: `src/types/domain.rs`, `src/cli.rs`

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
- Keep runtime-defaulted flags as `Option<T>` when env or cwd helpers decide the real default after parsing.
  - Example: `remember.agent: Option<Agent>` so `MMR_DEFAULT_REMEMBER_AGENT` can apply in `run_cli()`.
- Use `num_args = 0..=1` plus `default_missing_value` for optional-value switches like `--latest`, where bare presence should mean `1` but an explicit number is also valid.
