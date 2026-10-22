"""Small, standard-library-only fixture shared by the runnable examples."""

import json
import os
from pathlib import Path
import subprocess
import sys


SESSION = "example-session"
PROVIDERS = (".codex", ".claude", ".cursor", ".grok", ".pi")


def binary():
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "target/debug/mmr"
    path = path.resolve()
    if not path.is_file():
        raise SystemExit("Build first: cargo build --locked (or pass the mmr binary path)")
    return str(path)


def seed(home, text="Keep the retry limit at three."):
    """Write only caller-owned temporary files, never an existing user home."""
    home.mkdir()
    project = home / "demo project"
    project.mkdir()
    native = home / ".codex/sessions/example.jsonl"
    native.parent.mkdir(parents=True)
    rows = [
        {"type": "session_meta", "timestamp": "2025-01-02T00:00:00Z",
         "payload": {"id": SESSION, "cwd": str(project.resolve())}},
        {"type": "event_msg", "timestamp": "2025-01-02T00:00:01Z",
         "payload": {"type": "user_message", "message": text}},
        {"type": "response_item", "timestamp": "2025-01-02T00:00:02Z",
         "payload": {"role": "assistant", "content": [{"type": "output_text", "text": "Noted for the next session."}]}},
    ]
    native.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
    return project


def environment(home):
    # Deliberately omit inherited provider paths, config, tokens and REST settings.
    return {"HOME": str(home), "PATH": os.defpath}


def run(mmr, home, project, *args):
    result = subprocess.run([mmr, *args], cwd=project, env=environment(home),
                            text=True, capture_output=True, timeout=15)
    if result.returncode:
        raise RuntimeError(f"mmr {' '.join(args)} failed: {result.stdout}{result.stderr}")
    return json.loads(result.stdout)


def native_snapshot(home):
    return {str(path.relative_to(home)): path.read_bytes()
            for provider in PROVIDERS for path in (home / provider).rglob("*")
            if path.is_file()}
