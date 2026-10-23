"""Review a proposed check and a judgement lesson; never execute mined code."""

import json
from pathlib import Path
from tempfile import TemporaryDirectory

from _synthetic import binary, native_snapshot, run, seed


def main():
    mmr = binary()
    with TemporaryDirectory(prefix="mmr-dream-example-") as temp:
        home = Path(temp) / "home"
        project = seed(home)
        native = home / ".codex/sessions/example.jsonl"
        with native.open("a", encoding="utf-8") as stream:
            for second, text in [(3, "Another incident confirms: keep the retry limit at three."),
                                 (4, "Choose the explanation wording for the audience.")]:
                stream.write(json.dumps({"type": "event_msg", "timestamp": f"2025-01-02T00:00:0{second}Z",
                                         "payload": {"type": "user_message", "message": text}}) + "\n")
        before = native_snapshot(home)
        run(mmr, home, project, "init")
        handoff = run(mmr, home, project, "assimilate", "project")
        events = handoff["evidence"]["events"]
        retry_refs = [e["evidence_ref"] for e in events if "retry limit" in e["content_text"]]
        judgment_refs = [e["evidence_ref"] for e in events if "wording" in e["content_text"]]
        assert len(retry_refs) == 2 and len(judgment_refs) == 1, events
        # Synthetic agent-authored proposal, not a model-generated or executed check.
        plan = {"observations": [
            {"kind": "workflow", "claim": "Keep the retry limit at three.", "confidence": 0.9,
             "scope": handoff["project_id"], "evidence_refs": retry_refs,
             "codification": {"state": "proposed", "target": "tests/retry_limit.py",
                              "expected_behavior": "Accept three attempts and reject a fourth.",
                              "validation": "Run tests/retry_limit.py with boundary cases three and four."}},
            {"kind": "judgment", "claim": "Adapt explanations to the audience.", "confidence": 0.9,
             "scope": handoff["project_id"], "evidence_refs": judgment_refs,
             "recommended_action": "Audience context needs human judgement; a fixed wording check would be misleading."},
        ]}
        proposal = home / "proposal.json"
        proposal.write_text(json.dumps(plan), encoding="utf-8")
        review = run(mmr, home, project, "assimilate", "project", "--review", str(proposal))
        lessons = review["reviewed_output"]["observations"]
        assert lessons[0]["codification"]["state"] == "proposed"
        assert lessons[0]["claim"] == plan["observations"][0]["claim"]
        assert lessons[0]["evidence_refs"] == retry_refs
        assert lessons[1]["recommended_action"] == plan["observations"][1]["recommended_action"]
        assert "codification" not in lessons[1]
        assert not (project / "tests/retry_limit.py").exists()
        assert native_snapshot(home) == before
        assert run(mmr, home, project, "assimilate", "project")["evidence"] == handoff["evidence"]
        print("Recurring lesson: retry cap, 2 supporting events retained.")
        print("Check: tests/retry_limit.py — proposed, not verified or installed.")
        print("Judgment lesson retained: adapt explanations to the audience.")
        print("No check executed or installed; evidence and native histories unchanged.")
    print("Temporary home removed.")


if __name__ == "__main__":
    main()
