use serde_json::Value;

use anyhow::{Result, ensure};

use super::{
    AdapterCapabilities, AdapterDescriptor, AttachmentRef, CanonicalBlock, HistoryAdapter,
    MAX_BLOCKS, MAX_IDENTIFIER_BYTES, PARSER_VERSION, RecordProjection,
};

pub struct CodexHistoryAdapter;
pub struct ClaudeHistoryAdapter;

impl HistoryAdapter for CodexHistoryAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        builtin_descriptor("codex")
    }
    fn project_record(&self, value: &Value) -> Result<RecordProjection> {
        let mut projection = builtin_projection(value)?;
        projection.session_id = string(value, "session_id").or_else(|| {
            if string(value, "type").as_deref() == Some("session_meta") {
                string(&value["payload"], "id")
            } else {
                None
            }
        });
        check_content_count(&value["payload"]["content"])?;
        parse_codex_record(value, &mut projection);
        Ok(projection)
    }
}
impl HistoryAdapter for ClaudeHistoryAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        builtin_descriptor("claude")
    }
    fn project_record(&self, value: &Value) -> Result<RecordProjection> {
        let mut projection = builtin_projection(value)?;
        projection.session_id = string(value, "sessionId").or_else(|| string(value, "session_id"));
        check_content_count(&value["message"]["content"])?;
        parse_claude_record(value, &mut projection);
        Ok(projection)
    }
}
fn builtin_descriptor(source_id: &str) -> AdapterDescriptor {
    AdapterDescriptor {
        source_id: source_id.into(),
        version: PARSER_VERSION.into(),
        capabilities: AdapterCapabilities::readable_only(Vec::new()),
    }
}
fn check_content_count(content: &Value) -> Result<()> {
    ensure!(
        content
            .as_array()
            .is_none_or(|items| items.len() <= MAX_BLOCKS),
        "session conversion block limit exceeded ({MAX_BLOCKS})"
    );
    Ok(())
}
fn builtin_projection(value: &Value) -> Result<RecordProjection> {
    // Keep legacy builtin validation and mapping inside these adapters, not the engine.
    for candidate in [
        value.get("sessionId"),
        value.get("session_id"),
        value.get("uuid"),
        value.get("id"),
        value.get("branch_id"),
        value.get("parentUuid"),
        value.get("parent_id"),
        value["payload"].get("id"),
        value["payload"].get("branch_id"),
        value["payload"].get("parent_id"),
        value["payload"]["git"].get("branch"),
    ] {
        ensure!(
            candidate
                .and_then(Value::as_str)
                .is_none_or(|v| v.len() <= MAX_IDENTIFIER_BYTES),
            "source identifier exceeds byte limit ({MAX_IDENTIFIER_BYTES})"
        );
    }
    Ok(RecordProjection {
        branch_id: string(value, "branch_id")
            .or_else(|| string(&value["payload"], "branch_id"))
            .or_else(|| string(&value["payload"]["git"], "branch")),
        source_event_id: string(value, "uuid")
            .or_else(|| string(value, "id"))
            .or_else(|| string(&value["payload"], "id")),
        parent_id: string(value, "parentUuid")
            .or_else(|| string(value, "parent_id"))
            .or_else(|| string(&value["payload"], "parent_id")),
        record_type: string(value, "type"),
        timestamp: value.get("timestamp").cloned().or_else(|| {
            value
                .get("message")
                .and_then(|v| v.get("timestamp"))
                .cloned()
        }),
        ..RecordProjection::default()
    })
}

pub(super) fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn empty_block(kind: &str, source: Value) -> CanonicalBlock {
    CanonicalBlock::new(kind, source)
}
pub(super) fn opaque(source: Value) -> CanonicalBlock {
    empty_block("opaque", source)
}

pub(super) fn parse_codex_record(value: &Value, event: &mut RecordProjection) {
    let payload = &value["payload"];
    event.source_role = string(payload, "role");
    event.role = event.source_role.clone();
    match event.record_type.as_deref() {
        Some("response_item") => match string(payload, "type").as_deref() {
            Some("function_call" | "tool_call") => {
                event.role = Some("assistant".into());
                event
                    .blocks
                    .push(tool_call(payload, "call_id", "arguments"));
            }
            Some("function_call_output" | "tool_result") => {
                event.role = Some("tool".into());
                event.blocks.push(tool_result(payload, "call_id", "output"));
            }
            _ if payload.get("content").is_some() => {
                event.blocks = content_blocks(&payload["content"]);
            }
            _ => event.blocks.push(opaque(payload.clone())),
        },
        Some("event_msg") if string(payload, "type").as_deref() == Some("user_message") => {
            event.role = Some("user".into());
            event.blocks = content_blocks(&payload["message"]);
        }
        _ => event.blocks.push(opaque(value.clone())),
    }
}

pub(super) fn parse_claude_record(value: &Value, event: &mut RecordProjection) {
    if let Some(message) = value.get("message") {
        event.source_role = string(message, "role");
        event.role = event.source_role.clone();
        event.blocks = content_blocks(&message["content"]);
        if !event.blocks.is_empty() && event.blocks.iter().all(|b| b.kind == "tool_result") {
            event.role = Some("tool".into());
        }
    } else {
        event.blocks.push(opaque(value.clone()));
    }
}

fn content_blocks(value: &Value) -> Vec<CanonicalBlock> {
    if let Some(items) = value.as_array() {
        items.iter().map(content_block).collect()
    } else {
        vec![content_block(value)]
    }
}

fn content_block(value: &Value) -> CanonicalBlock {
    if let Some(text) = value.as_str() {
        let mut block = empty_block("text", value.clone());
        block.text = Some(text.into());
        return block;
    }
    match string(value, "type").as_deref() {
        Some("text" | "input_text" | "output_text") => {
            let mut block = empty_block("text", value.clone());
            block.text = string(value, "text");
            if block.text.is_none() {
                return opaque(value.clone());
            }
            block
        }
        Some("tool_use") => tool_call(value, "id", "input"),
        Some("tool_result") => tool_result(value, "tool_use_id", "content"),
        Some("image" | "input_image" | "image_url" | "file" | "document" | "input_file") => {
            let mut block = empty_block("attachment", value.clone());
            let source = value.get("source").unwrap_or(value);
            let embedded = (string(source, "type").as_deref() == Some("base64")
                && source
                    .get("data")
                    .and_then(Value::as_str)
                    .is_some_and(|data| !data.is_empty()))
                || value
                    .get("file_data")
                    .and_then(Value::as_str)
                    .is_some_and(|data| !data.is_empty());
            block.attachment = Some(AttachmentRef {
                content_hash: string(value, "sha256")
                    .or_else(|| string(source, "sha256"))
                    .or_else(|| string(value, "hash")),
                media_type: string(value, "mime_type").or_else(|| string(source, "media_type")),
                locator: string(value, "url")
                    .or_else(|| string(source, "url"))
                    .or_else(|| string(value, "image_url"))
                    .or_else(|| string(value, "file_id")),
                availability: if embedded {
                    "embedded_unverified"
                } else {
                    "reference_only"
                }
                .into(),
            });
            block
        }
        _ => opaque(value.clone()),
    }
}

fn tool_call(value: &Value, id_field: &str, arguments_field: &str) -> CanonicalBlock {
    let mut block = empty_block("tool_call", value.clone());
    block.call_id = string(value, id_field);
    block.name = string(value, "name");
    block.arguments = value.get(arguments_field).cloned();
    block.status = value.get("status").cloned();
    block
}
fn tool_result(value: &Value, id_field: &str, result_field: &str) -> CanonicalBlock {
    let mut block = empty_block("tool_result", value.clone());
    block.call_id = string(value, id_field);
    block.result = value.get(result_field).cloned();
    block.status = value.get("status").cloned();
    block.is_error = value.get("is_error").cloned();
    block
}
