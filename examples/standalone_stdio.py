#!/usr/bin/env python3
"""Read a synthetic session through MCP without a REST server or socket."""

import json
from pathlib import Path
from queue import Queue
import subprocess
from tempfile import TemporaryDirectory
from threading import Thread
from time import monotonic

from _synthetic import SESSION, binary, environment, native_snapshot, seed


def main():
    mmr = binary()
    with TemporaryDirectory(prefix="mmr-stdio-example-") as temporary:
        home = Path(temporary) / "home"
        project = seed(home)
        before = native_snapshot(home)
        env = environment(home)
        env.update(MMR_MCP_REST_BACKING_BIND="not-a-socket-address",
                   MMR_MCP_REST_BACKING_BASE_URL="not-a-url")
        with subprocess.Popen([mmr, "mcp", "--transport", "stdio"], cwd=project,
                              env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True) as process:
            lines = Queue()

            def read_stdout():
                for line in process.stdout:
                    lines.put(line)
                lines.put(None)

            reader = Thread(target=read_stdout, daemon=True)
            reader.start()

            def send(message):
                process.stdin.write(json.dumps(message) + "\n")
                process.stdin.flush()

            def request(identifier, method, params):
                send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
                deadline = monotonic() + 5
                while True:
                    line = lines.get(timeout=max(0, deadline - monotonic()))
                    assert line is not None, "MCP closed stdout before responding"
                    response = json.loads(line)  # Human diagnostics on stdout fail here.
                    assert response["jsonrpc"] == "2.0"
                    if response.get("id") == identifier:
                        assert "error" not in response, response
                        return response["result"]

            try:
                info = request(1, "initialize", {
                    "protocolVersion": "2025-03-26", "capabilities": {},
                    "clientInfo": {"name": "mmr-example", "version": "1"},
                })
                assert "tools" in info["capabilities"]
                send({"jsonrpc": "2.0", "method": "notifications/initialized"})
                tools = request(2, "tools/list", {})["tools"]
                assert any(tool["name"] == "mmr_read_session" for tool in tools)
                result = request(3, "tools/call", {"name": "mmr_read_session",
                    "arguments": {"source": "codex", "session_id": SESSION}})
                messages = json.loads(result["content"][0]["text"])["messages"]
                assert [message["content"] for message in messages] == [
                    "Keep the retry limit at three.", "Noted for the next session."]
                process.stdin.close()
                assert process.wait(timeout=5) == 0, "MCP failed to exit on EOF"
                # Check any trailing frames before EOF too.
                while True:
                    line = lines.get(timeout=5)
                    if line is None:
                        break
                    assert json.loads(line)["jsonrpc"] == "2.0"
                assert not process.stderr.read(), "unexpected server diagnostics"
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                reader.join(timeout=5)

        assert native_snapshot(home) == before, "native histories changed"
        print("Stdio initialized with unusable REST settings; no REST server needed.")
        print(f"Listed {len(tools)} MCP tools.")
        for message in messages:
            print(f"{message['role']}: {message['content']}")
        print("Protocol stdout was JSON only; native histories unchanged; clean EOF shutdown.")
    print("Temporary home removed.")


if __name__ == "__main__":
    main()
