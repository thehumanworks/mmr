use serde_json::Value;

use super::{AttachmentRef, CanonicalBlock, CanonicalEvent};

pub(super) fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn empty_block(kind: &str, source: Value) -> CanonicalBlock {
    CanonicalBlock {
        kind: kind.into(),
        source,
        text: None,
        call_id: None,
        name: None,
        arguments: None,
        result: None,
        status: None,
        is_error: None,
        attachment: None,
        relationship: None,
    }
}
pub(super) fn opaque(source: Value) -> CanonicalBlock {
    empty_block("opaque", source)
}

pub(super) fn parse_codex_record(value: &Value, event: &mut CanonicalEvent) {
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

pub(super) fn parse_claude_record(value: &Value, event: &mut CanonicalEvent) {
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
