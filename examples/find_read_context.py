"""Find a decision, read its native session, then collect bounded project context."""

from pathlib import Path
from tempfile import TemporaryDirectory

from _synthetic import binary, native_snapshot, run, seed


def main():
    mmr = binary()
    with TemporaryDirectory(prefix="mmr-find-read-context-") as temp:
        home = Path(temp) / "home"
        project = seed(home)
        before = native_snapshot(home)
        run(mmr, home, project, "init")

        found = run(mmr, home, project, "find", "retry")
        assert found["total_results"] == 1, found
        assert found["results"][0]["snippet"] == "Keep the retry limit at three.", found
        print(f'find: {found["results"][0]["snippet"]}')

        # find reports store IDs. retrieve supplies the provider ID for read.
        retrieved = run(mmr, home, project, "retrieve", "retry", "--max-sessions", "1")
        assert retrieved["total_selected_sessions"] == 1, retrieved
        selected = retrieved["selected_sessions"][0]
        assert selected["first_match_citation"] == found["results"][0]["citation"], retrieved
        session = selected["source_session_id"]
        print(f'retrieve: {selected["source"]}/{session}')
        read = run(mmr, home, project, "--source", selected["source"], "read", "session",
                   session, "--project", selected["project_name"])
        assert [(item["role"], item["content"]) for item in read["messages"]] == [
            ("user", "Keep the retry limit at three."),
            ("assistant", "Noted for the next session."),
        ], read
        for message in read["messages"]:
            print(f'read {message["role"]}: {message["content"]}')

        context = run(mmr, home, project, "context", "project", "--limit", "2")
        assert context["messages"] == read["messages"], context
        assert context["total_sessions"] == 1, context
        assert context["sessions"][0]["session_id"] == session, context
        print(f'context: {context["total_sessions"]} session, {len(context["messages"])} messages')
        assert native_snapshot(home) == before, "Native histories changed"
        print("Native histories unchanged; only the temporary local store was initialized.")
    print("Temporary home removed.")


if __name__ == "__main__":
    main()
