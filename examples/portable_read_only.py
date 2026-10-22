#!/usr/bin/env python3
"""Export/copy/read a synthetic session without touching native histories."""

from pathlib import Path
import shutil
from tempfile import TemporaryDirectory

from _synthetic import SESSION, binary, native_snapshot, run, seed


def main():
    mmr = binary()
    with TemporaryDirectory(prefix="mmr-portable-example-") as temporary:
        root = Path(temporary)
        source, destination = root / "source-home", root / "destination-home"
        source_project = seed(source)
        # Same session/file name, different text: a collision must survive reading.
        destination_project = seed(destination, "Destination-only work; leave this unchanged.")
        before = [native_snapshot(home) for home in (source, destination)]

        expected = run(mmr, source, source_project, "--source", "codex", "read", "session", SESSION)
        shared = run(mmr, source, source_project, "--source", "codex", "share", "session", SESSION,
                     "--to", f"file://{root / 'outbox'}")
        # Copy the whole published entry, including its checksum and ready marker.
        copied = root / "copied-entry"
        shutil.copytree(shared["inbox_path"], copied)
        imported = run(mmr, destination, destination_project, "import", "bundle", str(copied))
        assert imported["messages"] == expected["messages"], "portable messages changed"
        assert "apply" not in imported, "default import unexpectedly applied native files"
        assert Path(imported["bundle_path"]).is_file(), "verified bundle was not cached"
        assert [native_snapshot(home) for home in (source, destination)] == before, "native histories changed"

        print("Copied a JSON bundle and verified its hashes; no SSH or server needed.")
        for message in imported["messages"]:
            print(f"{message['role']}: {message['content']}")
        print("Source and destination native histories unchanged (including a colliding session).")
        print("Bundle cached for reading; no native apply requested.")
    print("Temporary homes removed.")


if __name__ == "__main__":
    main()
