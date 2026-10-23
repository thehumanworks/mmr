"""Check only lexical ordering of a synthetic release manifest's path list."""
import json
import sys
from pathlib import Path


def main():
    document = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    paths = document["release_manifest_paths"]
    if not isinstance(paths, list) or not all(isinstance(path, str) for path in paths):
        print("FAIL: release_manifest_paths must be a list of strings")
        return 2
    if paths != sorted(paths):
        print("FAIL: release manifest paths are not lexically ascending")
        return 1
    print("PASS: release manifest paths are lexically ascending")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
