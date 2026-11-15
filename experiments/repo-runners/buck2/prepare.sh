#!/bin/sh
# Capture the selected checkout's committed source as a declared Buck2 input.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive=$(mktemp "$here/source.tar.XXXXXXXX")
trap 'rm -f "$archive"' EXIT HUP INT TERM
root=$(git -C "$here" rev-parse --show-toplevel)
git -C "$root" archive --format=tar HEAD > "$archive"
if ! cmp -s "$archive" "$here/source.tar"; then
    mv "$archive" "$here/source.tar"
fi
