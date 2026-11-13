"""Write synthetic sessions in each agent harness's native on-disk format.

The layouts and record shapes follow what the harnesses wrote on a real
machine in November 2026 (checked structurally, without reading content):

* Claude Code 2.1: ``~/.claude/projects/<cwd with every non-alphanumeric
  character replaced by "-">/<uuid>.jsonl``; one JSON line per content block.
* Codex 0.164 (codex-tui): ``~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl``.
  User prompts appear as ``response_item`` messages with role ``user`` and as
  ``event_msg``/``item_completed`` ``UserMessage`` items; there is no
  ``event_msg``/``user_message`` record any more. Archived sessions live in
  ``~/.codex/archived_sessions/`` and are zstd-compressed (``.jsonl.zst``).
* Cursor agent: ``~/.cursor/projects/<cwd without leading "/", "/" -> "-">/
  agent-transcripts/<uuid>/<uuid>.jsonl``; records carry no timestamp or cwd;
  user text is wrapped as ``<timestamp>…</timestamp>\\n<user_query>…</user_query>``.
* Grok: ``~/.grok/sessions/<url-encoded cwd>/<id>/{summary.json,updates.jsonl}``.
* Pi: ``~/.pi/agent/sessions/--<cwd with "/" -> "-">--/<ts>_<id>.jsonl``.

A session is a list of steps:
    ("user", text)
    ("assistant", text)
    ("thinking", text)
    ("tool", kind, argument, output)   kind is "shell", "read" or "edit"
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import urllib.parse
import uuid
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path


@dataclass
class Session:
    key: str
    harness: str  # claude | codex | codex-archived | cursor | grok | pi
    cwd: str
    start: datetime
    steps: list
    model: str = ""
    session_id: str = ""

    def __post_init__(self):
        # Stable ids so a probe can find a session again after rebuilding the corpus.
        if not self.session_id:
            self.session_id = str(uuid.uuid5(uuid.NAMESPACE_URL, f"mmr-eval/{self.key}"))


def iso(ts: datetime) -> str:
    return ts.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def step_times(session: Session):
    """Yield (step, timestamp) with a plausible pace between steps."""
    ts = session.start
    for index, step in enumerate(session.steps):
        yield step, ts
        kind = step[0]
        ts = ts + timedelta(seconds={"user": 25, "assistant": 40, "thinking": 6, "tool": 9}[kind] + index % 7)


def write_jsonl(path: Path, rows: list) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")


# --------------------------------------------------------------------------- Claude Code


def claude_slug(cwd: str) -> str:
    return re.sub(r"[^A-Za-z0-9]", "-", cwd)


def write_claude(home: Path, s: Session) -> Path:
    path = home / ".claude" / "projects" / claude_slug(s.cwd) / f"{s.session_id}.jsonl"
    model = s.model or "claude-opus-4-8"
    rows = []
    parent = None
    base = {
        "isSidechain": False,
        "userType": "external",
        "entrypoint": "cli",
        "cwd": s.cwd,
        "sessionId": s.session_id,
        "version": "2.1.300",
        "gitBranch": "main",
    }

    def add(kind: str, ts: datetime, message: dict, extra: dict | None = None):
        nonlocal parent
        row_uuid = str(uuid.uuid4())
        row = {"parentUuid": parent, **base, "type": kind, "message": message, "uuid": row_uuid, "timestamp": iso(ts)}
        if extra:
            row.update(extra)
        rows.append(row)
        parent = row_uuid

    rows.append({"type": "file-history-snapshot", "messageId": str(uuid.uuid4()), "snapshot": {"trackedFileBackups": {}, "timestamp": iso(s.start)}, "isSnapshotUpdate": False})
    msg_counter = 0
    for step, ts in step_times(s):
        kind = step[0]
        if kind == "user":
            add("user", ts, {"role": "user", "content": step[1]}, {"permissionMode": "default"})
            continue
        msg_counter += 1
        msg_id = f"msg_{s.session_id[:8]}{msg_counter:04d}"
        usage = {"input_tokens": 6, "cache_read_input_tokens": 21000 + 900 * msg_counter, "output_tokens": 180}
        envelope = {"id": msg_id, "type": "message", "role": "assistant", "model": model, "stop_reason": None, "usage": usage}
        if kind == "assistant":
            add("assistant", ts, {**envelope, "content": [{"type": "text", "text": step[1]}]}, {"requestId": f"req_{msg_id}"})
        elif kind == "thinking":
            add("assistant", ts, {**envelope, "content": [{"type": "thinking", "thinking": step[1], "signature": "EqQBCkYIBxgCKkA" + "x" * 40}]}, {"requestId": f"req_{msg_id}"})
        elif kind == "tool":
            _, tool, arg, output = step
            tool_id = f"toolu_{uuid.uuid4().hex[:24]}"
            if tool == "shell":
                name, tool_input = "Bash", {"command": arg, "description": "Run command"}
            elif tool == "read":
                name, tool_input = "Read", {"file_path": f"{s.cwd}/{arg}"}
            else:
                name, tool_input = "Edit", {"file_path": f"{s.cwd}/{arg}", "old_string": "...", "new_string": "..."}
            add("assistant", ts, {**envelope, "content": [{"type": "tool_use", "id": tool_id, "name": name, "input": tool_input}]}, {"requestId": f"req_{msg_id}"})
            add(
                "user",
                ts + timedelta(seconds=2),
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tool_id, "content": output, "is_error": False}]},
                {"toolUseResult": {"stdout": output, "stderr": "", "interrupted": False}, "sourceToolAssistantUUID": parent},
            )
    write_jsonl(path, rows)
    return path


# --------------------------------------------------------------------------- Codex


def codex_rows(s: Session) -> list:
    model = s.model or "gpt-5.5-codex"
    rows = []
    ordinal = 0

    def add(ts: datetime, kind: str, payload: dict):
        nonlocal ordinal
        rows.append({"timestamp": iso(ts), "type": kind, "payload": payload, "ordinal": ordinal})
        ordinal += 1

    add(s.start, "session_meta", {
        "id": s.session_id,
        "session_id": s.session_id,
        "timestamp": iso(s.start),
        "cwd": s.cwd,
        "originator": "codex-tui",
        "cli_version": "0.164.0",
        "source": "vscode",
        "thread_source": "user",
        "model_provider": "openai",
        "history_mode": "full",
        "context_window": 272000,
    })
    add(s.start, "response_item", {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "<permissions instructions>\nFilesystem sandboxing: workspace-write. Approval policy: on-request.\n</permissions instructions>"}]})
    add(s.start, "response_item", {"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": f"# AGENTS.md instructions for {s.cwd}\n\n<INSTRUCTIONS>\nKeep diffs small. Run the tests before you finish.\n</INSTRUCTIONS>"},
        {"type": "input_text", "text": f"<environment_context>\n  <cwd>{s.cwd}</cwd>\n  <shell>zsh</shell>\n  <current_date>{s.start.date().isoformat()}</current_date>\n</environment_context>"},
    ]})
    add(s.start, "turn_context", {"cwd": s.cwd, "model": model, "approval_policy": "on-request", "sandbox_policy": {"type": "workspace-write"}, "effort": "medium"})
    for step, ts in step_times(s):
        kind = step[0]
        item_id = uuid.uuid4().hex[:16]
        if kind == "user":
            add(ts, "event_msg", {"type": "task_started", "model_context_window": 272000})
            add(ts, "response_item", {"type": "message", "role": "user", "content": [{"type": "input_text", "text": step[1]}]})
            add(ts, "event_msg", {"type": "item_completed", "item": {"type": "UserMessage", "id": item_id, "client_id": item_id, "content": [{"type": "text", "text": step[1], "text_elements": []}]}})
        elif kind == "thinking":
            add(ts, "response_item", {"type": "reasoning", "summary": [{"type": "summary_text", "text": step[1]}], "content": None, "encrypted_content": "gAAAAABp" + "Q" * 64})
            add(ts, "event_msg", {"type": "item_completed", "item": {"type": "Reasoning", "id": item_id, "summary_text": [step[1]], "raw_content": []}})
        elif kind == "assistant":
            add(ts, "response_item", {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": step[1]}]})
            add(ts, "event_msg", {"type": "item_completed", "item": {"type": "AgentMessage", "id": item_id, "content": [{"type": "Text", "text": step[1]}], "phase": "final_answer"}})
            add(ts, "event_msg", {"type": "token_count", "info": {"total_token_usage": {"input_tokens": 31000, "output_tokens": 900}}})
        elif kind == "tool":
            _, tool, arg, output = step
            call_id = f"call_{uuid.uuid4().hex[:20]}"
            if tool == "edit":
                patch = f"*** Begin Patch\n*** Update File: {arg}\n@@\n{output}\n*** End Patch"
                add(ts, "response_item", {"type": "custom_tool_call", "status": "completed", "call_id": call_id, "name": "apply_patch", "input": patch})
                add(ts, "response_item", {"type": "custom_tool_call_output", "call_id": call_id, "output": json.dumps({"output": f"Success. Updated the following files:\nM {arg}\n", "metadata": {"exit_code": 0, "duration_seconds": 0.0}})})
                add(ts, "event_msg", {"type": "item_completed", "item": {"type": "FileChange", "id": item_id, "changes": {f"{s.cwd}/{arg}": {"type": "update", "unified_diff": output}}, "status": "completed", "stdout": "", "stderr": ""}})
            else:
                command = arg if tool == "shell" else f"sed -n '1,200p' {arg}"
                add(ts, "response_item", {"type": "function_call", "name": "shell_command", "arguments": json.dumps({"command": command, "workdir": s.cwd}), "call_id": call_id})
                add(ts, "response_item", {"type": "function_call_output", "call_id": call_id, "output": f"Exit code: 0\nWall time: 0.4 seconds\nOutput:\n{output}"})
                add(ts, "event_msg", {"type": "item_completed", "item": {"type": "CommandExecution", "id": item_id, "command": command, "cwd": s.cwd, "aggregated_output": output, "exit_code": 0, "status": "completed", "duration": {"secs": 0, "nanos": 400000000}, "parsed_cmd": [], "process_id": None, "source": "agent"}})
    last = [step[1] for step in s.steps if step[0] == "assistant"]
    add(s.start + timedelta(hours=1), "event_msg", {"type": "task_complete", "last_agent_message": last[-1] if last else None})
    return rows


def codex_filename(s: Session) -> str:
    return f"rollout-{s.start.astimezone(timezone.utc).strftime('%Y-%m-%dT%H-%M-%S')}-{s.session_id}.jsonl"


def zstd_compress(data: bytes) -> bytes:
    try:
        import zstandard  # type: ignore

        return zstandard.ZstdCompressor(level=3).compress(data)
    except ImportError:
        pass
    if shutil.which("zstd"):
        return subprocess.run(["zstd", "-q", "-c"], input=data, stdout=subprocess.PIPE, check=True).stdout
    raise SystemExit("archived Codex sessions need zstd: install the `zstd` CLI or `pip install zstandard`")


def write_codex(home: Path, s: Session, archived: bool = False) -> Path:
    rows = codex_rows(s)
    if archived:
        path = home / ".codex" / "archived_sessions" / (codex_filename(s) + ".zst")
        path.parent.mkdir(parents=True, exist_ok=True)
        payload = "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows).encode()
        path.write_bytes(zstd_compress(payload))
        return path
    day = s.start.astimezone(timezone.utc)
    path = home / ".codex" / "sessions" / f"{day:%Y}" / f"{day:%m}" / f"{day:%d}" / codex_filename(s)
    write_jsonl(path, rows)
    return path


# --------------------------------------------------------------------------- Cursor


def cursor_slug(cwd: str) -> str:
    return cwd.strip("/").replace("/", "-")


def cursor_stamp(ts: datetime) -> str:
    local = ts.astimezone(timezone.utc)
    return local.strftime("%A, %b %-d, %Y, %-I:%M %p (UTC+0)")


def write_cursor(home: Path, s: Session) -> Path:
    path = home / ".cursor" / "projects" / cursor_slug(s.cwd) / "agent-transcripts" / s.session_id / f"{s.session_id}.jsonl"
    rows = []
    pending_assistant = []

    def flush():
        if pending_assistant:
            rows.append({"role": "assistant", "message": {"content": list(pending_assistant)}})
            pending_assistant.clear()

    for step, ts in step_times(s):
        kind = step[0]
        if kind == "user":
            flush()
            if rows:
                rows.append({"type": "turn_ended", "status": "success"})
            text = f"<timestamp>{cursor_stamp(ts)}</timestamp>\n<user_query>\n{step[1]}\n</user_query>"
            rows.append({"role": "user", "message": {"content": [{"type": "text", "text": text}]}})
        elif kind == "assistant":
            pending_assistant.append({"type": "text", "text": step[1]})
            flush()
        elif kind == "thinking":
            continue
        elif kind == "tool":
            _, tool, arg, _output = step
            if tool == "shell":
                block = {"type": "tool_use", "name": "Shell", "input": {"command": arg, "working_directory": s.cwd}}
            elif tool == "read":
                block = {"type": "tool_use", "name": "Read", "input": {"target_file": f"{s.cwd}/{arg}"}}
            else:
                block = {"type": "tool_use", "name": "StrReplace", "input": {"file_path": f"{s.cwd}/{arg}", "old_string": "...", "new_string": "..."}}
            pending_assistant.append(block)
            flush()
    flush()
    rows.append({"type": "turn_ended", "status": "success"})
    write_jsonl(path, rows)
    return path


# --------------------------------------------------------------------------- Grok


def write_grok(home: Path, s: Session) -> Path:
    session_dir = home / ".grok" / "sessions" / urllib.parse.quote(s.cwd, safe="") / s.session_id
    rows = [{"timestamp": int(s.start.timestamp()), "method": "session/update", "params": {"sessionId": s.session_id, "update": {"sessionUpdate": "available_commands_update", "availableCommands": []}}}]
    count = 0
    for step, ts in step_times(s):
        kind = step[0]
        stamp = int(ts.timestamp())
        meta = {"agentTimestampMs": stamp * 1000}
        if kind == "user":
            count += 1
            rows.append({"timestamp": stamp, "method": "session/update", "params": {"sessionId": s.session_id, "update": {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": step[1]}, "_meta": {"modelId": "grok-build"}}, "_meta": meta}})
        elif kind == "assistant":
            count += 1
            # Grok streams the reply in chunks.
            text = step[1]
            for offset in range(0, len(text), 240):
                rows.append({"timestamp": stamp, "method": "session/update", "params": {"sessionId": s.session_id, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text[offset:offset + 240]}}, "_meta": meta}})
        elif kind == "thinking":
            rows.append({"timestamp": stamp, "method": "session/update", "params": {"sessionId": s.session_id, "update": {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": step[1]}}, "_meta": meta}})
        elif kind == "tool":
            _, tool, arg, output = step
            call_id = f"call_{uuid.uuid4().hex[:12]}"
            rows.append({"timestamp": stamp, "method": "session/update", "params": {"sessionId": s.session_id, "update": {"sessionUpdate": "tool_call", "toolCallId": call_id, "title": arg if tool == "shell" else f"{tool} {arg}", "kind": "execute" if tool == "shell" else tool, "status": "pending"}, "_meta": meta}})
            rows.append({"timestamp": stamp + 1, "method": "session/update", "params": {"sessionId": s.session_id, "update": {"sessionUpdate": "tool_call_update", "toolCallId": call_id, "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": output}}]}, "_meta": meta}})
    write_jsonl(session_dir / "updates.jsonl", rows)
    end = s.start + timedelta(seconds=60 * len(s.steps))
    summary = {
        "info": {"id": s.session_id, "cwd": s.cwd},
        "session_summary": next((step[1][:60] for step in s.steps if step[0] == "user"), ""),
        "created_at": iso(s.start),
        "updated_at": iso(end),
        "current_model_id": "grok-build",
        "num_messages": count,
    }
    (session_dir / "summary.json").write_text(json.dumps(summary), encoding="utf-8")
    return session_dir


# --------------------------------------------------------------------------- Pi


def write_pi(home: Path, s: Session) -> Path:
    slug = "--" + s.cwd.strip("/").replace("/", "-") + "--"
    stamp = s.start.astimezone(timezone.utc).strftime("%Y-%m-%dT%H-%M-%S-%f")[:-3] + "Z"
    path = home / ".pi" / "agent" / "sessions" / slug / f"{stamp}_{s.session_id}.jsonl"
    model = s.model or "gpt-5.5"
    rows = [
        {"type": "session", "version": 3, "id": s.session_id, "timestamp": iso(s.start), "cwd": s.cwd},
        {"type": "model_change", "id": "model-1", "parentId": None, "timestamp": iso(s.start), "provider": "openai-codex", "modelId": model},
    ]
    parent = "model-1"
    pending = []

    def emit(role: str, content: list, ts: datetime, extra: dict | None = None):
        nonlocal parent
        row_id = uuid.uuid4().hex[:8]
        message = {"role": role, "content": content, "timestamp": int(ts.timestamp() * 1000)}
        if extra:
            message.update(extra)
        rows.append({"type": "message", "id": row_id, "parentId": parent, "timestamp": iso(ts), "message": message})
        parent = row_id

    for step, ts in step_times(s):
        kind = step[0]
        if kind == "user":
            emit("user", [{"type": "text", "text": step[1]}], ts)
        elif kind == "thinking":
            pending.append({"type": "thinking", "thinking": step[1]})
        elif kind == "assistant":
            emit("assistant", pending + [{"type": "text", "text": step[1]}], ts, {"model": model, "usage": {"input": 9000, "output": 400}})
            pending = []
        elif kind == "tool":
            _, tool, arg, output = step
            call_id = f"call-{uuid.uuid4().hex[:8]}"
            name = {"shell": "bash", "read": "read", "edit": "edit"}[tool]
            arguments = {"command": arg} if tool == "shell" else {"path": arg}
            emit("assistant", pending + [{"type": "toolCall", "id": call_id, "name": name, "arguments": arguments}], ts, {"model": model, "usage": {"input": 9000, "output": 120}})
            pending = []
            emit("toolResult", [{"type": "text", "text": output}], ts + timedelta(seconds=2), {"toolCallId": call_id, "toolName": name, "isError": False})
    write_jsonl(path, rows)
    return path


def write_session(home: Path, s: Session) -> Path:
    if s.harness == "claude":
        return write_claude(home, s)
    if s.harness == "codex":
        return write_codex(home, s)
    if s.harness == "codex-archived":
        return write_codex(home, s, archived=True)
    if s.harness == "cursor":
        return write_cursor(home, s)
    if s.harness == "grok":
        return write_grok(home, s)
    if s.harness == "pi":
        return write_pi(home, s)
    raise ValueError(s.harness)
