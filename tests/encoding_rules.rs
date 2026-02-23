use mmr::ingest::decode_project_name;

/// Simulates Claude Code's encoding: `path.replace(/[^a-zA-Z0-9]/g, "-")`
fn claude_code_encode(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn known_mappings() -> Vec<(&'static str, &'static str)> {
    vec![
        ("-Users-mish", "/Users/mish"),
        ("-Users-mish-ClaudeOS", "/Users/mish/ClaudeOS"),
        ("-Users-mish-memory", "/Users/mish/memory"),
        (
            "-Users-mish-workspaces-experiments-agpy",
            "/Users/mish/workspaces/experiments/agpy",
        ),
        (
            "-Users-mish-workspaces-experiments-msi",
            "/Users/mish/workspaces/experiments/msi",
        ),
        (
            "-Users-mish-workspaces-games-goodboy",
            "/Users/mish/workspaces/games/goodboy",
        ),
        (
            "-Users-mish-workspaces-sandbox-agpy",
            "/Users/mish/workspaces/sandbox/agpy",
        ),
        (
            "-Users-mish-workspaces-tools-notebooklm",
            "/Users/mish/workspaces/tools/notebooklm",
        ),
        (
            "-Users-mish-workspaces-tools-wit",
            "/Users/mish/workspaces/tools/wit",
        ),
        (
            "-Users-mish--claude-skills-wit",
            "/Users/mish/.claude/skills/wit",
        ),
        ("-Users-mish--warp-themes", "/Users/mish/.warp/themes"),
        (
            "-Users-mish-workspaces-experiments-modal-rs--agents-tasks",
            "/Users/mish/workspaces/experiments/modal-rs/.agents/tasks",
        ),
        (
            "-Users-mish-workspaces-experiments-codex-auth",
            "/Users/mish/workspaces/experiments/codex-auth",
        ),
        (
            "-Users-mish-workspaces-experiments-modal-rs",
            "/Users/mish/workspaces/experiments/modal-rs",
        ),
        (
            "-Users-mish-workspaces-experiments-modal-rs-main-fixed",
            "/Users/mish/workspaces/experiments/modal-rs-main-fixed",
        ),
        (
            "-Users-mish-workspaces-experiments-modal-rs-main-updated",
            "/Users/mish/workspaces/experiments/modal-rs-main-updated",
        ),
        (
            "-Users-mish-workspaces-experiments-modal-rs-main-updated-crates-asi",
            "/Users/mish/workspaces/experiments/modal-rs-main-updated/crates/asi",
        ),
        (
            "-Users-mish-workspaces-tools-perplexity-finance",
            "/Users/mish/workspaces/tools/perplexity-finance",
        ),
        (
            "-Users-mish-workspaces-experiments-modalrs-optimized",
            "/Users/mish/workspaces/experiments/modalrs_optimized",
        ),
    ]
}

#[test]
fn test_claude_code_encoding_rule() {
    for (encoded, actual_path) in known_mappings() {
        let computed = claude_code_encode(actual_path);
        assert_eq!(computed, encoded);
    }
}

#[test]
fn test_decode_project_name_is_identity_fallback() {
    assert_eq!(
        decode_project_name("-Users-mish--claude-skills-wit"),
        "-Users-mish--claude-skills-wit"
    );
    assert_eq!(
        decode_project_name("-Users-mish-workspaces-experiments-codex-auth"),
        "-Users-mish-workspaces-experiments-codex-auth"
    );
    assert_eq!(decode_project_name("some-plain-name"), "some-plain-name");
}

#[test]
fn test_encoding_is_lossy() {
    let path_with_dash = "/Users/mish/my-project";
    let path_with_slash = "/Users/mish/my/project";
    let path_with_underscore = "/Users/mish/my_project";
    let path_with_space = "/Users/mish/my project";
    let path_with_dot = "/Users/mish/my.project";

    let encoded_dash = claude_code_encode(path_with_dash);
    let encoded_slash = claude_code_encode(path_with_slash);
    let encoded_underscore = claude_code_encode(path_with_underscore);
    let encoded_space = claude_code_encode(path_with_space);
    let encoded_dot = claude_code_encode(path_with_dot);

    assert_eq!(encoded_dash, encoded_slash);
    assert_eq!(encoded_dash, encoded_underscore);
    assert_eq!(encoded_dash, encoded_space);
    assert_eq!(encoded_dash, encoded_dot);
    assert_eq!(encoded_dash, "-Users-mish-my-project");
}
