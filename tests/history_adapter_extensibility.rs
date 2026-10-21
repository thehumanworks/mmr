//! This file compiles as an external crate: only the public mmr API is available.
use anyhow::Result;
use mmr::conversion::{
    AdapterCapabilities, AdapterDescriptor, AdapterFinding, AdapterRegistry, AttachmentRef,
    CanonicalBlock, HistoryAdapter, InputFormat, NativeSource, RecordProjection, RegisteredInput,
    TargetFormat, ToolRelationship, convert_text, convert_with_registry, parse_registered_native,
};
use mmr::store::content_hash;
use serde_json::{Value, json};

const THIRD: &str = include_str!("fixtures/session_conversion/external.jsonl");

struct SyntheticAdapter {
    version: &'static str,
    readable: bool,
}
impl HistoryAdapter for SyntheticAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        let mut capabilities = AdapterCapabilities::readable_only(vec!["vendor-state".into()]);
        capabilities.readable_history = self.readable;
        AdapterDescriptor {
            source_id: "test.synthetic".into(),
            version: self.version.into(),
            capabilities,
        }
    }
    fn project_record(&self, row: &Value) -> Result<RecordProjection> {
        let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
        let mut record = RecordProjection {
            session_id: text("sid"),
            source_event_id: text("eid"),
            branch_id: text("branch"),
            parent_id: text("previous"),
            record_type: Some("synthetic".into()),
            role: text("role"),
            source_role: text("role"),
            timestamp: row.get("at").cloned(),
            ..RecordProjection::default()
        };
        for part in row["parts"].as_array().into_iter().flatten() {
            let mut block =
                CanonicalBlock::new(part["kind"].as_str().unwrap_or("opaque"), part.clone());
            block.text = part.get("text").and_then(Value::as_str).map(str::to_owned);
            block.call_id = part.get("call").and_then(Value::as_str).map(str::to_owned);
            block.name = part.get("name").and_then(Value::as_str).map(str::to_owned);
            block.arguments = part.get("arguments").cloned();
            block.result = part.get("result").cloned();
            block.status = part.get("status").cloned();
            block.is_error = part.get("is_error").cloned();
            if block.kind == "attachment" {
                block.attachment = Some(AttachmentRef {
                    content_hash: part.get("hash").and_then(Value::as_str).map(str::to_owned),
                    media_type: part.get("mime").and_then(Value::as_str).map(str::to_owned),
                    locator: part.get("url").and_then(Value::as_str).map(str::to_owned),
                    availability: "reference_only".into(),
                });
            }
            if !matches!(
                block.kind.as_str(),
                "text" | "tool_call" | "tool_result" | "attachment"
            ) {
                block.kind = "opaque".into();
                record.findings.push(AdapterFinding {
                    field: "parts".into(),
                    code: "synthetic_unknown_part".into(),
                    disposition: "opaque".into(),
                    message: "Unknown synthetic part retained in its source block.".into(),
                });
            }
            record.blocks.push(block);
        }
        Ok(record)
    }
}
fn make_registry(version: &'static str) -> AdapterRegistry {
    let mut registry = AdapterRegistry::with_builtins();
    registry
        .register(SyntheticAdapter {
            version,
            readable: true,
        })
        .unwrap();
    registry
}

#[test]
fn external_adapter_preserves_raw_nulls_unknowns_attachments_and_engine_relationships() {
    let registry = make_registry("synthetic-v1");
    let first = convert_with_registry(
        THIRD,
        RegisteredInput::Native("test.synthetic"),
        TargetFormat::ClaudeReadable,
        Some("origin-a"),
        &registry,
    )
    .unwrap();
    assert_eq!(first.canonical.archive.text, THIRD);
    assert_eq!(first.canonical.source.as_str(), "test.synthetic");
    assert_eq!(first.canonical.parser_version, "synthetic-v1");
    assert_eq!(first.canonical.events[0].timestamp, Some(Value::Null));
    assert!(first.canonical.events[1].timestamp.is_none());
    assert_eq!(first.canonical.events[0].parent_id.as_deref(), Some("root"));
    assert_eq!(
        first.canonical.events[0].blocks[1].arguments,
        Some(Value::Null)
    );
    assert_eq!(
        first.canonical.events[2].blocks[0].result,
        Some(Value::Null)
    );
    assert_eq!(
        first.canonical.events[1].blocks[0].is_error,
        Some(Value::Null)
    );
    assert_eq!(
        first.canonical.events[0].blocks[1]
            .relationship
            .as_ref()
            .unwrap()
            .matches,
        vec![format!("{}#block/0", first.canonical.events[2].id)]
    );
    assert_eq!(
        first.canonical.events[0].blocks[2]
            .relationship
            .as_ref()
            .unwrap()
            .matches,
        vec![format!("{}#block/0", first.canonical.events[1].id)]
    );
    assert_eq!(
        first.canonical.events[3].blocks[0]
            .attachment
            .as_ref()
            .unwrap()
            .availability,
        "reference_only"
    );
    assert_eq!(
        first.canonical.events[3].blocks[1].source["unknown"],
        json!([1, 2])
    );
    assert!(
        first
            .findings
            .iter()
            .any(|f| f.code == "adapter_unsupported_content")
    );
    assert!(
        first
            .findings
            .iter()
            .any(|f| f.code == "attachment_bytes_unavailable")
    );
    assert!(
        first
            .findings
            .iter()
            .any(|f| f.code == "synthetic_unknown_part"
                && f.event_id.as_deref() == Some(first.canonical.events[3].id.as_str()))
    );
    assert!(!first.capabilities.native_resume && !first.capabilities.api_replay);
    let encoded = serde_json::to_string(&first).unwrap();
    // A new registry instance models a restart: source+version must be registered again.
    let restarted = convert_with_registry(
        &encoded,
        RegisteredInput::Canonical,
        TargetFormat::ClaudeReadable,
        None,
        &make_registry("synthetic-v1"),
    )
    .unwrap();
    assert_eq!(first, restarted);
}

#[test]
fn missing_version_mismatch_unknown_fields_and_archive_tampering_fail_closed() {
    let native = convert_with_registry(
        THIRD,
        RegisteredInput::Native("test.synthetic"),
        TargetFormat::Canonical,
        Some("origin-a"),
        &make_registry("synthetic-v1"),
    )
    .unwrap();
    let encoded = serde_json::to_string(&native).unwrap();
    assert!(
        convert_text(
            &encoded,
            InputFormat::Canonical,
            TargetFormat::Canonical,
            None
        )
        .is_err()
    );
    for registry in [AdapterRegistry::new(), make_registry("synthetic-v2")] {
        assert!(
            convert_with_registry(
                &encoded,
                RegisteredInput::Canonical,
                TargetFormat::Canonical,
                None,
                &registry
            )
            .is_err()
        );
    }
    let registered = make_registry("synthetic-v1");
    for field in ["archive", "version", "unknown", "span"] {
        let mut changed = serde_json::to_value(&native).unwrap();
        match field {
            "archive" => changed["canonical"]["archive"]["text"] = json!("changed"),
            "version" => changed["canonical"]["parser_version"] = json!("missing"),
            "unknown" => changed["canonical"]["new_authority"] = json!(true),
            _ => changed["canonical"]["events"][0]["raw"]["start"] = json!(1),
        }
        assert!(
            convert_with_registry(
                &changed.to_string(),
                RegisteredInput::Canonical,
                TargetFormat::Canonical,
                None,
                &registered
            )
            .is_err()
        );
    }
}

struct DescriptorOnly(AdapterDescriptor);
impl HistoryAdapter for DescriptorOnly {
    fn descriptor(&self) -> AdapterDescriptor {
        self.0.clone()
    }
    fn project_record(&self, _: &Value) -> Result<RecordProjection> {
        Ok(RecordProjection::default())
    }
}
#[test]
fn registration_rejects_duplicates_invalid_identifiers_and_native_capability_claims() {
    let mut registered = make_registry("synthetic-v1");
    assert!(
        registered
            .register(SyntheticAdapter {
                version: "synthetic-v1",
                readable: true
            })
            .is_err()
    );
    let descriptor = registered.descriptor("test.synthetic").unwrap().clone();
    for bad in ["source", "version", "native", "api", "unsupported"] {
        let mut changed = descriptor.clone();
        changed.source_id = "test.other".into();
        match bad {
            "source" => changed.source_id = "x".repeat(65),
            "version" => changed.version = "".into(),
            "native" => changed.capabilities.native_resume = true,
            "api" => changed.capabilities.api_replay = true,
            _ => changed.capabilities.unsupported_content = vec!["x".repeat(65)],
        }
        assert!(registered.register(DescriptorOnly(changed)).is_err());
    }
    assert!(
        registered
            .register(mmr::conversion::CodexHistoryAdapter)
            .is_err()
    );
    assert_eq!(registered.descriptors().count(), 3);
    let mut canonical_only = AdapterRegistry::new();
    canonical_only
        .register(SyntheticAdapter {
            version: "synthetic-v1",
            readable: false,
        })
        .unwrap();
    let canonical = convert_with_registry(
        THIRD,
        RegisteredInput::Native("test.synthetic"),
        TargetFormat::Canonical,
        Some("origin"),
        &canonical_only,
    )
    .unwrap();
    assert!(!canonical.capabilities.readable_history);
    assert!(
        convert_with_registry(
            THIRD,
            RegisteredInput::Native("test.synthetic"),
            TargetFormat::CodexReadable,
            Some("origin"),
            &canonical_only
        )
        .is_err()
    );
}

struct BadProjection(u8);
impl HistoryAdapter for BadProjection {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            source_id: "test.bad".into(),
            version: "v1".into(),
            capabilities: AdapterCapabilities::readable_only(if self.0 == 3 {
                vec!["tool_call".into()]
            } else {
                Vec::new()
            }),
        }
    }
    fn project_record(&self, _: &Value) -> Result<RecordProjection> {
        let mut record = RecordProjection {
            session_id: Some("s".into()),
            ..RecordProjection::default()
        };
        let mut block = CanonicalBlock::new("tool_call", Value::Null);
        match self.0 {
            0 => record.session_id = Some("s".repeat(1025)),
            1 => {
                block.relationship = Some(ToolRelationship {
                    status: "matched".into(),
                    matches: vec!["forged".into()],
                    call_count: 1,
                    result_count: 1,
                })
            }
            2 => block.text = Some("x".repeat(4 * 1024 * 1024)),
            3 => {}
            4 => record.record_type = Some("x".repeat(1025)),
            5 => {
                block.kind = "attachment".into();
                block.attachment = Some(AttachmentRef {
                    content_hash: None,
                    media_type: None,
                    locator: None,
                    availability: "verified_bytes".into(),
                });
            }
            6 => block.kind = "vendor_state".into(),
            _ => block.text = Some("x".repeat(17 * 1024 * 1024)),
        }
        record.blocks.push(block);
        Ok(record)
    }
}
#[test]
fn common_engine_rejects_adapter_owned_relationships_and_inherited_output_amplification() {
    for (mode, expected_error) in [
        (0, "source identifier exceeds byte limit"),
        (1, "engine-owned tool relationships"),
        (2, "adapter event materialization byte limit"),
        (3, "content kind declared unsupported"),
        (4, "source identifier exceeds byte limit"),
        (5, "attachment availability must be"),
        (6, "unknown canonical block kind"),
        (7, "adapter projection materialization byte limit"),
    ] {
        let mut registry = AdapterRegistry::new();
        registry.register(BadProjection(mode)).unwrap();
        let error =
            parse_registered_native("{}\n{}\n{}\n{}\n{}\n", "test.bad", "origin", &registry)
                .unwrap_err();
        assert!(
            error.to_string().contains(expected_error),
            "mode {mode}: {error}"
        );
    }
}

#[test]
fn external_origin_append_and_malformed_preservation_use_the_common_engine() {
    let registry = make_registry("synthetic-v1");
    let initial = parse_registered_native(THIRD, "test.synthetic", "origin-a", &registry).unwrap();
    let raw = format!("{THIRD}\r\nnot-json\n{{\"unfinished\":");
    let appended = parse_registered_native(&raw, "test.synthetic", "origin-a", &registry).unwrap();
    assert_eq!(appended.archive.text, raw);
    assert_eq!(&appended.events[..initial.events.len()], initial.events);
    assert!(
        appended
            .findings
            .iter()
            .any(|f| f.code == "malformed_record")
    );
    assert!(appended.findings.iter().any(|f| f.code == "truncated_tail"));
    let other = parse_registered_native(THIRD, "test.synthetic", "origin-b", &registry).unwrap();
    assert_ne!(initial.events[0].id, other.events[0].id);
    let encoded = serde_json::to_string(&appended).unwrap();
    let reimported = convert_with_registry(
        &encoded,
        RegisteredInput::Canonical,
        TargetFormat::Canonical,
        None,
        &registry,
    )
    .unwrap();
    assert_eq!(appended, reimported.canonical);
}

#[test]
fn six_legacy_builtin_wire_hashes_are_unchanged() {
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/session_conversion/v1-compatibility-hashes.json"
    ))
    .unwrap();
    for case in expected["cases"].as_array().unwrap() {
        let (input, from) = if case["source"] == "codex" {
            (
                include_str!("fixtures/session_conversion/codex.jsonl"),
                InputFormat::Codex,
            )
        } else {
            (
                include_str!("fixtures/session_conversion/claude.jsonl"),
                InputFormat::Claude,
            )
        };
        let target = match case["target"].as_str().unwrap() {
            "canonical" => TargetFormat::Canonical,
            "codex-readable" => TargetFormat::CodexReadable,
            _ => TargetFormat::ClaudeReadable,
        };
        let response =
            convert_text(input, from, target, Some(case["origin"].as_str().unwrap())).unwrap();
        let encoded = serde_json::to_string(&response).unwrap();
        assert_eq!(content_hash(&encoded), case["sha256"].as_str().unwrap());
        assert_eq!(encoded.len(), case["bytes"].as_u64().unwrap() as usize);
    }
    assert_eq!(
        serde_json::to_string(&NativeSource::Codex).unwrap(),
        "\"codex\""
    );
    assert_eq!(
        serde_json::to_string(&NativeSource::Claude).unwrap(),
        "\"claude\""
    );
}
