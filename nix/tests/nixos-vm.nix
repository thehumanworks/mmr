# Boots a NixOS VM with both modules enabled, seeds synthetic Codex history for
# one user, and checks that the CLI and the MCP service both read it.
{ pkgs, self }:

pkgs.testers.runNixOSTest {
  name = "mmr";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ self.nixosModules.default ];

      users.users.alice.isNormalUser = true;

      programs.mmr = {
        enable = true;
        defaultSource = "codex";
      };

      services.mmr-mcp = {
        enable = true;
        user = "alice";
      };

      environment.systemPackages = [ pkgs.curl ];
    };

  testScript = ''
    import json

    session = "\n".join([
        json.dumps({"type": "session_meta", "timestamp": "2025-01-02T00:00:00",
                    "payload": {"id": "sess-nix-1", "cwd": "/home/alice/nix-proj",
                                "cli_version": "1.0.0", "model_provider": "openai",
                                "timestamp": "2025-01-02T00:00:00"}}),
        json.dumps({"type": "event_msg", "timestamp": "2025-01-02T00:00:01",
                    "payload": {"type": "user_message", "message": "hello from nix"}}),
    ])

    def mcp(payload):
        out = machine.succeed(
            "curl -sSf http://127.0.0.1:8765/mcp "
            "-H 'Content-Type: application/json' "
            "-H 'Accept: application/json, text/event-stream' "
            f"-d '{json.dumps(payload)}'"
        )
        return json.loads(out)

    machine.wait_for_unit("multi-user.target")
    machine.succeed("install -d -o alice -g users /home/alice/.codex/sessions")
    machine.succeed(
        f"echo '{session}' > /home/alice/.codex/sessions/sess-nix-1.jsonl",
        "chown alice:users /home/alice/.codex/sessions/sess-nix-1.jsonl",
    )

    with subtest("the CLI is installed and exports its defaults"):
        machine.succeed("mmr --version")
        assert machine.succeed("su - alice -c 'echo $MMR_DEFAULT_SOURCE'").strip() == "codex"
        projects = machine.succeed("su - alice -c 'mmr list projects'")
        assert "nix-proj" in projects, projects

    with subtest("the MCP service runs as alice and answers MCP requests"):
        machine.wait_for_unit("mmr-mcp.service")
        machine.wait_for_open_port(8765)
        assert machine.succeed("systemctl show -P User mmr-mcp.service").strip() == "alice"
        init = mcp({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                               "clientInfo": {"name": "nixos-test", "version": "1.0"}}})
        assert "tools" in init["result"]["capabilities"], init
        listed = mcp({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                      "params": {"name": "mmr_list_projects", "arguments": {}}})
        text = listed["result"]["content"][0]["text"]
        assert "nix-proj" in text, text

    with subtest("the MCP service stops cleanly"):
        machine.succeed("systemctl stop mmr-mcp.service")
        machine.fail("systemctl is-failed mmr-mcp.service")
  '';
}
