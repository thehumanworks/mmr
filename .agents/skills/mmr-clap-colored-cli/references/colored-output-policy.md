# Colored Output Policy

## Table of Contents
- [Local Policy in mmr](#local-policy-in-mmr)
- [Core Colorize API](#core-colorize-api)
- [Environment-Controlled Coloring](#environment-controlled-coloring)
- [Manual Override API](#manual-override-api)

## Local Policy in mmr

Keep stdout machine-readable JSON. Use color only for human-facing stderr paths.

```rust
match run_cli(cli) {
    Ok(json) => println!("{json}"),
    Err(error) => {
        eprintln!("{} {}", "error:".red().bold(), error);
        std::process::exit(1);
    }
}
```

Source: `src/main.rs:8-13`

## Core Colorize API

`Colorize` chains style methods on `&str` and `String`.

```rust
use colored::Colorize;

"this is red".red();
"this is red on blue".red().on_blue();
"you can also make bold text".bold();
"this is default color and style".red().bold().clear();
```

Source: `colored-rs/colored/src/lib.rs:3-18`

## Environment-Controlled Coloring

Color behavior priority in upstream colored:
1. `CLICOLOR_FORCE`
2. `NO_COLOR`
3. `CLICOLOR` + TTY check

Source: `colored-rs/colored/src/control.rs:102-111`

## Manual Override API

Use override only when a command explicitly needs to force color/no-color.

```rust
colored::control::set_override(true);
colored::control::set_override(false);
colored::control::unset_override();
```

Sources:
- `colored-rs/colored/examples/control.rs:47-53`
- `colored-rs/colored/src/control.rs:75-83`
