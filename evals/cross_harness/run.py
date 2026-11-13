#!/usr/bin/env python3
"""Cross-harness context evaluation for mmr.

A fresh agent (Claude Code, headless) starts in a project whose earlier work was
done by other harnesses, and answers a question whose answer exists only in
that history. See README.md in this directory.

    python3 evals/cross_harness/run.py --selfcheck
    python3 evals/cross_harness/run.py --label baseline --reps 2
    python3 evals/cross_harness/run.py --report evals/cross_harness/results/<label>
"""

from __future__ import annotations

import argparse
import concurrent.futures as futures
import json
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import scenario  # noqa: E402
from writers import write_session  # noqa: E402

REPO_ROOT = HERE.parent.parent
CONDITIONS = ["none", "path", "shipped-skill", "skill", "told", "mcp", "hook", "instructions"]
# The paragraph `mmr skill install` adds to each agent's global instructions file
# (~/.claude/CLAUDE.md, ~/.codex/AGENTS.md).
INSTRUCTIONS = (REPO_ROOT / "src" / "agent_instructions.md").read_text()
TOLD_PREFIX = (
    "Note: `mmr` is installed on this machine. It reads the history of earlier AI coding "
    "sessions (Claude Code, Codex, Cursor, Grok, Pi) on this machine.\n\n"
)
HISTORY_DIRS = re.compile(r"\.claude/projects/[^ ]*\.jsonl|\.claude/projects/?(\s|$|\*)|\.codex/(sessions|archived)|\.cursor/projects|\.grok/sessions|\.pi/agent")
PASS_ENV = ["ANTHROPIC_BASE_URL", "HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy", "NO_PROXY", "no_proxy",
            "NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
SYSTEM_PATH = "/usr/local/bin:/usr/bin:/bin"


# --------------------------------------------------------------------------- sandbox


def git(cwd: Path, *args: str, when: datetime | None = None) -> None:
    env = {"PATH": SYSTEM_PATH, "HOME": str(cwd), "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_AUTHOR_NAME": "Dev", "GIT_AUTHOR_EMAIL": "dev@example.test",
           "GIT_COMMITTER_NAME": "Dev", "GIT_COMMITTER_EMAIL": "dev@example.test"}
    if when:
        stamp = when.strftime("%Y-%m-%dT%H:%M:%S%z")
        env.update(GIT_AUTHOR_DATE=stamp, GIT_COMMITTER_DATE=stamp)
    subprocess.run(["git", *args], cwd=cwd, env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def write_files(root: Path, files: dict) -> None:
    for rel, body in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")


def build_world(home: Path, now: datetime) -> dict:
    """Create HOME with native histories and the project repo. Returns key -> native path."""
    project = home / scenario.PROJECT
    project.mkdir(parents=True)
    git(project, "init", "-q", "-b", "main")
    for days, message, files in scenario.COMMITS:
        write_files(project, files)
        git(project, "add", "-A")
        git(project, "commit", "-q", "-m", message, when=(now - timedelta(days=days)).replace(hour=11))
    write_files(project, scenario.WORKTREE_CHANGES)
    for sibling in ("src/ledger-ui", "scratch/ledger-svc", "src/infra"):
        (home / sibling).mkdir(parents=True, exist_ok=True)
    paths = {}
    for session in scenario.sessions(str(home), now):
        paths[session.key] = str(write_session(home, session))
    return paths


def mmr_skill_dir(mmr: Path, scratch: Path) -> Path:
    """Install the skill bundled in this mmr binary into a scratch HOME and return its directory."""
    if not (scratch / ".agents" / "skills" / "mmr").exists():
        scratch.mkdir(parents=True, exist_ok=True)
        subprocess.run([str(mmr), "skill", "install"], env={"HOME": str(scratch), "PATH": SYSTEM_PATH},
                       check=True, stdout=subprocess.DEVNULL)
    return scratch / ".agents" / "skills" / "mmr"


def setup_condition(condition: str, home: Path, mmr: Path, run_dir: Path) -> tuple[str, list[str], str]:
    """Return (PATH, extra claude args, prompt prefix)."""
    bin_dir = run_dir / "bin"
    bin_dir.mkdir()
    path = f"{bin_dir}:{Path(shutil.which('claude')).parent}:{SYSTEM_PATH}"
    mcp_config = run_dir / "mcp.json"
    servers = {}
    if condition != "none":
        shutil.copy2(mmr, bin_dir / "mmr")
    if condition == "shipped-skill":
        subprocess.run([str(bin_dir / "mmr"), "skill", "install"], env={"HOME": str(home), "PATH": path},
                       check=True, stdout=subprocess.DEVNULL)
    if condition in ("skill", "hook", "instructions"):
        shutil.copytree(mmr_skill_dir(mmr, run_dir.parent / "_skill"), home / ".claude" / "skills" / "mmr")
    if condition == "hook":
        # A SessionStart hook puts the project's recent sessions in front of the agent.
        command = ("echo 'Earlier agent sessions in this project, newest first (from mmr; "
                   "read one with: mmr read session <session_id>):'; mmr list sessions --limit 8")
        settings = {"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": command}]}]}}
        (home / ".claude").mkdir(exist_ok=True)
        (home / ".claude" / "settings.json").write_text(json.dumps(settings))
    if condition == "instructions":
        (home / ".claude").mkdir(exist_ok=True)
        (home / ".claude" / "CLAUDE.md").write_text(INSTRUCTIONS)
    if condition == "mcp":
        servers["mmr"] = {"command": str(bin_dir / "mmr"), "args": ["mcp", "--transport", "stdio"]}
    mcp_config.write_text(json.dumps({"mcpServers": servers}))
    args = ["--mcp-config", str(mcp_config), "--strict-mcp-config"]
    prefix = TOLD_PREFIX if condition == "told" else ""
    return path, args, prefix


# --------------------------------------------------------------------------- agent run


def run_agent(task: dict, condition: str, rep: int, args, out_dir: Path) -> dict:
    run_id = f"{condition}__{task['id']}__{rep}"
    run_dir = Path(args.work) / run_id
    if run_dir.exists():
        shutil.rmtree(run_dir)
    home = run_dir / "home"
    home.mkdir(parents=True)
    now = datetime.now(timezone.utc).astimezone()
    build_world(home, now)
    project = home / scenario.PROJECT
    path, extra, prefix = setup_condition(condition, home, Path(args.mmr).resolve(), run_dir)
    env = {k: os.environ[k] for k in PASS_ENV if k in os.environ}
    env.update(HOME=str(home), PATH=path, IS_SANDBOX="1", TERM="dumb", LANG="C.UTF-8")
    prompt = prefix + task["prompt"]
    cmd = ["claude", "-p", prompt, "--model", args.model, "--output-format", "stream-json", "--verbose",
           "--max-turns", str(args.max_turns), "--permission-mode", "bypassPermissions",
           "--disallowedTools", "WebFetch,WebSearch,Task,Agent", *extra]
    started = time.time()
    transcript_path = out_dir / "transcripts" / f"{run_id}.jsonl"
    transcript_path.parent.mkdir(parents=True, exist_ok=True)
    timed_out = False
    with transcript_path.open("w") as handle:
        try:
            subprocess.run(cmd, cwd=project, env=env, stdout=handle, stderr=subprocess.STDOUT, timeout=args.timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
    result = analyse(transcript_path, task)
    result.update(run_id=run_id, condition=condition, task=task["id"], rep=rep, model=args.model,
                  wall_s=round(time.time() - started, 1), timed_out=timed_out)
    if not args.keep:
        shutil.rmtree(run_dir, ignore_errors=True)
    return result


def analyse(transcript: Path, task: dict) -> dict:
    events = []
    for line in transcript.read_text(errors="replace").splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    calls, results = [], {}
    final = None
    for event in events:
        if event.get("type") == "assistant":
            for block in event.get("message", {}).get("content", []):
                if block.get("type") == "tool_use":
                    calls.append(block)
        elif event.get("type") == "user":
            content = event.get("message", {}).get("content", [])
            for block in content if isinstance(content, list) else []:
                if block.get("type") == "tool_result":
                    text = block.get("content")
                    if isinstance(text, list):
                        text = "\n".join(part.get("text", "") for part in text if isinstance(part, dict))
                    results[block.get("tool_use_id")] = {"text": text or "", "error": bool(block.get("is_error"))}
        elif event.get("type") == "result":
            final = event
    steps = []
    for call in calls:
        name = call.get("name", "")
        tool_input = call.get("input", {})
        detail = tool_input.get("command") or tool_input.get("file_path") or tool_input.get("pattern") or tool_input.get("skill") or json.dumps(tool_input)[:200]
        res = results.get(call.get("id"), {"text": "", "error": False})
        is_mmr = name.startswith("mcp__mmr__") or (name == "Bash" and re.search(r"(^|[\s;&|(/])mmr(\s|$)", detail or "") is not None)
        is_raw_history = bool(HISTORY_DIRS.search(detail or "")) and not is_mmr
        steps.append({
            "tool": name, "detail": (detail or "")[:400], "mmr": is_mmr, "raw_history": is_raw_history,
            "error": res["error"] or bool(re.search(r"^error:|Error: ", res["text"][:300], re.M)),
            "result_chars": len(res["text"]),
        })
    answer = (final or {}).get("result") or ""
    rubric = {name: bool(re.search(pattern, answer, re.I)) for name, pattern in task["rubric"].items()}
    usage = (final or {}).get("usage", {})
    tokens_in = usage.get("input_tokens", 0) + usage.get("cache_creation_input_tokens", 0) + usage.get("cache_read_input_tokens", 0)
    return {
        "passed": bool(final) and all(rubric.values()),
        "rubric": rubric,
        "score": sum(rubric.values()) / len(rubric),
        "answer": answer,
        "tool_calls": len(steps),
        "mmr_calls": sum(s["mmr"] for s in steps),
        "mmr_errors": sum(s["mmr"] and s["error"] for s in steps),
        "mmr_output_chars": sum(s["result_chars"] for s in steps if s["mmr"]),
        "raw_history_calls": sum(s["raw_history"] for s in steps),
        "skill_calls": sum(s["tool"] == "Skill" for s in steps),
        "steps": steps,
        "tokens_in": tokens_in,
        "tokens_out": usage.get("output_tokens", 0),
        "cost_usd": (final or {}).get("total_cost_usd", 0.0),
        "turns": (final or {}).get("num_turns", 0),
        "is_error": (final or {}).get("is_error", final is None),
    }


# --------------------------------------------------------------------------- self-check / probe


def mmr_json(mmr: Path, home: Path, cwd: Path, *argv: str) -> tuple[int, str]:
    proc = subprocess.run([str(mmr), *argv], cwd=cwd, env={"HOME": str(home), "PATH": SYSTEM_PATH},
                          capture_output=True, text=True, timeout=120)
    return proc.returncode, proc.stdout + proc.stderr


def selfcheck(mmr: Path, work: Path) -> dict:
    """Verify the corpus, then probe what mmr can see of it without any agent."""
    home = work / "selfcheck" / "home"
    if home.parent.exists():
        shutil.rmtree(home.parent)
    home.mkdir(parents=True)
    now = datetime.now(timezone.utc).astimezone()
    paths = build_world(home, now)
    project = home / scenario.PROJECT

    # 1. Each fact phrase is in exactly one native file and nowhere in the repo or git history.
    texts = {}
    for key, native in paths.items():
        p = Path(native)
        files = [p] if p.is_file() else sorted(x for x in p.rglob("*") if x.is_file())
        blob = b""
        for f in files:
            data = f.read_bytes()
            if f.suffix == ".zst":
                from writers import zstd_compress  # noqa: F401  (ensures zstd is available)
                try:
                    import zstandard  # type: ignore
                    data = zstandard.ZstdDecompressor().decompressobj().decompress(data)
                except ImportError:
                    data = subprocess.run(["zstd", "-dc"], input=data, stdout=subprocess.PIPE, check=True).stdout
            blob += data
        texts[key] = blob.decode("utf-8", "replace").replace("\\\"", "\"")
    git_text = subprocess.run(["git", "log", "-p", "--all"], cwd=project, capture_output=True, text=True).stdout
    repo_text = git_text + "".join(f.read_text(errors="replace") for f in project.rglob("*") if f.is_file() and ".git" not in f.parts)
    problems = []
    for fact, (key, phrase) in scenario.FACTS.items():
        holders = [k for k, t in texts.items() if phrase in t]
        if holders != [key]:
            problems.append(f"{fact}: phrase found in {holders}, expected [{key}]")
        if phrase in repo_text:
            problems.append(f"{fact}: phrase leaks into the repo or git history")

    # 2. What mmr sees for the project, from the project directory.
    probe = {}
    code, out = mmr_json(mmr, home, project, "list", "sessions", "--limit", "100")
    listed = out
    session_ids = {s.key: s.session_id for s in scenario.sessions(str(home), now)}
    probe["list sessions (cwd)"] = {k: (sid in listed) for k, sid in session_ids.items()}
    code, out = mmr_json(mmr, home, project, "read", "project", "--limit", "100000")
    probe["read project (cwd): fact visible"] = {fact: phrase.replace("**", "") in out.replace("**", "") for fact, (_, phrase) in scenario.FACTS.items()}
    probe["read project (cwd): output chars"] = len(out)
    code, out = mmr_json(mmr, home, project, "recall")
    probe["recall (cwd): selected"] = [k for k, sid in session_ids.items() if sid in out]
    code, out = mmr_json(mmr, home, project, "context", "project")
    probe["context project (cwd): fact visible"] = {fact: phrase.replace("**", "") in out.replace("**", "") for fact, (_, phrase) in scenario.FACTS.items()}
    probe["context project (cwd): output chars"] = len(out)
    code, out = mmr_json(mmr, home, project, "list", "sessions", "--all", "--limit", "100")
    probe["list sessions --all"] = {k: (sid in out) for k, sid in session_ids.items()}
    return {"problems": problems, "probe": probe, "native_paths": {k: str(Path(v).relative_to(home)) for k, v in paths.items()}}


# --------------------------------------------------------------------------- report


def load_results(out_dir: Path) -> list[dict]:
    path = out_dir / "results.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()] if path.exists() else []


def fmt_k(n: float) -> str:
    return f"{n / 1000:.0f}k" if n >= 1000 else f"{n:.0f}"


def report(out_dir: Path) -> str:
    results = load_results(out_dir)
    meta = json.loads((out_dir / "meta.json").read_text()) if (out_dir / "meta.json").exists() else {}
    tasks = [t["id"] for t in scenario.TASKS]
    conditions = [c for c in CONDITIONS if any(r["condition"] == c for r in results)]
    lines = [f"# Cross-harness context eval: {out_dir.name}", ""]
    if meta:
        lines += [f"- mmr: `{meta.get('mmr_version', '?')}` ({meta.get('mmr_commit', '?')})", f"- agent: Claude Code {meta.get('claude_version', '?')}, model `{meta.get('model')}`", f"- runs: {len(results)} ({meta.get('reps')} per cell), {meta.get('date')}", ""]
    lines += ["## Pass rate (all rubric items found in the final answer)", "", "| condition | " + " | ".join(tasks) + " | all |", "|---|" + "---|" * (len(tasks) + 1)]
    for c in conditions:
        row = [c]
        for t in tasks:
            cell = [r for r in results if r["condition"] == c and r["task"] == t]
            row.append(f"{sum(r['passed'] for r in cell)}/{len(cell)}" if cell else "–")
        allc = [r for r in results if r["condition"] == c]
        row.append(f"**{sum(r['passed'] for r in allc)}/{len(allc)}**")
        lines.append("| " + " | ".join(row) + " |")
    lines += ["", "## Cost per run (median over all tasks; mmr errors and raw history reads are totals)", "", "| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |", "|---|---|---|---|---|---|---|---|---|---|---|"]
    med = lambda xs: sorted(xs)[len(xs) // 2] if xs else 0  # noqa: E731
    for c in conditions:
        rs = [r for r in results if r["condition"] == c]
        lines.append("| " + " | ".join([
            c, f"{sum(r['mmr_calls'] > 0 for r in rs)}/{len(rs)}", str(med([r['tool_calls'] for r in rs])), str(med([r['mmr_calls'] for r in rs])),
            str(sum(r['mmr_errors'] for r in rs)), fmt_k(med([r['mmr_output_chars'] for r in rs])), str(sum(r['raw_history_calls'] for r in rs)),
            fmt_k(med([r['tokens_in'] for r in rs])), fmt_k(med([r['tokens_out'] for r in rs])), f"{med([r['cost_usd'] for r in rs]):.2f}", f"{med([r['wall_s'] for r in rs]):.0f}",
        ]) + " |")
    lines += ["", "## Rubric items missed", ""]
    for t in scenario.TASKS:
        misses = {}
        for r in results:
            if r["task"] == t["id"] and r["condition"] != "none":
                for item, ok in r["rubric"].items():
                    if not ok:
                        misses.setdefault(item, []).append(f"{r['condition']}#{r['rep']}")
        if misses:
            lines.append(f"- **{t['id']}** ({t['tests']}): " + "; ".join(f"{item}: {', '.join(who)}" for item, who in misses.items()))
    lines += ["", "## mmr commands used", ""]
    counts: dict[str, int] = {}
    for r in results:
        for s in r["steps"]:
            if s["mmr"]:
                d = s["detail"]
                if s["tool"].startswith("mcp__mmr__"):
                    key = s["tool"]
                else:
                    m = re.search(r"mmr((?:\s+--?[a-z-]+(?:\s+[a-z]+)?)*\s+[a-z]+(?:\s+[a-z]+)?)", d)
                    key = "mmr" + re.sub(r"\s+", " ", m.group(1)) if m else d[:40]
                    key = re.sub(r" (--[a-z-]+)( [a-z0-9/._-]+)?", r" \1", key)
                counts[key] = counts.get(key, 0) + 1
    for key, n in sorted(counts.items(), key=lambda kv: -kv[1])[:30]:
        lines.append(f"- `{key}`: {n}")
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------- main


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mmr", default=str(REPO_ROOT / "target" / "release" / "mmr"))
    parser.add_argument("--model", default="claude-sonnet-5-5")
    parser.add_argument("--label", default=datetime.now().strftime("%Y%m%d-%H%M"))
    parser.add_argument("--mmr-commit", help="commit the mmr binary was built from (default: this checkout's HEAD)")
    parser.add_argument("--conditions", default=",".join(CONDITIONS))
    parser.add_argument("--tasks", default=",".join(t["id"] for t in scenario.TASKS))
    parser.add_argument("--reps", type=int, default=2)
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--max-turns", type=int, default=40)
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--work", default=str(Path(os.environ.get("TMPDIR", "/tmp")) / "mmr-eval-work"))
    parser.add_argument("--results", default=str(HERE / "results"))
    parser.add_argument("--keep", action="store_true", help="keep sandboxes for inspection")
    parser.add_argument("--selfcheck", action="store_true", help="verify the corpus and probe mmr without an agent")
    parser.add_argument("--report", metavar="DIR", help="re-render the report for a results directory")
    parser.add_argument("--regrade", metavar="DIR", help="re-grade stored transcripts with the current rubrics, then re-render")
    args = parser.parse_args()

    if args.regrade:
        out = Path(args.regrade)
        tasks = {t["id"]: t for t in scenario.TASKS}
        regraded = []
        for r in load_results(out):
            fresh = analyse(out / "transcripts" / f"{r['run_id']}.jsonl", tasks[r["task"]])
            regraded.append({**r, **fresh})
        (out / "results.jsonl").write_text("".join(json.dumps(r) + "\n" for r in regraded))
        args.report = args.regrade
    if args.report:
        out = Path(args.report)
        (out / "report.md").write_text(report(out))
        print(out / "report.md")
        return
    mmr = Path(args.mmr).resolve()
    if args.selfcheck:
        result = selfcheck(mmr, Path(args.work))
        print(json.dumps(result, indent=2))
        sys.exit(1 if result["problems"] else 0)

    out_dir = Path(args.results) / args.label
    out_dir.mkdir(parents=True, exist_ok=True)
    check = selfcheck(mmr, Path(args.work))
    if check["problems"]:
        sys.exit("corpus self-check failed: " + "; ".join(check["problems"]))
    (out_dir / "probe.json").write_text(json.dumps(check, indent=2))
    version = subprocess.run([str(mmr), "--version"], capture_output=True, text=True).stdout.strip()
    commit = args.mmr_commit or subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=REPO_ROOT, capture_output=True, text=True).stdout.strip()
    claude_version = subprocess.run(["claude", "--version"], capture_output=True, text=True).stdout.strip()
    (out_dir / "meta.json").write_text(json.dumps({"mmr_version": version, "mmr_commit": commit, "claude_version": claude_version,
                                                   "model": args.model, "reps": args.reps, "date": datetime.now().isoformat(timespec="minutes")}, indent=2))
    shutil.rmtree(Path(args.work) / "_skill", ignore_errors=True)
    mmr_skill_dir(mmr, Path(args.work) / "_skill")  # once, before threads share it
    done = {r["run_id"] for r in load_results(out_dir)}
    tasks = [t for t in scenario.TASKS if t["id"] in args.tasks.split(",")]
    jobs = [(t, c, rep) for rep in range(1, args.reps + 1) for c in args.conditions.split(",") for t in tasks
            if f"{c}__{t['id']}__{rep}" not in done]
    print(f"{len(jobs)} runs to do ({len(done)} already recorded) -> {out_dir}", flush=True)
    with futures.ThreadPoolExecutor(max_workers=args.jobs) as pool, (out_dir / "results.jsonl").open("a") as sink:
        pending = {pool.submit(run_agent, t, c, rep, args, out_dir): (t["id"], c, rep) for t, c, rep in jobs}
        for fut in futures.as_completed(pending):
            try:
                r = fut.result()
            except Exception as exc:  # keep going; report the failure
                print(f"FAILED {pending[fut]}: {exc!r}", flush=True)
                continue
            sink.write(json.dumps(r) + "\n")
            sink.flush()
            print(f"{r['run_id']}: {'PASS' if r['passed'] else 'fail'} mmr_calls={r['mmr_calls']} tools={r['tool_calls']} ${r['cost_usd']:.2f} {r['wall_s']}s", flush=True)
    (out_dir / "report.md").write_text(report(out_dir))
    print(out_dir / "report.md")


if __name__ == "__main__":
    main()
