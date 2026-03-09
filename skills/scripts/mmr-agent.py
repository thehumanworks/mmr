#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "google-genai>=1.66.0",
#     "python-dotenv>=1.2.2",
# ]
# ///

from concurrent.futures import ThreadPoolExecutor
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Literal
import warnings

import google.genai as genai
from dotenv import load_dotenv

warnings.filterwarnings("ignore", message=".*Interactions usage is experimental.*")

load_dotenv()

api_key = os.getenv("GOOGLE_API_KEY") or os.getenv("GEMINI_API_KEY")
if api_key is None:
    raise ValueError("GOOGLE_API_KEY or GEMINI_API_KEY is not set")

client = genai.Client(api_key=api_key)

MEMORY_AGENT_MODEL = "gemini-3-flash-preview"

MEMORY_AGENT_SYSTEM_INSTRUCTION = """\
You are a Memory Agent — a specialized summarizer that processes AI coding session transcripts and produces structured continuity briefs. Your sole purpose is to enable an AI agent resuming work on this project to pick up exactly where the previous session left off, at the highest quality.

## Input Format

You receive a list of messages from one or more coding sessions, ordered most recent first. Each message has a role (user/assistant/tool), content, and a timestamp. Messages may span multiple sessions, each identified by a session_id.

## Output Format

Produce a structured summary with these sections. Omit any section that has no relevant content.

### 1. Status
One sentence: what state is the project/task in right now? Is there an open task in progress, or was the last session completed cleanly?

### 2. What Was Done
Bullet list of concrete changes made, ordered by importance:
- Files created, modified, or deleted (with paths)
- Features implemented, bugs fixed, refactors applied
- Tests added or modified, and their pass/fail status at session end
- Dependencies added or changed
- Configuration or infrastructure changes

### 3. Key Decisions & Context
Bullet list of decisions made during the session(s) that a resuming agent needs to know:
- Architectural choices and why they were made
- Approaches that were tried and abandoned (and why)
- Constraints or requirements discovered during the work
- User preferences or instructions that affect future work

### 4. Open Items
Bullet list of anything unfinished, blocked, or explicitly deferred:
- Tasks started but not completed
- Known bugs or failing tests at session end
- TODOs mentioned by user or agent
- Questions that were raised but not answered

### 5. Relevant File Map
List the key files involved in the work with a one-line description of each file's role. Only include files that were actively worked on or are critical context for resuming.

### 6. Resume Instructions
A short paragraph (2-4 sentences) telling the resuming agent exactly what to do first. Be specific: name the file, the function, the test, the command. This is the most important section — it should be actionable enough that the resuming agent can start working immediately without re-reading the full history.

## Rules

- Be precise. Use exact file paths, function names, variable names, and command invocations.
- Be concise. This is a working document, not a narrative. No filler, no hedging.
- Prioritize recency. More recent sessions matter more than older ones.
- Distinguish facts from intent. "User asked for X" is different from "X was implemented".
- If messages are from multiple sessions, note session boundaries only when the context shift matters.
- If the conversation is too short or trivial to warrant a full summary, say so in one line and skip the sections.
"""


def get_mmr_sessions(project_id: str = Path.cwd().as_posix()) -> dict:
    return json.loads(
        subprocess.run(
            ["mmr", "sessions", "--project", project_id],
            capture_output=True,
            text=True,
        ).stdout
    )


def get_codex_sessions(
    project_id: str = Path.cwd().as_posix(), mode: Literal["latest", "all"] = "latest"
):
    match mode:
        case "latest":
            return get_mmr_sessions(project_id)["sessions"][-1]["session_id"]
        case "all":
            return [
                session["session_id"]
                for session in get_mmr_sessions(project_id)["sessions"]
            ]
        case _:
            raise ValueError(f"Invalid mode: {mode}")


def _fetch_session_messages(
    session_id: str,
) -> tuple[str, list[dict] | None, str | None]:
    """Fetch messages for one session. Returns (session_id, messages, stderr)."""
    p = subprocess.run(
        ["mmr", "messages", "--session", session_id],
        capture_output=True,
        text=True,
    )
    if p.returncode != 0:
        return (session_id, None, p.stderr)
    return (session_id, json.loads(p.stdout)["messages"], None)


def get_codex_messages(sessions: list[str] | str):
    if isinstance(sessions, str):
        sessions = [sessions]
    if not sessions:
        return []

    with ThreadPoolExecutor(max_workers=min(len(sessions), 16)) as ex:
        results = list(ex.map(_fetch_session_messages, sessions))

    session_messages = []
    for session_id, messages, stderr in results:
        if messages is None:
            if stderr:
                print(stderr, file=sys.stderr)
            return []
        session_messages.append({"session_id": session_id, "messages": messages})

    def _first_ts(sm: dict) -> str:
        msgs = sm["messages"]
        return min(m["timestamp"] for m in msgs) if msgs else ""

    session_messages.sort(key=_first_ts, reverse=True)

    return session_messages if len(session_messages) > 1 else session_messages[0]


def _format_messages_for_input(session_data: list[dict] | dict) -> str:
    """Format session messages into a text block for the memory agent input."""
    if isinstance(session_data, dict):
        session_data = [session_data]

    parts = []
    for session in session_data:
        sid = session["session_id"]
        parts.append(f"=== Session: {sid} ===")
        for msg in session["messages"]:
            role = msg.get("role", "unknown")
            content = msg.get("content", "")
            ts = msg.get("timestamp", "")
            # Truncate very long tool outputs to keep input manageable
            if role == "tool" and len(content) > 2000:
                content = content[:2000] + "\n... [truncated]"
            parts.append(f"[{ts}] {role}: {content}")
        parts.append("")

    return "\n".join(parts)


def run_memory_agent(
    session_data: list[dict] | dict,
    previous_interaction_id: str | None = None,
    follow_up: str | None = None,
) -> tuple[str, str]:
    """Run the memory agent on session data.

    Args:
        session_data: Session messages from get_codex_messages().
            Ignored when follow_up is provided with a previous_interaction_id.
        previous_interaction_id: ID from a prior memory agent interaction
            to continue the conversation.
        follow_up: A follow-up question or request. When provided with
            previous_interaction_id, this is sent instead of the session data,
            allowing the caller to ask clarifying questions or request
            deeper analysis on specific aspects of the summary.

    Returns:
        A tuple of (summary_text, interaction_id). The interaction_id can be
        passed back as previous_interaction_id for follow-up calls.
    """
    if follow_up and previous_interaction_id:
        # Continue from previous interaction with a follow-up question
        interaction = client.interactions.create(
            model=MEMORY_AGENT_MODEL,
            system_instruction=MEMORY_AGENT_SYSTEM_INSTRUCTION,
            input=follow_up,
            previous_interaction_id=previous_interaction_id,
        )
    else:
        # Initial summarization of session data
        formatted = _format_messages_for_input(session_data)
        input_text = (
            "Analyze the following AI coding session transcript(s) and produce "
            "a continuity brief.\n\n"
            f"{formatted}"
        )
        kwargs = {
            "model": MEMORY_AGENT_MODEL,
            "system_instruction": MEMORY_AGENT_SYSTEM_INSTRUCTION,
            "input": input_text,
        }
        if previous_interaction_id:
            kwargs["previous_interaction_id"] = previous_interaction_id
        interaction = client.interactions.create(**kwargs)

    return interaction.outputs[-1].text, interaction.id


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Memory Agent — session summarizer")
    parser.add_argument(
        "--mode",
        choices=["latest", "all"],
        default="all",
        help="Which sessions to summarize (default: all)",
    )
    parser.add_argument(
        "--continue-from",
        dest="continue_from",
        default=None,
        help="Interaction ID to continue from (for follow-ups)",
    )
    parser.add_argument(
        "--follow-up",
        dest="follow_up",
        default=None,
        help="Follow-up question to ask about a previous summary",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output as JSON with summary and interaction_id",
    )
    args = parser.parse_args()

    if args.follow_up and args.continue_from:
        # Follow-up mode: no need to fetch sessions
        summary, interaction_id = run_memory_agent(
            session_data=[],
            previous_interaction_id=args.continue_from,
            follow_up=args.follow_up,
        )
    else:
        # Initial summarization mode
        sessions = get_codex_sessions(mode=args.mode)
        messages = get_codex_messages(sessions)
        summary, interaction_id = run_memory_agent(
            session_data=messages if isinstance(messages, list) else [messages],
            previous_interaction_id=args.continue_from,
        )

    if args.json:
        print(json.dumps({"summary": summary, "interaction_id": interaction_id}))
    else:
        print(summary)
        print(f"\n---\nInteraction ID: {interaction_id}")
        print("(Use --continue-from <id> --follow-up '<question>' to ask follow-ups)")
