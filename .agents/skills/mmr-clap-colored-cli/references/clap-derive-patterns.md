# Clap Derive Patterns

## Table of Contents

- [Root Parser + Global Flags](#root-parser--global-flags)
- [Subcommand Layout](#subcommand-layout)
- [Sort Enums and Defaults](#sort-enums-and-defaults)
- [Nested Args for `remember`](#nested-args-for-remember)
- [Upstream Clap Patterns (via wit)](#upstream-clap-patterns-via-wit)
- [Gotchas](#gotchas)

## Root Parser + Global Flags

Use global flags at the root and let every subcommand reuse them.

```rust
#[derive(Parser, Debug)]
#[command(name = "mmr")]
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
    },
    Remember(RememberArgs),
}
```

Source: `src/cli.rs`

## Sort Enums and Defaults

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

`SortOrder` follows the same pattern:

```rust
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[clap(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    Asc,
    Desc,
}
```

Source: `src/types/domain.rs`

## Nested Args for `remember`

Use `Args` for shared flags and a nested `Subcommand` for selectors like `all` and `session <id>`.

```rust
#[derive(Args, Debug)]
pub struct RememberArgs {
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,
    #[arg(long, value_enum, global = true)]
    agent: Option<Agent>,
    #[arg(short = 'O', long = "output-format", value_enum, default_value = "md", global = true)]
    output_format: RememberOutputFormatArg,
    #[command(subcommand)]
    selection: Option<RememberSelectorCommand>,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum RememberSelectorCommand {
    All,
    Session { session_id: String },
}
```

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
- Keep clap help text aligned with runtime behavior. `messages --session <id>` has special scoping semantics, and `remember` defaults to markdown output, so derive annotations and surrounding docs must stay in sync with `run_cli`.
