#!/bin/sh
# Usage: reproduce.sh /absolute/path/to/isolated-evaluation-directory
# Requires Git, rustup/Cargo, Bun (bun and bunx), and the Apple command line tools.
set -eu
ROOT=${1:?provide an isolated directory}
case "$ROOT" in /*) ;; *)
    echo "directory must be absolute" >&2
    exit 2
    ;;
esac
FIXTURES=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$ROOT" "$ROOT/bin"
if [ ! -d "$ROOT/source/.git" ]; then
    git clone https://github.com/tangramdotdev/tangram.git "$ROOT/source"
fi
if [ ! -d "$ROOT/packages/.git" ]; then
    git clone https://github.com/tangramdotdev/packages.git "$ROOT/packages"
fi
git -C "$ROOT/source" checkout --detach e80203a092aa2a2fab7f9123a67ac7b14a37d278
git -C "$ROOT/packages" checkout --detach d522ff1a63b7e2245d77aa0c400457836f67730e
export RUSTUP_HOME="$ROOT/rustup" CARGO_HOME="$ROOT/cargo" RUSTUP_TOOLCHAIN=1.100.0
export CARGO_TARGET_DIR="$ROOT/target" BUN_INSTALL_CACHE_DIR="$ROOT/bun-cache"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off
rustup toolchain install 1.100.0 --profile minimal --no-self-update
(cd "$ROOT/source" && cargo build --locked --package tangram_cli --bin tangram --no-default-features --features lmdb,sqlite,tls,quickjs,tsrs,python -j 6)
ln -sf ../target/debug/tangram "$ROOT/bin/tg"
ln -sf ../target/debug/tangram "$ROOT/bin/tangram"
export PATH="$ROOT/bin:$PATH"
printf '{}\n' > "$ROOT/config.json"
for name in managed-bootstrap managed-native; do
    mkdir -p "$ROOT/$name"
    cp "$FIXTURES/$name/tangram.ts" "$ROOT/$name/tangram.ts"
done
tg --version
otool -l "$ROOT/bin/tg" | sed -n '/LC_BUILD_VERSION/,+6p'
tg -c "$ROOT/config.json" -d "$ROOT/.tangram" --no-remotes build --checkout="$ROOT/managed-bootstrap-hello" "$ROOT/managed-bootstrap"
"$ROOT/managed-bootstrap-hello"
artifact=$(tg -c "$ROOT/config.json" -d "$ROOT/.tangram" --no-remotes build --no-tokens "$ROOT/managed-native#standardSdk")
tg -c "$ROOT/config.json" -d "$ROOT/.tangram" --no-remotes run "$artifact"
# Stop only the isolated test daemon after the checks.
tg -c "$ROOT/config.json" -d "$ROOT/.tangram" --no-remotes server stop
