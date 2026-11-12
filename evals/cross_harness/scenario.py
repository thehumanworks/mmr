"""The ledger-svc scenario: a repo, three weeks of multi-harness history, tasks.

Every key fact lives in exactly one earlier session and is absent from the repo
files and the git log, which hold *what* changed but not *why*. `FACTS` maps
each fact to the session that holds it; `run.py --selfcheck` verifies that.
"""

from __future__ import annotations

from datetime import datetime, timedelta

from writers import Session

PROJECT = "src/ledger-svc"  # relative to HOME

STAGING_URL = "https://hooks-staging.acme-partner.test/v2/ledger-inbox"

# --------------------------------------------------------------------------- repo

DELIVERY_V1 = '''"""Deliver ledger events to partner webhooks."""

from __future__ import annotations

import time
from dataclasses import dataclass

import httpx

from ledger.store import Event, Outbox

MAX_RETRIES = 5
TIMEOUT_S = 10
BACKOFF_BASE_S = 2


@dataclass
class DeliveryResult:
    ok: bool
    attempts: int
    status: int | None = None


def deliver(event: Event, endpoint: str, *, timeout: float | None = None) -> DeliveryResult:
    attempts = 0
    while True:
        attempts += 1
        try:
            response = httpx.post(endpoint, json=event.payload, timeout=timeout or TIMEOUT_S)
            if response.status_code < 500:
                return DeliveryResult(ok=response.is_success, attempts=attempts, status=response.status_code)
        except httpx.TimeoutException:
            pass
        if attempts > MAX_RETRIES:
            return DeliveryResult(ok=False, attempts=attempts)
        time.sleep(BACKOFF_BASE_S * 2 ** (attempts - 1))


def drain(outbox: Outbox, endpoint: str) -> int:
    sent = 0
    for event in outbox.pending():
        result = deliver(event, endpoint)
        outbox.mark(event.id, delivered=result.ok, attempts=result.attempts)
        sent += result.ok
    return sent
'''

DELIVERY_V2 = DELIVERY_V1.replace("MAX_RETRIES = 5", "MAX_RETRIES = 3").replace("TIMEOUT_S = 10", "TIMEOUT_S = 15")

DELIVERY_V3 = DELIVERY_V2.replace(
    "import time\n", "import random\nimport time\n"
).replace(
    "        time.sleep(BACKOFF_BASE_S * 2 ** (attempts - 1))\n",
    "        time.sleep(backoff_delay(attempts))\n",
).replace(
    "def deliver(",
    "def backoff_delay(attempt: int) -> float:\n    return random.uniform(0, BACKOFF_BASE_S * 2 ** (attempt - 1))\n\n\ndef deliver(",
)

DELIVERY_V4 = DELIVERY_V3.replace(
    "from ledger.store import Event, Outbox\n",
    "from ledger.log import log\nfrom ledger.store import Event, Outbox\n",
).replace(
    "        outbox.mark(event.id, delivered=result.ok, attempts=result.attempts)\n",
    "        outbox.mark(event.id, delivered=result.ok, attempts=result.attempts)\n        log.info(\"delivery\", event_id=event.id, ok=result.ok, attempts=result.attempts, status=result.status)\n",
)

# Uncommitted work left by the most recent session.
DELIVERY_WORKTREE = DELIVERY_V4.replace(
    "            response = httpx.post(endpoint, json=event.payload, timeout=timeout or TIMEOUT_S)\n",
    "            headers = {\"Idempotency-Key\": event.id}\n"
    "            if outbox_seen(event.id):\n"
    "                return DeliveryResult(ok=True, attempts=0)\n"
    "            response = httpx.post(endpoint, json=event.payload, headers=headers, timeout=timeout or TIMEOUT_S)\n",
).replace(
    "def backoff_delay(",
    "def outbox_seen(event_id: str) -> bool:\n    from ledger.store import connect\n\n    with connect() as db:\n        row = db.execute(\"SELECT 1 FROM dedupe_keys WHERE key = ?\", (event_id,)).fetchone()\n    return row is not None\n\n\ndef backoff_delay(",
)

STORE = '''"""SQLite outbox for ledger events."""

from __future__ import annotations

import json
import os
import sqlite3
from contextlib import contextmanager
from dataclasses import dataclass

DB_PATH = os.environ.get("LEDGER_DB", "ledger.db")


@dataclass
class Event:
    id: str
    payload: dict


@contextmanager
def connect():
    db = sqlite3.connect(DB_PATH)
    db.execute("PRAGMA journal_mode=WAL")
    try:
        yield db
        db.commit()
    finally:
        db.close()


class Outbox:
    def pending(self):
        with connect() as db:
            rows = db.execute("SELECT id, payload FROM outbox WHERE delivered = 0 ORDER BY created_at").fetchall()
        return [Event(id=row[0], payload=json.loads(row[1])) for row in rows]

    def mark(self, event_id: str, *, delivered: bool, attempts: int) -> None:
        with connect() as db:
            db.execute(
                "UPDATE outbox SET delivered = ?, attempts = attempts + ? WHERE id = ?",
                (int(delivered), attempts, event_id),
            )
'''

LOG = '''"""Structured logging."""

import structlog

log = structlog.get_logger("ledger")
'''

TEST_DELIVERY = '''from ledger.delivery import MAX_RETRIES, backoff_delay, deliver


def test_backoff_is_bounded():
    for attempt in range(1, MAX_RETRIES + 1):
        assert 0 <= backoff_delay(attempt) <= 2 * 2 ** (attempt - 1)


def test_gives_up_after_max_retries(httpx_mock):
    httpx_mock.add_exception(TimeoutError(), is_reusable=True)
    result = deliver(make_event(), "https://partner.invalid/hook")
    assert not result.ok
    assert result.attempts == MAX_RETRIES + 1
'''

TEST_DELIVERY_WORKTREE = TEST_DELIVERY + '''

def test_duplicate_delivery_is_ignored(httpx_mock, tmp_db):
    event = make_event()
    httpx_mock.add_response(status_code=200)
    assert deliver(event, "https://partner.invalid/hook").ok
    second = deliver(event, "https://partner.invalid/hook")
    assert second.attempts == 0
'''

MIGRATIONS = {
    "0001_outbox.sql": "CREATE TABLE outbox (id TEXT PRIMARY KEY, payload TEXT NOT NULL, created_at TEXT NOT NULL, delivered INTEGER NOT NULL DEFAULT 0);\n",
    "0002_attempts.sql": "ALTER TABLE outbox ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;\n",
    "0003_endpoint.sql": "ALTER TABLE outbox ADD COLUMN endpoint TEXT;\n",
    "0004_outbox_pending_index.sql": "CREATE INDEX outbox_pending ON outbox (delivered, created_at);\n",
    "0005_dead_letters.sql": "CREATE TABLE dead_letters (id TEXT PRIMARY KEY, payload TEXT NOT NULL, failed_at TEXT NOT NULL);\n",
    "0006_delivery_attempts_index.sql": "CREATE INDEX outbox_attempts ON outbox (attempts);\n",
}

REPLAY = '''"""Replay dead letters: python scripts/replay.py --endpoint URL --since 2h"""

import argparse

from ledger.delivery import deliver
from ledger.store import Event, connect


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--since", default="1h")
    args = parser.parse_args()
    with connect() as db:
        rows = db.execute("SELECT id, payload FROM dead_letters").fetchall()
    for row in rows:
        print(row[0], deliver(Event(id=row[0], payload=row[1]), args.endpoint))


if __name__ == "__main__":
    main()
'''

README = """# ledger-svc

Delivers ledger events from the outbox to partner webhooks.

## Development

    uv sync
    uv run pytest

Migrations live in `migrations/` and run in filename order.
"""

PYPROJECT = """[project]
name = "ledger-svc"
version = "0.4.0"
requires-python = ">=3.12"
dependencies = ["httpx>=0.27", "structlog>=24.1"]

[dependency-groups]
dev = ["pytest>=8", "pytest-httpx>=0.30", "ruff>=0.7"]
"""

BASE_FILES = {
    "README.md": README,
    "pyproject.toml": PYPROJECT,
    "ledger/__init__.py": "",
    "ledger/store.py": STORE,
    "scripts/replay.py": REPLAY,
    "tests/test_store.py": "from ledger.store import Event\n\n\ndef test_event_roundtrip():\n    assert Event(id=\"e1\", payload={}).id == \"e1\"\n",
    **{f"migrations/{name}": body for name, body in MIGRATIONS.items()},
}

# (days ago, message, files to write)
COMMITS = [
    (24, "initial scaffold", {**BASE_FILES, "ledger/delivery.py": DELIVERY_V1, "tests/test_delivery.py": TEST_DELIVERY.replace("backoff_delay, ", "").split("\n\ndef test_backoff")[0] + "\n"}),
    (20, "retry tweaks", {"ledger/delivery.py": DELIVERY_V2}),
    (12, "refactor backoff", {"ledger/delivery.py": DELIVERY_V3, "tests/test_delivery.py": TEST_DELIVERY}),
    (9, "structured logging", {"ledger/delivery.py": DELIVERY_V4, "ledger/log.py": LOG}),
    (2, "lint", {"pyproject.toml": PYPROJECT.replace("ruff>=0.7", "ruff>=0.8")}),
]

WORKTREE_CHANGES = {"ledger/delivery.py": DELIVERY_WORKTREE, "tests/test_delivery.py": TEST_DELIVERY_WORKTREE}

# --------------------------------------------------------------------------- noise helpers


def pytest_output(passed: int, failed: list[tuple[str, str]] | None = None) -> str:
    lines = ["============================= test session starts ==============================", "platform linux -- Python 3.12.7, pytest-8.3.3, pluggy-1.5.0", "rootdir: /work", "configfile: pyproject.toml", "plugins: httpx-0.33.0", f"collected {passed + len(failed or [])} items", ""]
    lines.append("tests/test_store.py " + "." * min(passed, 9) + " " * 30 + "[ 40%]")
    lines.append("tests/test_delivery.py " + "." * max(passed - 9, 0) + ("F" * len(failed or [])) + " " * 20 + "[100%]")
    for name, error in failed or []:
        lines += ["", "=================================== FAILURES ===================================", f"_______________________ {name} _______________________", "", error, ""]
    summary = f"{len(failed)} failed, {passed} passed" if failed else f"{passed} passed"
    lines.append(f"========================= {summary} in 0.{passed + 21}s =========================")
    return "\n".join(lines)


def log_lines(count: int, seed: str, endpoint: str = "<redacted>") -> str:
    out = []
    for i in range(count):
        status = 202 if (i * 7 + len(seed)) % 25 else 504
        out.append(f'{{"event":"delivery","level":"info","event_id":"evt_{seed}{i:04d}","endpoint":"{endpoint}","attempt":1,"status":{status},"elapsed_ms":{180 + (i * 37) % 900}}}')
    return "\n".join(out)


def ruff_output(count: int) -> str:
    codes = ["E501 Line too long (104 > 100)", "I001 Import block is un-sorted or un-formatted", "UP007 Use `X | Y` for type annotations", "F401 `os` imported but unused", "B008 Do not perform function call in argument defaults"]
    files = ["ledger/delivery.py", "ledger/store.py", "scripts/replay.py", "tests/test_delivery.py"]
    lines = [f"{files[i % 4]}:{10 + i * 3}:{1 + i % 9}: {codes[i % 5]}" for i in range(count)]
    return "\n".join(lines) + f"\nFound {count} errors.\n[*] {count - 3} fixable with the `--fix` option."


# --------------------------------------------------------------------------- sessions


def sessions(home: str, now: datetime) -> list[Session]:
    proj = f"{home}/{PROJECT}"
    ago = lambda days, hour=10: (now - timedelta(days=days)).replace(hour=hour, minute=12, second=0, microsecond=0)  # noqa: E731
    out: list[Session] = []

    # S1 · Codex, archived (.jsonl.zst) · scaffold with 5 retries
    out.append(Session("scaffold", "codex-archived", proj, ago(24), [
        ("user", "Set up a small Python service that delivers ledger events from an outbox to partner webhooks. SQLite for the outbox, httpx for HTTP, exponential backoff on failures."),
        ("thinking", "Plan the package layout, outbox schema, delivery loop."),
        ("tool", "shell", "ls -la", "total 8\ndrwxr-xr-x  3 dev dev 4096 .\ndrwxr-xr-x 12 dev dev 4096 ..\ndrwxr-xr-x  7 dev dev 4096 .git"),
        ("tool", "edit", "ledger/store.py", STORE),
        ("tool", "edit", "ledger/delivery.py", DELIVERY_V1),
        ("tool", "edit", "migrations/0001_outbox.sql", MIGRATIONS["0001_outbox.sql"]),
        ("tool", "shell", "uv run pytest -q", pytest_output(2)),
        ("assistant", "Done. Layout: `ledger/store.py` (SQLite outbox in WAL mode), `ledger/delivery.py` (delivery loop), `migrations/0001_outbox.sql`.\n\nDelivery policy: up to **5 retries** after the first attempt, exponential backoff starting at 2s (2, 4, 8, 16, 32s), 10s request timeout. 5xx and timeouts retry; 4xx is final. Tests pass."),
        ("user", "Good. Commit it as 'initial scaffold'."),
        ("tool", "shell", "git add -A && git commit -m 'initial scaffold'", "[main (root-commit) 3f2a1c9] initial scaffold\n 14 files changed, 212 insertions(+)"),
        ("assistant", "Committed as 3f2a1c9."),
    ]))

    # N2 · Claude · an old prototype with the same directory basename
    out.append(Session("prototype", "claude", f"{home}/scratch/ledger-svc", ago(40), [
        ("user", "quick prototype: webhook sender with redis dedupe, retry up to 10 times"),
        ("tool", "shell", "pip install redis httpx", "Successfully installed httpx-0.27.2 redis-5.2.0"),
        ("tool", "edit", "sender.py", "MAX_RETRIES = 10\n..."),
        ("assistant", "Prototype ready in `sender.py`: retries up to 10 times, dedupes with Redis `SETNX` (24h TTL). Redis works well for dedupe here since it is already running locally."),
    ]))

    # S2 · Claude · duplicate-charge incident → 3 retries, 15s timeout
    out.append(Session("incident", "claude", proj, ago(20), [
        ("user", "billing-svc reported duplicate charges from Acme yesterday. Can you look at our retry logic?"),
        ("tool", "read", "ledger/delivery.py", DELIVERY_V1),
        ("tool", "shell", "grep -rn 'acme' ../billing-svc/logs/2026-*.log | head -40", log_lines(40, "acme", "acme")),
        ("thinking", "Acme responses take 11-14s. Our timeout is 10s, so we retry while Acme is still processing."),
        ("assistant", "Found it. Acme's endpoint regularly takes 11-14s to respond, and our timeout is 10s. Acme keeps processing the request after we give up, so every timeout-then-retry creates another charge on their side. With 5 retries one event could be charged up to 6 times."),
        ("user", "Ugh. OK: cut retries to 3 and raise the timeout to 15s for now. We'll do proper idempotency keys later, once we know what Acme supports."),
        ("tool", "edit", "ledger/delivery.py", "-MAX_RETRIES = 5\n-TIMEOUT_S = 10\n+MAX_RETRIES = 3\n+TIMEOUT_S = 15"),
        ("tool", "shell", "uv run pytest -q", pytest_output(4)),
        ("assistant", "Changed `MAX_RETRIES` 5 → 3 and `TIMEOUT_S` 10 → 15. Reason, for the record: Acme processes requests even after we time out, so retries duplicated charges. 15s covers Acme's normal latency; 3 retries bounds the damage until idempotency keys land. Tests pass."),
        ("user", "commit as 'retry tweaks'"),
        ("tool", "shell", "git commit -am 'retry tweaks'", "[main 8b41d07] retry tweaks\n 1 file changed, 2 insertions(+), 2 deletions(-)"),
        ("assistant", "Committed 8b41d07."),
    ]))

    # S3 · Claude worktree · Redis tried and rejected
    out.append(Session("redis", "claude", f"{proj}/.claude/worktrees/dedupe-redis", ago(16), [
        ("user", "Let's prototype the dedupe store for idempotency. Try Redis: SETNX on the event id with a 24h TTL."),
        ("tool", "shell", "git worktree list", f"{proj}  8b41d07 [main]\n{proj}/.claude/worktrees/dedupe-redis  8b41d07 [dedupe-redis]"),
        ("tool", "edit", "ledger/dedupe.py", "import redis\n\n_r = redis.Redis()\n\ndef seen(key):\n    return not _r.set(key, 1, nx=True, ex=86400)\n"),
        ("tool", "shell", "uv add redis && uv run pytest -q", "Resolved 9 packages in 210ms\n" + pytest_output(5)),
        ("assistant", "Prototype works: `ledger/dedupe.py` uses `SET key 1 NX EX 86400`. It adds `redis` as a runtime dependency and needs a Redis server wherever ledger-svc runs."),
        ("user", "Scrap it. Ops confirmed the on-prem installs (the Hetzner boxes) don't run Redis and they won't add it just for us. Keep dedupe in SQLite instead, a dedupe_keys table next to the outbox."),
        ("tool", "shell", "git checkout main -- . && git status --short", ""),
        ("assistant", "Reverted the Redis prototype; nothing was committed to main. Decision recorded: no Redis (on-prem installs don't run it). Dedupe will be a `dedupe_keys` table in the existing SQLite database."),
    ]))

    # S4 · Cursor · refactor backoff, add full jitter (thundering herd)
    out.append(Session("jitter", "cursor", proj, ago(12), [
        ("user", "After Acme's outage on Monday all our queued retries hit them at the same second when they came back, a thundering herd. Split the backoff calculation out of deliver() and add full jitter."),
        ("tool", "read", "ledger/delivery.py", DELIVERY_V2),
        ("tool", "edit", "ledger/delivery.py", "+def backoff_delay(attempt: int) -> float:\n+    return random.uniform(0, BACKOFF_BASE_S * 2 ** (attempt - 1))"),
        ("tool", "shell", "uv run pytest -q", pytest_output(5)),
        ("assistant", "Added `backoff_delay(attempt)` with full jitter: `random.uniform(0, 2 * 2**(attempt-1))`. Retries after an outage now spread over the whole window instead of arriving together. Added `test_backoff_is_bounded`."),
        ("user", "commit as 'refactor backoff'"),
        ("tool", "shell", "git commit -am 'refactor backoff'", "[main c19e2aa] refactor backoff"),
        ("assistant", "Committed c19e2aa."),
    ]))

    # S5 · Codex (current format) · structured logging; user's constraints
    out.append(Session("constraints", "codex", proj, ago(9), [
        ("user", "Add structured logging to the delivery loop with structlog. Two rules for delivery.py from now on: don't change the signature of deliver(event, endpoint, *, timeout=None), because billing-svc imports it directly and pins to it. And no new runtime dependencies without asking me first. structlog is fine, I'm approving that one."),
        ("tool", "read", "ledger/delivery.py", DELIVERY_V3),
        ("tool", "edit", "ledger/log.py", LOG),
        ("tool", "edit", "ledger/delivery.py", "+        log.info(\"delivery\", event_id=event.id, ok=result.ok, attempts=result.attempts, status=result.status)"),
        ("tool", "shell", "uv add structlog && uv run pytest -q", "Resolved 10 packages in 190ms\n" + pytest_output(5)),
        ("assistant", "Added `ledger/log.py` and a `delivery` log event per outbox item (event_id, ok, attempts, status). `deliver()` is unchanged and structlog is the only new dependency. Tests pass."),
        ("user", "commit as 'structured logging'"),
        ("tool", "shell", "git commit -am 'structured logging'", "[main 5d0e3b1] structured logging"),
        ("assistant", "Committed 5d0e3b1."),
    ]))

    # S6 · Grok · from scripts/ · manual staging test (needle: URL)
    out.append(Session("staging", "grok", f"{proj}/scripts", ago(8), [
        ("user", f"Help me test delivery against Acme's staging endpoint by hand. The URL is {STAGING_URL} and the token is in ACME_STAGING_TOKEN. Send 50 synthetic events."),
        ("tool", "shell", "python make_events.py 50 > /tmp/events.jsonl && wc -l /tmp/events.jsonl", "50 /tmp/events.jsonl"),
        ("tool", "shell", "python replay.py --endpoint \"$ACME_STAGING_URL\" --since 10m", log_lines(120, "stg")),
        ("assistant", "Staging run done: 48 of 50 accepted on the first attempt (202). 2 timed out at 15s and succeeded on retry. p50 latency 410ms, p99 12.8s. No duplicates on Acme's side according to their staging dashboard."),
        ("user", "nice, thanks"),
    ]))

    # N1 · Codex · ledger-ui (sibling project, similar words)
    out.append(Session("ui-retry", "codex", f"{home}/src/ledger-ui", ago(6), [
        ("user", "The ledger dashboard should retry failed fetches. Max 2 retries, and point the staging build at https://staging.ledger-ui.test."),
        ("tool", "edit", "src/api.ts", "const MAX_RETRIES = 2;"),
        ("assistant", "Added fetch retries (max 2, linear 500ms backoff) in `src/api.ts` and set the staging base URL to https://staging.ledger-ui.test."),
    ]))

    # N4 · Cursor · flaky store test (noise)
    out.append(Session("flaky", "cursor", proj, ago(7), [
        ("user", "test_store is flaky on CI, can you take a look"),
        ("tool", "shell", "for i in $(seq 1 20); do uv run pytest -q tests/test_store.py; done", "\n".join(pytest_output(1) for _ in range(6))),
        ("assistant", "I ran it 20 times locally without a failure. The CI failures are `database is locked`; WAL mode should prevent that, so CI is probably sharing one DB file between parallel jobs. Setting `LEDGER_DB` per job would fix it."),
    ]))

    # N3 · Claude · explain store.py (noise, long tool output)
    out.append(Session("explain", "claude", proj, ago(3), [
        ("user", "explain how the outbox works, I'm new to this code"),
        ("tool", "read", "ledger/store.py", STORE),
        ("tool", "read", "ledger/delivery.py", DELIVERY_V4),
        ("tool", "shell", "ls migrations && cat migrations/*.sql", "\n".join(MIGRATIONS) + "\n" + "".join(MIGRATIONS.values())),
        ("assistant", "Events are written to the `outbox` table. `drain()` reads pending rows oldest first, calls `deliver()` for each, and marks the row delivered with the attempt count. `deliver()` retries 5xx and timeouts with jittered exponential backoff. Rows that fail permanently go to `dead_letters`, which `scripts/replay.py` can resend."),
    ]))

    # N5 · Codex · lint (noise, long output)
    out.append(Session("lint", "codex", proj, ago(2), [
        ("user", "bump ruff and fix whatever it complains about. And note for whoever deploys this week: never deploy between 17:00 and 19:00 CET, that's Acme's settlement batch window and they drop webhooks during it."),
        ("tool", "shell", "uv add --dev 'ruff>=0.8' && uv run ruff check .", ruff_output(60)),
        ("tool", "shell", "uv run ruff check . --fix && uv run ruff format .", "Found 60 errors (57 fixed, 3 remaining).\n4 files reformatted"),
        ("tool", "shell", "git commit -am lint", "[main 0a7f9e2] lint"),
        ("assistant", "Bumped ruff to 0.8, auto-fixed 57 findings and the 3 remaining by hand. Committed as `lint`."),
    ]))

    # S7 · Pi · latest relevant session: idempotency keys, left mid-way
    out.append(Session("idempotency", "pi", proj, ago(1, hour=17), [
        ("user", "Let's start on idempotency keys for deliveries, like we planned after the Acme incident. Acme confirmed they honour an Idempotency-Key header."),
        ("tool", "read", "ledger/delivery.py", DELIVERY_V4),
        ("tool", "edit", "ledger/delivery.py", "+            headers = {\"Idempotency-Key\": event.id}"),
        ("tool", "edit", "tests/test_delivery.py", "+def test_duplicate_delivery_is_ignored(httpx_mock, tmp_db):"),
        ("tool", "shell", "uv run pytest -q", pytest_output(5, [("test_duplicate_delivery_is_ignored", "E       sqlite3.OperationalError: no such table: dedupe_keys")])),
        ("assistant", "The header is in, and `outbox_seen()` checks a `dedupe_keys` table before sending. `test_duplicate_delivery_is_ignored` fails because that table doesn't exist yet.\n\nNext steps:\n1. Add `migrations/0007_dedupe_keys.sql`: `CREATE TABLE dedupe_keys (key TEXT PRIMARY KEY, first_seen_at TEXT NOT NULL)`, and insert the key after a successful delivery.\n2. Re-run `uv run pytest -k duplicate`.\n3. Then a cleanup job that deletes keys older than 72 hours, which is how long Acme keeps idempotency keys."),
        ("user", "I have to stop here. Don't commit anything yet, we'll continue tomorrow."),
        ("assistant", "OK, leaving the changes uncommitted in the working tree."),
    ]))

    return out


# --------------------------------------------------------------------------- facts and tasks

# fact id -> (session key, phrase that must appear in that session's text and nowhere else)
FACTS = {
    "five-retries": ("scaffold", "up to **5 retries**"),
    "duplicate-charges": ("incident", "every timeout-then-retry creates another charge"),
    "redis-rejected": ("redis", "don't run Redis and they won't add it"),
    "thundering-herd": ("jitter", "thundering herd"),
    "billing-imports": ("constraints", "billing-svc imports it directly"),
    "staging-url": ("staging", STAGING_URL),
    "72-hours": ("idempotency", "older than 72 hours"),
    "deploy-window": ("lint", "settlement batch window"),
    "flaky-cause": ("flaky", "sharing one DB file"),
    "ui-retries": ("ui-retry", "Max 2 retries"),
}

TASKS = [
    {
        "id": "why-retries",
        "prompt": "Why is MAX_RETRIES 3 in ledger/delivery.py? I'm fairly sure it used to be higher.",
        "tests": "Claude Code session, main project path",
        "rubric": {"duplicate charges": r"duplicat", "partner keeps processing after timeout": r"time[sd]?[ -]?out", "names Acme or billing": r"acme|billing"},
    },
    {
        "id": "resume",
        "prompt": "Pick up where we left off. Before you change anything, tell me what the last session was working on, what state it left things in, and what you would do next.",
        "tests": "Pi session (newest before this one)",
        "rubric": {"idempotency/dedupe work": r"idempoten|dedup", "migration 0007 next": r"0007", "72-hour key cleanup": r"72"},
    },
    {
        "id": "constraints",
        "prompt": "I'm about to refactor ledger/delivery.py. Did I set any rules earlier that I should keep in mind?",
        "tests": "Codex session (current Codex format), user message",
        "rubric": {"keep deliver() signature": r"signature", "because billing-svc imports it": r"billing.{0,60}import|import.{0,60}billing", "no new dependencies without asking": r"depend"},
    },
    {
        "id": "redis",
        "prompt": "Would Redis be a good fit for the dedupe store? Have we looked at that before?",
        "tests": "Claude Code session run in a worktree under .claude/worktrees",
        "rubric": {"mentions Redis was tried": r"(tried|prototyp|scrap|reject|revert|ruled out|decided against|looked at)", "reason: on-prem/ops has no Redis": r"on-?prem|hetzner|\bops\b"},
    },
    {
        "id": "staging-url",
        "prompt": "What was the Acme staging endpoint we used for manual webhook testing?",
        "tests": "Grok session run from the scripts/ subdirectory; a detail mentioned once",
        "rubric": {"exact staging URL": r"hooks-staging\.acme-partner\.test/v2/ledger-inbox"},
    },
    {
        "id": "retry-history",
        "prompt": "How has the webhook retry policy changed over time, and why was each change made? Oldest first.",
        "tests": "synthesis across an archived Codex session, a Claude Code session and a Cursor session",
        # Only facts git cannot supply: the partner behind the cut and the outage behind the jitter.
        "rubric": {"started at 5 retries": r"\b5\b|five", "cut after Acme/billing duplicate charges": r"acme|billing", "jitter added": r"jitter", "jitter after Acme's outage": r"outage"},
    },
    # Held out: added after the skill text was written, to check it was not tuned to the tasks above.
    {
        "id": "deploy",
        "held_out": True,
        "prompt": "I'm about to deploy this. Anything I should watch out for?",
        "tests": "held out; Codex session (current format), aside in a user message of an unrelated task",
        "rubric": {"17:00-19:00 CET window": r"17[:.]?00|5 ?pm", "Acme settlement batch": r"settlement|batch"},
    },
    {
        "id": "flaky",
        "held_out": True,
        "prompt": "Are any of the tests known to be flaky?",
        "tests": "held out; Cursor session",
        "rubric": {
            "test_store": r"test_store",
            "cause: CI jobs share one DB file (database is locked)": r"database is locked|LEDGER_DB|(one|same|single|shared)\W+(\w+\W+){0,2}(db|database|sqlite)\W+file|parallel (ci )?jobs",
        },
    },
    {
        "id": "dashboard",
        "held_out": True,
        "prompt": "How many times does the ledger dashboard retry a failed fetch?",
        "tests": "held out; sibling project (ledger-ui), Codex session",
        "rubric": {
            "2 retries": r"(max(imum)?|up to|at most)\W{0,3}(of\W{0,3})?(2|two)\b|\b(2|two)\W{0,3}(retries|retry|times)\b|MAX_RETRIES\W{0,6}2\b",
            "names the ledger-ui project": r"ledger-ui",
        },
    },
]
