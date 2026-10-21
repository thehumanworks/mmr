use std::io::Write;

use anyhow::Result;
use serde_json::{Value, json};

use super::BoundedWriter;
use super::{
    CanonicalBlock, CanonicalEvent, CanonicalSession, ConversionFinding, TargetFormat, finding,
};

pub(super) fn project_readable(
    session: &CanonicalSession,
    target: TargetFormat,
    findings: &mut Vec<ConversionFinding>,
) -> Result<String> {
    let mut output = BoundedWriter::buffer();
    for event in &session.events {
        let metadata = json!({"event_id":event.id,"origin":session.origin,"session_id":event.session_id,"branch_id":event.branch_id,"parent_id":event.parent_id,"source_role":event.source_role,"raw":event.raw});
        findings.push(finding(Some(&event.id), "raw", "source_record_retained", "archive_only", "Original record metadata, formatting and opaque fields remain recoverable from canonical.archive and the event byte span."));
        if event.role.is_none() || event.record_type.as_deref() == Some("malformed") {
            append_row(
                &mut output,
                &json!({"type":"mmr_opaque","timestamp":event.timestamp,"_mmr":metadata,"source_record":event.raw_text(session)?}),
            )?;
            findings.push(finding(Some(&event.id), "record", "opaque_target_record", "opaque", "This source record has no target message equivalent; an archival placeholder is emitted."));
            continue;
        }
        if target == TargetFormat::CodexReadable {
            for (index, block) in event.blocks.iter().enumerate() {
                let payload = match block.kind.as_str() {
                    "tool_call" => {
                        json!({"type":"function_call","call_id":block.call_id,"name":block.name,"arguments":codex_arguments(block),"status":block.status})
                    }
                    "tool_result" => {
                        json!({"type":"function_call_output","call_id":block.call_id,"output":block.result,"status":block.status,"is_error":block.is_error})
                    }
                    _ => {
                        json!({"type":"message","role":event.role,"content":[project_block(block, target, event, index, findings)]})
                    }
                };
                let mut meta = metadata.clone();
                meta["block_index"] = json!(index);
                append_row(
                    &mut output,
                    &json!({"type":"response_item","timestamp":event.timestamp,"payload":payload,"_mmr":meta}),
                )?;
            }
        } else {
            let blocks: Vec<Value> = event
                .blocks
                .iter()
                .enumerate()
                .map(|(index, block)| project_block(block, target, event, index, findings))
                .collect();
            let role = if event.role.as_deref() == Some("tool") {
                Some("user")
            } else {
                event.role.as_deref()
            };
            if role != event.role.as_deref() {
                findings.push(finding(Some(&event.id), "role", "tool_result_role_mapping", "mapped", "Claude readable tool results use the user envelope; the original role remains in canonical history."));
            }
            append_row(
                &mut output,
                &json!({"type":role,"timestamp":event.timestamp,"sessionId":event.session_id,"uuid":event.id,"message":{"role":role,"content":blocks},"_mmr":metadata}),
            )?;
        }
    }
    output.into_string()
}

fn project_block(
    block: &CanonicalBlock,
    target: TargetFormat,
    event: &CanonicalEvent,
    index: usize,
    findings: &mut Vec<ConversionFinding>,
) -> Value {
    match block.kind.as_str() {
        "text" => {
            json!({"type":if target == TargetFormat::CodexReadable { if event.role.as_deref() == Some("assistant") { "output_text" } else { "input_text" } } else { "text" },"text":block.text})
        }
        "tool_call" => {
            json!({"type":"tool_use","id":block.call_id,"name":block.name,"input":claude_arguments(block,event,index,findings),"status":block.status})
        }
        "tool_result" => {
            json!({"type":"tool_result","tool_use_id":block.call_id,"content":block.result,"status":block.status,"is_error":block.is_error})
        }
        _ => {
            findings.push(finding(Some(&event.id), &format!("blocks[{index}]"), "opaque_target_block", "archive_only", "Attachment or opaque block has no assured target semantic mapping; source content is retained in an explicit readable placeholder and canonical archive."));
            json!({"type":"mmr_opaque","source_kind":block.kind,"source_block":block.source})
        }
    }
}

fn codex_arguments(block: &CanonicalBlock) -> Value {
    match &block.arguments {
        Some(Value::String(value)) => Value::String(value.clone()),
        Some(value) => Value::String(value.to_string()),
        None => Value::Null,
    }
}

fn claude_arguments(
    block: &CanonicalBlock,
    event: &CanonicalEvent,
    index: usize,
    findings: &mut Vec<ConversionFinding>,
) -> Value {
    match &block.arguments {
        Some(Value::String(value)) => match serde_json::from_str::<Value>(value) {
            Ok(parsed) if parsed.is_object() => {
                findings.push(finding(Some(&event.id), &format!("blocks[{index}].arguments"), "arguments_container_mapping", "mapped", "JSON argument text is mapped to the Claude readable object container; original argument text remains archived."));
                parsed
            }
            _ => {
                findings.push(finding(Some(&event.id), &format!("blocks[{index}].arguments"), "arguments_not_object", "opaque", "Arguments are not a JSON object; raw content is retained and no API replay compatibility is claimed."));
                Value::String(value.clone())
            }
        },
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

fn append_row(output: &mut BoundedWriter, row: &Value) -> Result<()> {
    serde_json::to_writer(&mut *output, row)?;
    output.write_all(b"\n")?;
    Ok(())
}
