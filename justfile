test:
    cargo test --all

install:
    cargo install --path .

build:
    cargo build --release

export:
    uv run --script scripts/export.py

alias e := export
alias b := build
alias i := install
alias t := test
