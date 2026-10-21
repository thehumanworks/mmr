#!/usr/bin/env python3
"""Measure only generated public-style fixtures; never discover or import live history."""
import argparse
import hashlib
import json
import os
import pathlib
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--events", default="200,2000", help="Comma-separated generated message counts")
    parser.add_argument("--repeats", default=3, type=int)
    args = parser.parse_args()
    binary = args.binary.resolve()
    counts = [int(value) for value in args.events.split(",")]
    if not 1 <= args.repeats <= 100 or not counts or any(not 1 <= n < 40000 for n in counts):
        parser.error("use 1–100 repeats and message counts 1–39999")
    results = []
    with tempfile.TemporaryDirectory(prefix="mmr-conversion-benchmark-") as directory:
        root = pathlib.Path(directory)
        home = root / "synthetic-home"
        home.mkdir()
        env = {"HOME": str(home), "PATH": "/usr/bin:/bin"}
        for count in counts:
            rows = [{"type": "session_meta", "payload": {"id": "fixture-session", "branch_id": "fixture-branch"}}]
            rows.extend({"type": "response_item", "timestamp": "2026-10-21T00:00:00Z", "payload": {
                "id": f"message-{index}", "type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": f"Synthetic coding handoff message {index}: preserve provenance and ordering."}
                ]}, "future_metadata": {"retain": True}} for index in range(count))
            raw = "\n".join(json.dumps(row, separators=(",", ":")) for row in rows) + "\n"
            source = root / "fixture.jsonl"
            source.write_text(raw)
            canonical = root / "canonical.json"
            digest = "sha256:" + hashlib.sha256(raw.encode()).hexdigest()
            for label, input_file, source_format, target in [
                ("native_to_canonical", source, "codex", "canonical"),
                ("canonical_restart_to_claude_readable", canonical, "canonical", "claude-readable"),
            ]:
                samples = []
                baseline = None
                for _ in range(args.repeats):
                    command = [str(binary), "convert", "session", str(input_file), "--from", source_format, "--to", target]
                    if source_format != "canonical":
                        command += ["--origin", "synthetic-benchmark-host"]
                    started = time.perf_counter()
                    run = subprocess.run(command, env=env, cwd=home, capture_output=True, timeout=60, check=True)
                    samples.append(round((time.perf_counter() - started) * 1000, 3))
                    response = json.loads(run.stdout)
                    assert response["canonical"]["archive"] == {"sha256": digest, "text": raw}
                    assert len(response["canonical"]["events"]) == count + 1
                    assert not response["capabilities"]["native_resume"] and not response["capabilities"]["api_replay"]
                    if baseline is not None:
                        assert baseline == run.stdout, "process restart changed conversion output"
                    baseline = run.stdout
                if source_format == "codex":
                    canonical.write_bytes(baseline)
                results.append({"workload": label, "message_count": count, "input_bytes": input_file.stat().st_size,
                    "output_bytes": len(baseline), "elapsed_ms": samples, "median_ms": statistics.median(samples),
                    "archive_sha256": digest, "deterministic_restart": True})
        print(json.dumps({"binary": str(binary), "platform": os.uname().sysname + " " + os.uname().machine,
            "scope": "Synthetic text-only messages; separate-process wall time including stdout transfer. No retrieval/quality/speedup claim.",
            "results": results}, indent=2))


if __name__ == "__main__":
    main()
