test:
    cargo test --all

install:
    cargo install --path .

build:
    cargo build --release

alias b := build
alias i := install
alias t := test
