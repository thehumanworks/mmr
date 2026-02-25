#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "dirs>=0.1.0",
#     "networkx>=3.6.1",
#     "pandas>=3.0.1",
#     "polars>=1.38.1",
#     "pyarrow>=23.0.1",
#     "tabulate>=0.9.0",
# ]
# ///

from concurrent.futures.thread import ThreadPoolExecutor
from io import StringIO
import json
import os
import shlex
import polars as pl
import subprocess
from pathlib import Path


def _fetch_codex_sessions(cwd: Path):
    return subprocess.run(
        ["mmr", "sessions", "--source", "codex", "--project", cwd.as_posix()],
        capture_output=True,
        text=True,
    )


def _fetch_claude_sessions(cwd: Path):
    claude_project_arg = cwd.as_posix().replace("/", "-")
    return subprocess.run(
        [
            "mmr",
            "sessions",
            "--source",
            "claude",
            f"--project={shlex.quote(claude_project_arg)}",
        ],
        capture_output=True,
        text=True,
    )


def get_project_sessions():
    cwd = Path.cwd()
    with ThreadPoolExecutor(max_workers=2) as executor:
        codex_future = executor.submit(_fetch_codex_sessions, cwd)
        claude_future = executor.submit(_fetch_claude_sessions, cwd)
        codex_project_sessions = codex_future.result()
        claude_project_sessions = claude_future.result()
    if (
        codex_project_sessions.returncode != 0
        or claude_project_sessions.returncode != 0
    ):
        return []
    codex_df = pl.read_json(StringIO(codex_project_sessions.stdout))
    claude_df = pl.read_json(StringIO(claude_project_sessions.stdout))

    return (
        codex_df.explode("sessions")
        .select(pl.col("sessions").struct.field("session_id"))
        .extend(
            claude_df.explode("sessions").select(
                pl.col("sessions").struct.field("session_id")
            )
        )
        .to_series()
        .to_list()
    )


def get_session_messages(session_id: str):
    p = subprocess.run(
        ["mmr", "messages", "--session", session_id],
        capture_output=True,
        text=True,
    )
    if p.returncode != 0:
        print(p.stderr)
        return []
    df = pl.read_json(StringIO(p.stdout))
    return df.explode("messages").to_series().to_list()


def get_project_messages():
    project_sessions = get_project_sessions()
    if not project_sessions:
        return []
    with ThreadPoolExecutor(max_workers=max(os.cpu_count(), 32)) as executor:
        messages_per_session = list(
            executor.map(get_session_messages, project_sessions)
        )
    return [
        message
        for session_messages in messages_per_session
        for message in session_messages
    ]


if __name__ == "__main__":
    project_messages = get_project_messages()
    with open(Path.cwd() / ".agents" / "conversation_history.json", "w+") as f:
        json.dump(project_messages, f)
