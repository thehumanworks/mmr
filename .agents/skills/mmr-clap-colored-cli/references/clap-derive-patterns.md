# Clap Derive Patterns

## Table of Contents
- [Root Parser + Global Flags](#root-parser--global-flags)
- [Subcommand Layout](#subcommand-layout)
- [Sort Enums and Defaults](#sort-enums-and-defaults)
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
}
```

Source: `src/cli.rs:8-24`

## Subcommand Layout

Keep each subcommand as a structured variant with typed args.

```rust
#[derive(Subcommand, Debug)]
pub enum Commands {
    Projects {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value = "last-activity")]
        sort_by: ProjectSortBy,
    },
    Sessions {
        #[arg(long)]
        project: String,
        #[arg(long)]
        limit: Option<usize>,
    },
}
```

Source: `src/cli.rs:27-55`

## Sort Enums and Defaults

Use `ValueEnum` + kebab-case names to preserve wire compatibility.

```rust
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[clap(rename_all = "kebab-case")]
pub enum SessionSortBy {
    #[default]
    #[value(name = "last-activity")]
    LastActivity,
    #[value(name = "message-count")]
    MessageCount,
}
```

Source: `src/model.rs:25-40`

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
