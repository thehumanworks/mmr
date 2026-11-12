"""Find a decision, read its session, then collect bounded project context. No setup."""

from pathlib import Path
from tempfile import TemporaryDirectory

from _synthetic import binary, native_snapshot, run, seed


def main():
    mmr = binary()
    with TemporaryDirectory(prefix="mmr-find-read-context-") as temp:
        home = Path(temp) / "home"
        project = seed(home)
        before = native_snapshot(home)

        found = run(mmr, home, project, "find", "RETRY")
        assert found["total_matches"] == 1, found
        match = found["matches"][0]
        assert match["snippet"] == "Keep the retry limit at three.", found
        print(f'find: {match["snippet"]}')
        print(f'read with: {match["read_command"]}')

        read = run(mmr, home, project, "--source", match["source"], "read", "session",
                   match["session_id"])
        assert [(item["role"], item["content"]) for item in read["messages"]] == [
            ("user", "Keep the retry limit at three."),
            ("assistant", "Noted for the next session."),
        ], read
        for message in read["messages"]:
            print(f'read {message["role"]}: {message["content"]}')
        session = match["session_id"]

        context = run(mmr, home, project, "context", "project", "--limit", "2")
        assert context["messages"] == read["messages"], context
        assert context["total_sessions"] == 1, context
        assert context["sessions"][0]["session_id"] == session, context
        print(f'context: {context["total_sessions"]} session, {len(context["messages"])} messages')
        assert native_snapshot(home) == before, "Native histories changed"
        print("Native histories unchanged; nothing was set up or written.")
    print("Temporary home removed.")


if __name__ == "__main__":
    main()
