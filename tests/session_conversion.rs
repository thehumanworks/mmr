use std::fs;
use std::process::Command;

use mmr::conversion::{InputFormat, NativeSource, TargetFormat, convert_text, parse_native};
use serde_json::{Value, json};

const CODEX: &str = include_str!("fixtures/session_conversion/codex.jsonl");
const CLAUDE: &str = include_str!("fixtures/session_conversion/claude.jsonl");

#[test]
fn canonical_preserves_archive_blocks_and_parallel_relationships() {
    let session = parse_native(CODEX, NativeSource::Codex, "host-a").unwrap();
    assert_eq!(session.archive.text, CODEX);
    assert_eq!(session.events.len(), 7);
    assert_eq!(session.events[1].timestamp, session.events[2].timestamp);
    assert_eq!(
        session.events[0].timestamp,
        Some(json!("2026-10-21T00:00:00+02:00"))
    );
    assert!(session.events[5].timestamp.is_none());
    assert_eq!(
        session.events[5]
            .blocks
            .iter()
            .map(|b| b.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["text", "attachment", "opaque", "text"]
    );
    let a = session.events[1].blocks[0].relationship.as_ref().unwrap();
    let b = session.events[2].blocks[0].relationship.as_ref().unwrap();
    assert_eq!(a.status, "matched");
    assert_eq!(b.status, "matched");
    assert_eq!(a.matches, vec![format!("{}#block/0", session.events[4].id)]);
    assert_eq!(b.matches, vec![format!("{}#block/0", session.events[3].id)]);
    assert_eq!(
        session.events[6].blocks[0]
            .relationship
            .as_ref()
            .unwrap()
            .status,
        "pending"
    );
    assert_eq!(
        session.events[5].blocks[1]
            .attachment
            .as_ref()
            .unwrap()
            .availability,
        "reference_only"
    );
    assert_eq!(
        session.events[5].blocks[1]
            .attachment
            .as_ref()
            .unwrap()
            .content_hash
            .as_deref(),
        Some("fixture-hash")
    );
    for event in &session.events {
        assert_eq!(
            &CODEX[event.raw.start..event.raw.end],
            event.raw_text(&session).unwrap()
        );
    }
}

#[test]
fn claude_preserves_source_roles_parents_and_large_tool_results() {
    let mut session = parse_native(CLAUDE, NativeSource::Claude, "host-a").unwrap();
    assert_eq!(session.events[0].parent_id.as_deref(), Some("previous"));
    assert_eq!(session.events[1].source_role.as_deref(), Some("user"));
    assert_eq!(session.events[1].role.as_deref(), Some("tool"));
    assert_eq!(
        session.events[1].blocks[0].result.as_ref().unwrap()[1]["type"],
        "image"
    );
    assert_eq!(
        session.events[0].blocks[1].source["unknown_field"],
        "retain"
    );
    let long = "z".repeat(20_000);
    let row = json!({"type":"user","sessionId":"c","uuid":"r","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"late","content":long}]}}).to_string();
    session = parse_native(&row, NativeSource::Claude, "host-a").unwrap();
    assert_eq!(session.events[0].blocks[0].result, Some(json!(long)));
    assert_eq!(
        session.events[0].blocks[0]
            .relationship
            .as_ref()
            .unwrap()
            .status,
        "orphan"
    );
}

#[test]
fn identities_survive_append_and_namespace_collisions_stay_separate() {
    let first = parse_native(CODEX, NativeSource::Codex, "host-a").unwrap();
    let appended = format!("{CODEX}{{\"type\":\"future\",\"id\":\"new\"}}\n");
    let second = parse_native(&appended, NativeSource::Codex, "host-a").unwrap();
    assert_eq!(first.events, second.events[..first.events.len()]);
    let other = parse_native(CODEX, NativeSource::Codex, "host-b").unwrap();
    assert_ne!(first.events[1].id, other.events[1].id);
    let rows = "{\"type\":\"response_item\",\"session_id\":\"one\",\"payload\":{\"type\":\"function_call\",\"call_id\":\"same\",\"name\":\"x\"}}\n{\"type\":\"response_item\",\"session_id\":\"two\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"same\",\"output\":\"other session\"}}\n";
    let separated = parse_native(rows, NativeSource::Codex, "host-a").unwrap();
    assert_eq!(
        separated.events[0].blocks[0]
            .relationship
            .as_ref()
            .unwrap()
            .status,
        "pending"
    );
    assert_eq!(
        separated.events[1].blocks[0]
            .relationship
            .as_ref()
            .unwrap()
            .status,
        "orphan"
    );
}

#[test]
fn duplicates_and_branch_ambiguity_are_explicit() {
    let call = json!({"type":"response_item","session_id":"s","payload":{"type":"function_call","id":"duplicate-id","call_id":"same","name":"x"}}).to_string();
    let result = json!({"type":"response_item","session_id":"s","payload":{"type":"function_call_output","call_id":"same","output":"ok"}}).to_string();
    let session = parse_native(
        &format!("{call}\n{call}\n{result}\n"),
        NativeSource::Codex,
        "origin",
    )
    .unwrap();
    assert_eq!(session.events.len(), 3);
    assert_ne!(session.events[0].id, session.events[1].id);
    assert_eq!(
        session.events[2].blocks[0]
            .relationship
            .as_ref()
            .unwrap()
            .status,
        "ambiguous"
    );
    assert!(
        session
            .findings
            .iter()
            .any(|f| f.code == "duplicate_source_identity")
    );
}

#[test]
fn malformed_middle_partial_tail_blank_and_crlf_are_archived() {
    let raw = "{\"type\":\"future\",\"extra\":true}\r\n\r\nnot-json\n{\"unfinished\":";
    let session = parse_native(raw, NativeSource::Codex, "origin").unwrap();
    assert_eq!(session.archive.text.as_bytes(), raw.as_bytes());
    assert_eq!(session.events.len(), 4);
    assert_eq!(session.events[1].record_type.as_deref(), Some("blank"));
    assert!(
        session
            .findings
            .iter()
            .any(|f| f.code == "malformed_record")
    );
    assert!(session.findings.iter().any(|f| f.code == "truncated_tail"));
    let json = serde_json::to_string(&session).unwrap();
    let imported =
        convert_text(&json, InputFormat::Canonical, TargetFormat::Canonical, None).unwrap();
    assert_eq!(imported.canonical, session);
}

#[test]
fn canonical_reimport_detects_archive_event_span_and_version_tampering() {
    let session = parse_native(CODEX, NativeSource::Codex, "origin").unwrap();
    for change in ["archive", "event", "span", "version", "unknown"] {
        let mut value = serde_json::to_value(&session).unwrap();
        match change {
            "archive" => value["archive"]["text"] = json!("changed"),
            "event" => value["events"][1]["blocks"][0]["call_id"] = json!("forged"),
            "span" => value["events"][0]["raw"]["end"] = json!(1),
            "version" => value["schema_version"] = json!(99),
            _ => value["unexpected_authority"] = json!(true),
        }
        assert!(
            convert_text(
                &value.to_string(),
                InputFormat::Canonical,
                TargetFormat::Canonical,
                None
            )
            .is_err(),
            "accepted {change}"
        );
    }
    assert!(
        convert_text(
            &serde_json::to_string(&session).unwrap(),
            InputFormat::Canonical,
            TargetFormat::Canonical,
            Some("different")
        )
        .is_err()
    );
}

#[test]
fn readable_conversion_retains_recovery_and_reports_unavailable_semantics() {
    for (native, source) in [(CODEX, InputFormat::Codex), (CLAUDE, InputFormat::Claude)] {
        for target in [TargetFormat::CodexReadable, TargetFormat::ClaudeReadable] {
            let converted = convert_text(native, source, target, Some("origin")).unwrap();
            assert_eq!(converted.canonical.archive.text, native);
            assert!(!converted.capabilities.native_resume);
            assert!(!converted.capabilities.api_replay);
            assert!(converted.findings.iter().any(|f| f.code == "readable_only"));
            assert!(
                converted
                    .findings
                    .iter()
                    .any(|f| f.disposition == "archive_only")
            );
            for row in converted.output_jsonl.as_ref().unwrap().lines() {
                let v: Value = serde_json::from_str(row).unwrap();
                assert!(v.get("_mmr").is_some());
            }
            let restarted = convert_text(
                &serde_json::to_string(&converted).unwrap(),
                InputFormat::Canonical,
                target,
                None,
            )
            .unwrap();
            assert_eq!(converted, restarted);
        }
    }
}

fn cli(
    home: &std::path::Path,
    input: &std::path::Path,
    from: &str,
    target: &str,
    origin: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mmr"));
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(home)
        .args(["convert", "session"])
        .arg(input)
        .args(["--from", from, "--to", target]);
    if let Some(origin) = origin {
        command.args(["--origin", origin]);
    }
    command.output().unwrap()
}

#[test]
fn cli_restart_canonical_replay_is_deterministic_and_never_discovers_history() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    // A normal source load fails on these invalid-UTF8 fixture files.
    fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    fs::write(home.join(".codex/sessions/poison.jsonl"), [0xff]).unwrap();
    fs::create_dir_all(home.join(".claude/projects/poison")).unwrap();
    fs::write(home.join(".claude/projects/poison/session.jsonl"), [0xff]).unwrap();
    let negative_control = Command::new(env!("CARGO_BIN_EXE_mmr"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(&home)
        .args(["--source", "codex", "read", "source"])
        .output()
        .unwrap();
    assert!(
        !negative_control.status.success(),
        "normal provider loading must reject poisoned fixture"
    );
    let input = temp.path().join("input.jsonl");
    fs::write(&input, CODEX).unwrap();
    let first = cli(
        &home,
        &input,
        "codex",
        "claude-readable",
        Some("fixture-host"),
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let response: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(response["command"], "convert/session");
    let saved = temp.path().join("portable.json");
    fs::write(&saved, &first.stdout).unwrap();
    let second = cli(&home, &saved, "canonical", "claude-readable", None);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    let moved = temp.path().join("moved.jsonl");
    fs::copy(&input, &moved).unwrap();
    let third = cli(
        &home,
        &moved,
        "codex",
        "claude-readable",
        Some("fixture-host"),
    );
    assert_eq!(first.stdout, third.stdout);
    assert!(!home.join(".local").exists());
    assert!(!home.join("Library").exists());
}

#[test]
fn cli_requires_origin_and_rejects_other_formats_and_oversized_input() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.jsonl");
    fs::write(&input, CODEX).unwrap();
    assert!(
        !cli(temp.path(), &input, "codex", "canonical", None)
            .status
            .success()
    );
    assert!(
        !cli(temp.path(), &input, "cursor", "canonical", Some("origin"))
            .status
            .success()
    );
    fs::write(&input, vec![b'x'; 16 * 1024 * 1024 + 1]).unwrap();
    let output = cli(temp.path(), &input, "codex", "canonical", Some("origin"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("byte limit"));
}

#[test]
fn conversion_limits_nesting_and_reports_untrusted_missing_relationship_scope() {
    let nested = format!(
        "{{\"type\":\"future\",\"data\":{}0{}}}\n",
        "[".repeat(70),
        "]".repeat(70)
    );
    let response = convert_text(
        &nested,
        InputFormat::Codex,
        TargetFormat::CodexReadable,
        Some("origin"),
    )
    .unwrap();
    assert!(
        response
            .findings
            .iter()
            .any(|f| f.code == "source_depth_limit")
    );
    assert_eq!(response.canonical.archive.text, nested);
    convert_text(
        &serde_json::to_string(&response).unwrap(),
        InputFormat::Canonical,
        TargetFormat::Canonical,
        None,
    )
    .unwrap();
    let unknown_session = "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"call_id\":\"id\",\"status\":\"interrupted\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"id\",\"output\":\"untrusted\",\"is_error\":true}}\n";
    let parsed = parse_native(unknown_session, NativeSource::Codex, "origin").unwrap();
    assert_eq!(
        parsed.events[0].blocks[0].status,
        Some(json!("interrupted"))
    );
    assert_eq!(parsed.events[1].blocks[0].is_error, Some(json!(true)));
    assert_eq!(
        parsed.events[1].blocks[0]
            .relationship
            .as_ref()
            .unwrap()
            .status,
        "unknown_session"
    );
    let mut forged = serde_json::to_value(response).unwrap();
    forged["capabilities"]["native_resume"] = json!(true);
    assert!(
        convert_text(
            &forged.to_string(),
            InputFormat::Canonical,
            TargetFormat::Canonical,
            None
        )
        .is_err()
    );
}

#[test]
fn explicit_null_fields_roundtrip_without_becoming_absent() {
    for source in [InputFormat::Codex, InputFormat::Claude] {
        let native = if source == InputFormat::Codex {
            json!({"type":"response_item","session_id":"session","timestamp":null,"payload":{"type":"function_call_output","call_id":"c","output":null,"status":null,"is_error":null}}).to_string()
        } else {
            json!({"type":"assistant","sessionId":"session","timestamp":null,"message":{"role":"assistant","content":[{"type":"tool_use","id":"c","arguments":null,"input":null,"status":null}]}}).to_string()
        };
        let converted =
            convert_text(&native, source, TargetFormat::Canonical, Some("origin")).unwrap();
        assert_eq!(converted.canonical.events[0].timestamp, Some(Value::Null));
        let encoded = serde_json::to_string(&converted).unwrap();
        let replay = convert_text(
            &encoded,
            InputFormat::Canonical,
            TargetFormat::Canonical,
            None,
        )
        .unwrap();
        assert_eq!(converted, replay);
    }
}

#[test]
fn duplicate_relationship_budget_is_linear_and_oversized_identifiers_fail_early() {
    let call=json!({"type":"response_item","session_id":"s","payload":{"type":"function_call","call_id":"same"}}).to_string();
    let result=json!({"type":"response_item","session_id":"s","payload":{"type":"function_call_output","call_id":"same","output":"ok"}}).to_string();
    let raw = format!(
        "{}{}",
        format!("{call}\n").repeat(2000),
        format!("{result}\n").repeat(2000)
    );
    let parsed = parse_native(&raw, NativeSource::Codex, "origin").unwrap();
    assert_eq!(parsed.events.len(), 4000);
    for event in &parsed.events {
        let link = event.blocks[0].relationship.as_ref().unwrap();
        assert_eq!(link.status, "ambiguous");
        assert!(link.matches.is_empty());
        assert_eq!((link.call_count, link.result_count), (2000, 2000));
    }
    assert!(serde_json::to_vec(&parsed).unwrap().len() < raw.len() * 16);
    let giant_id = json!({"type":"session_meta","payload":{"id":"x".repeat(1025)}}).to_string();
    let err = parse_native(
        &format!("{giant_id}\n{}", "{\"type\":\"future\"}\n".repeat(100)),
        NativeSource::Codex,
        "origin",
    )
    .unwrap_err();
    assert!(err.to_string().contains("identifier exceeds byte limit"));
}

#[test]
fn user_message_normalizes_role_without_fabricating_original_role() {
    let raw=json!({"type":"event_msg","session_id":"s","payload":{"type":"user_message","message":"hello"}}).to_string();
    let session = parse_native(&raw, NativeSource::Codex, "origin").unwrap();
    assert_eq!(session.events[0].role.as_deref(), Some("user"));
    assert!(session.events[0].source_role.is_none());
}

#[test]
fn attachment_payload_presence_never_claims_verified_bytes() {
    for data in [
        Value::Null,
        json!(false),
        json!(42),
        json!(""),
        json!("aGVsbG8="),
    ] {
        let row=json!({"type":"assistant","sessionId":"s","message":{"role":"assistant","content":[{"type":"image","source":{"type":"base64","data":data,"media_type":"image/png"}}]}}).to_string();
        let session = parse_native(&row, NativeSource::Claude, "origin").unwrap();
        let attachment = session.events[0].blocks[0].attachment.as_ref().unwrap();
        if data == json!("aGVsbG8=") {
            assert_eq!(attachment.availability, "embedded_unverified");
            assert!(
                session
                    .findings
                    .iter()
                    .any(|f| f.code == "attachment_payload_unverified")
            );
        } else {
            assert_eq!(attachment.availability, "reference_only");
            assert!(
                session
                    .findings
                    .iter()
                    .any(|f| f.code == "attachment_bytes_unavailable")
            );
        }
    }
}
