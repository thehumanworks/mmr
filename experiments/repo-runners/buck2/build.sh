#!/bin/sh
# Buck2 owns action/output caching; Cargo owns the Rust compilation graph.
set -eu
output=$1
archive=$2
scratch=$(mktemp -d "${TMPDIR:-/tmp}/mmr-buck2.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
mkdir -p "$scratch/source"
tar -xf "$archive" -C "$scratch/source"
export CARGO_HOME="$scratch/cargo"
export CARGO_TARGET_DIR="$scratch/target"
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
cargo install --path "$scratch/source" --locked --root "$scratch/install"
mkdir -p "$(dirname "$output")"
cp "$scratch/install/bin/mmr" "$output"
chmod +x "$output"
