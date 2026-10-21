//! Explicit, local-file history conversion. Imported content is data, never instructions.
//!
//! Native archives are authoritative. Readable exports are not provider API requests or
//! resumable harness sessions. This module performs no history discovery or persistence.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use clap::{Args, Subcommand};
use serde_json::Value;

use crate::store::content_hash;

mod adapters;
mod model;
mod projection;
mod registry;

use adapters::opaque;
pub use adapters::{ClaudeHistoryAdapter, CodexHistoryAdapter};
pub use model::{
    AttachmentRef, CanonicalBlock, CanonicalEvent, CanonicalSession, ConversionCapabilities,
    ConversionFinding, ConversionResponse, InputFormat, NativeArchive, NativeSource, RawRecordRef,
    TargetFormat, ToolRelationship,
};
use projection::project_readable;
pub use registry::{
    AdapterCapabilities, AdapterDescriptor, AdapterFinding, AdapterRegistry, HistoryAdapter,
    RecordProjection, RegisteredInput,
};

const SCHEMA_VERSION: u32 = 1;
const PARSER_VERSION: &str = "mmr-session-v1";
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
// main adds a newline; reserve it so every successful stdout artifact fits the input budget.
const MAX_OUTPUT_BYTES: usize = MAX_INPUT_BYTES - 1;
const MAX_EVENTS: usize = 40_000;
const MAX_BLOCKS: usize = 100_000;
const MAX_SOURCE_DEPTH: usize = 64;
const MAX_IDENTIFIER_BYTES: usize = 1024;

#[derive(Args, Debug)]
pub struct ConvertArgs {
    #[command(subcommand)]
    command: ConvertCommand,
}

#[derive(Subcommand, Debug)]
enum ConvertCommand {
    /// Convert one explicit local file; never installs or resumes a harness session
    Session(ConvertSessionArgs),
}

#[derive(Args, Debug)]
struct ConvertSessionArgs {
    input: PathBuf,
    #[arg(long, value_enum)]
    from: InputFormat,
    #[arg(long, value_enum)]
    to: TargetFormat,
    /// Stable source-host namespace, required for native input
    #[arg(long)]
    origin: Option<String>,
}

pub fn command_response(args: &ConvertArgs, pretty: bool) -> Result<String> {
    let ConvertCommand::Session(args) = &args.command;
    ensure!(
        std::fs::metadata(&args.input)
            .context("inspect explicit session conversion input")?
            .is_file(),
        "session conversion input must be a regular file"
    );
    let file = File::open(&args.input).context("open explicit session conversion input")?;
    let mut bytes = Vec::new();
    file.take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("read session conversion input")?;
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "session conversion input exceeds byte limit ({MAX_INPUT_BYTES})"
    );
    let input = String::from_utf8(bytes).context("session conversion input must be UTF-8")?;
    let response = convert_text(&input, args.from, args.to, args.origin.as_deref())?;
    check_response_budget(&response, pretty)?;
    let output = if pretty {
        serde_json::to_string_pretty(&response)?
    } else {
        serde_json::to_string(&response)?
    };
    ensure!(
        output.len() <= MAX_OUTPUT_BYTES,
        "session conversion output exceeds roundtrip byte limit ({MAX_INPUT_BYTES})"
    );
    Ok(output)
}

pub fn convert_text(
    input: &str,
    from: InputFormat,
    target: TargetFormat,
    origin: Option<&str>,
) -> Result<ConversionResponse> {
    let from = match from {
        InputFormat::Codex => RegisteredInput::Native("codex"),
        InputFormat::Claude => RegisteredInput::Native("claude"),
        InputFormat::Canonical => RegisteredInput::Canonical,
    };
    convert_with_registry(
        input,
        from,
        target,
        origin,
        &AdapterRegistry::with_builtins(),
    )
}

/// Convert with an explicitly supplied trusted registry. The default CLI never loads extensions.
pub fn convert_with_registry(
    input: &str,
    from: RegisteredInput<'_>,
    target: TargetFormat,
    origin: Option<&str>,
    registry: &AdapterRegistry,
) -> Result<ConversionResponse> {
    ensure!(
        input.len() <= MAX_INPUT_BYTES,
        "session conversion input exceeds byte limit ({MAX_INPUT_BYTES})"
    );
    let mut supplied_response = None;
    let canonical = match from {
        RegisteredInput::Native(source_id) => {
            let origin = origin.ok_or_else(|| {
                anyhow::anyhow!("native session conversion requires --origin <stable-label>")
            })?;
            parse_registered_native(input, source_id, origin, registry)?
        }
        RegisteredInput::Canonical => {
            let value: Value =
                serde_json::from_str(input).context("parse canonical conversion input")?;
            let canonical: CanonicalSession = if value.get("canonical").is_some() {
                // Validate the full envelope too, so unknown fields cannot disappear silently.
                let response = serde_json::from_value::<ConversionResponse>(value)
                    .context("parse canonical conversion response")?;
                let canonical = response.canonical.clone();
                supplied_response = Some(response);
                canonical
            } else {
                serde_json::from_value(value).context("parse canonical session")?
            };
            ensure!(
                canonical.schema_version == SCHEMA_VERSION,
                "unsupported canonical schema or parser version"
            );
            let (descriptor, _) = registry.resolve(canonical.source.as_str())?;
            ensure!(
                canonical.parser_version == descriptor.version,
                "canonical adapter version does not match registered adapter"
            );
            if let Some(origin) = origin {
                ensure!(
                    origin == canonical.origin,
                    "canonical origin cannot be changed on reimport"
                );
            }
            ensure!(
                content_hash(&canonical.archive.text) == canonical.archive.sha256,
                "canonical archive hash mismatch"
            );
            let rebuilt = parse_registered_native(
                &canonical.archive.text,
                canonical.source.as_str(),
                &canonical.origin,
                registry,
            )?;
            ensure!(
                canonical == rebuilt,
                "canonical projection does not match archived source"
            );
            canonical
        }
    };
    if let Some(response) = supplied_response {
        let rebuilt_response = build_response(canonical.clone(), response.target, registry)?;
        ensure!(
            response == rebuilt_response,
            "conversion response does not match its archived source"
        );
    }
    let response = build_response(canonical, target, registry)?;
    check_response_budget(&response, false)?;
    Ok(response)
}

fn build_response(
    canonical: CanonicalSession,
    target: TargetFormat,
    registry: &AdapterRegistry,
) -> Result<ConversionResponse> {
    let (descriptor, _) = registry.resolve(canonical.source.as_str())?;
    ensure!(
        target == TargetFormat::Canonical || descriptor.capabilities.readable_history,
        "registered adapter does not support readable history projection"
    );
    let mut findings = canonical.findings.clone();
    let output_jsonl = if target == TargetFormat::Canonical {
        None
    } else {
        findings.push(finding(None, "capabilities", "readable_only", "unsupported", "Readable target output is not a resumable native session or a validated provider API replay."));
        Some(project_readable(&canonical, target, &mut findings)?)
    };
    Ok(ConversionResponse {
        command: "convert/session".into(),
        target,
        capabilities: ConversionCapabilities {
            native_archive: true,
            readable_history: descriptor.capabilities.readable_history,
            native_resume: false,
            api_replay: false,
        },
        canonical,
        output_jsonl,
        findings,
    })
}

pub fn parse_native(input: &str, source: NativeSource, origin: &str) -> Result<CanonicalSession> {
    parse_registered_native(
        input,
        source.as_str(),
        origin,
        &AdapterRegistry::with_builtins(),
    )
}

pub fn parse_registered_native(
    input: &str,
    source_id: &str,
    origin: &str,
    registry: &AdapterRegistry,
) -> Result<CanonicalSession> {
    let source = NativeSource::from_id(source_id)?;
    let (descriptor, adapter) = registry.resolve(source_id)?;
    ensure!(
        input.len() <= MAX_INPUT_BYTES,
        "session conversion input exceeds byte limit ({MAX_INPUT_BYTES})"
    );
    ensure!(
        !origin.trim().is_empty() && origin.len() <= 256,
        "origin must contain 1 to 256 bytes"
    );
    let mut session = CanonicalSession {
        schema_version: SCHEMA_VERSION,
        parser_version: descriptor.version.clone(),
        source: source.clone(),
        origin: origin.into(),
        archive: NativeArchive {
            sha256: content_hash(input),
            text: input.into(),
        },
        events: Vec::new(),
        findings: Vec::new(),
    };
    for category in &descriptor.capabilities.unsupported_content {
        session.findings.push(finding(
            None,
            "adapter.unsupported_content",
            "adapter_unsupported_content",
            "opaque",
            &format!("Registered adapter retains this category without interpretation: {category}"),
        ));
    }
    let mut materialization_budget = BoundedWriter::counting();
    serde_json::to_writer(&mut materialization_budget, &session.archive)
        .context("archive materialization byte limit")?;
    serde_json::to_writer(&mut materialization_budget, &session.findings)
        .context("adapter findings byte limit")?;
    let mut offset = 0;
    let mut current_session = None;
    let mut branches: BTreeMap<Option<String>, String> = BTreeMap::new();
    let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
    let mut block_count = 0usize;
    for (index, line) in input.split_inclusive('\n').enumerate() {
        ensure!(
            index < MAX_EVENTS,
            "session conversion event limit exceeded ({MAX_EVENTS})"
        );
        let end = offset + line.len();
        let parsed = serde_json::from_str::<Value>(line);
        let mut event = CanonicalEvent {
            id: String::new(),
            sequence: index,
            source_event_id: None,
            session_id: None,
            branch_id: None,
            parent_id: None,
            record_type: None,
            role: None,
            source_role: None,
            timestamp: None,
            raw: RawRecordRef {
                start: offset,
                end,
                sha256: content_hash(line),
            },
            blocks: Vec::new(),
        };
        let first_finding = session.findings.len();
        let mut adapter_findings = Vec::new();
        match parsed {
            Ok(value) if exceeds_source_depth(&value, 0) => {
                event.session_id = current_session.clone();
                event.branch_id = branches.get(&current_session).cloned();
                event.record_type = Some("opaque_depth_limit".into());
                event.blocks.push(opaque(Value::String(line.into())));
            }
            Ok(value) => {
                let projection = adapter.project_record(&value)?;
                validate_projection(&projection, &descriptor.capabilities)?;
                if projection.session_id.is_some() {
                    current_session = projection.session_id;
                }
                event.session_id = current_session.clone();
                if let Some(branch) = projection.branch_id {
                    branches.insert(current_session.clone(), branch);
                }
                event.branch_id = branches.get(&current_session).cloned();
                event.source_event_id = projection.source_event_id;
                event.parent_id = projection.parent_id;
                event.record_type = projection.record_type;
                event.timestamp = projection.timestamp;
                event.role = projection.role;
                event.source_role = projection.source_role;
                event.blocks = projection.blocks;
                adapter_findings = projection.findings;
            }
            Err(_) if line.trim().is_empty() => {
                event.record_type = Some("blank".into());
            }
            Err(_) => {
                event.session_id = current_session.clone();
                event.branch_id = branches.get(&current_session).cloned();
                event.record_type = Some("malformed".into());
                event.blocks.push(opaque(Value::String(line.into())));
            }
        }
        block_count += event.blocks.len();
        ensure!(
            block_count <= MAX_BLOCKS,
            "session conversion block limit exceeded ({MAX_BLOCKS})"
        );
        let source_identity = event
            .source_event_id
            .clone()
            .unwrap_or_else(|| content_hash(line.trim_end_matches(['\r', '\n'])));
        let identity = serde_json::to_string(&(
            origin,
            &source,
            &event.session_id,
            &event.branch_id,
            &event.record_type,
            source_identity,
        ))?;
        let occurrence = occurrences.entry(identity.clone()).or_default();
        event.id = format!(
            "session-event:v1:{}",
            content_hash(&format!("{identity}:{occurrence}"))
        );
        *occurrence += 1;
        if *occurrence > 1 {
            session.findings.push(finding(
                Some(&event.id),
                "source_event_id",
                "duplicate_source_identity",
                "mapped",
                "Repeated source identity retained as a separate ordered occurrence.",
            ));
        }
        if event.session_id.is_none() {
            session.findings.push(finding(Some(&event.id), "session_id", "missing_session_identity", "unavailable", "Source does not supply a session identity; relationships are scoped to the explicit origin and unknown-session partition."));
        }
        if event.record_type.as_deref() == Some("malformed") {
            let code = if end == input.len() && !line.ends_with('\n') {
                "truncated_tail"
            } else {
                "malformed_record"
            };
            session.findings.push(finding(
                Some(&event.id),
                "raw",
                code,
                "opaque",
                "Unparseable source bytes retained as an opaque archival event.",
            ));
        }
        if event.record_type.as_deref() == Some("opaque_depth_limit") {
            session.findings.push(finding(Some(&event.id), "raw", "source_depth_limit", "opaque", "Source nesting exceeds the projection depth limit; complete source bytes remain archived as opaque data."));
        }
        for (block_index, block) in event.blocks.iter().enumerate() {
            if block.kind == "opaque" && event.record_type.as_deref() != Some("malformed") {
                session.findings.push(finding(
                    Some(&event.id),
                    &format!("blocks[{block_index}]"),
                    "opaque_block",
                    "opaque",
                    "Unrecognized source content is retained without interpretation.",
                ));
            }
            if let Some(attachment) = &block.attachment {
                if attachment.content_hash.is_none() {
                    session.findings.push(finding(Some(&event.id), &format!("blocks[{block_index}].attachment.content_hash"), "attachment_hash_unavailable", "unavailable", "No attachment content hash was supplied; referenced bytes were not fetched."));
                }
                if attachment.availability == "embedded_unverified" {
                    session.findings.push(finding(Some(&event.id), &format!("blocks[{block_index}].attachment"), "attachment_payload_unverified", "opaque", "Encoded source payload is archived but is not decoded, fetched, or verified as attachment bytes."));
                } else {
                    session.findings.push(finding(Some(&event.id), &format!("blocks[{block_index}].attachment"), "attachment_bytes_unavailable", "unavailable", "Only an attachment reference is archived; external bytes are not available in this artifact."));
                }
            }
        }
        for item in adapter_findings {
            session.findings.push(finding(
                Some(&event.id),
                &item.field,
                &item.code,
                &item.disposition,
                &item.message,
            ));
        }
        serde_json::to_writer(&mut materialization_budget, &event)
            .context("adapter event materialization byte limit")?;
        serde_json::to_writer(
            &mut materialization_budget,
            &session.findings[first_finding..],
        )
        .context("adapter findings materialization byte limit")?;
        session.events.push(event);
        offset = end;
    }
    resolve_relationships(&mut session);
    Ok(session)
}

fn validate_projection(
    projection: &RecordProjection,
    capabilities: &AdapterCapabilities,
) -> Result<()> {
    for value in [
        &projection.session_id,
        &projection.branch_id,
        &projection.source_event_id,
        &projection.parent_id,
        &projection.record_type,
        &projection.role,
        &projection.source_role,
    ] {
        ensure!(
            value
                .as_ref()
                .is_none_or(|s| s.len() <= MAX_IDENTIFIER_BYTES),
            "source identifier exceeds byte limit ({MAX_IDENTIFIER_BYTES})"
        );
    }
    ensure!(
        projection.blocks.len() <= MAX_BLOCKS,
        "session conversion block limit exceeded ({MAX_BLOCKS})"
    );
    ensure!(
        projection.findings.len() <= 64,
        "adapter per-record finding limit exceeded"
    );
    for item in &projection.findings {
        ensure!(
            item.field.len() <= 1024
                && item.code.len() <= 64
                && item.disposition.len() <= 64
                && item.message.len() <= 4096,
            "adapter finding field limit exceeded"
        );
    }
    if let Some(value) = &projection.timestamp {
        ensure!(
            !exceeds_source_depth(value, 0),
            "adapter timestamp depth limit exceeded"
        );
    }
    for block in &projection.blocks {
        ensure!(
            matches!(
                block.kind.as_str(),
                "text" | "tool_call" | "tool_result" | "attachment" | "opaque"
            ),
            "adapter returned an unknown canonical block kind; retain it as opaque"
        );
        ensure!(
            block.kind == "opaque" || !capabilities.unsupported_content.contains(&block.kind),
            "adapter interpreted a content kind declared unsupported"
        );
        if let Some(attachment) = &block.attachment {
            ensure!(
                matches!(
                    attachment.availability.as_str(),
                    "reference_only" | "embedded_unverified"
                ),
                "adapter attachment availability must be reference_only or embedded_unverified"
            );
        }
        ensure!(
            block.relationship.is_none(),
            "adapters cannot supply engine-owned tool relationships"
        );
        for value in [
            Some(&block.source),
            block.arguments.as_ref(),
            block.result.as_ref(),
            block.status.as_ref(),
            block.is_error.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                !exceeds_source_depth(value, 0),
                "adapter block depth limit exceeded"
            );
        }
    }
    // Check returned data before cloning inherited context or serializing identity material.
    // Work performed inside the trusted adapter callback itself is outside this budget.
    serde_json::to_writer(&mut BoundedWriter::counting(), projection)
        .context("adapter projection materialization byte limit")?;
    Ok(())
}

fn exceeds_source_depth(value: &Value, depth: usize) -> bool {
    if depth > MAX_SOURCE_DEPTH {
        return true;
    }
    match value {
        Value::Array(items) => items.iter().any(|v| exceeds_source_depth(v, depth + 1)),
        Value::Object(items) => items.values().any(|v| exceeds_source_depth(v, depth + 1)),
        _ => false,
    }
}

fn resolve_relationships(session: &mut CanonicalSession) {
    type Key = (Option<String>, Option<String>, String);
    #[derive(Default)]
    struct Candidates {
        count: usize,
        unique: Option<String>,
    }
    let mut calls: BTreeMap<Key, Candidates> = BTreeMap::new();
    let mut results: BTreeMap<Key, Candidates> = BTreeMap::new();
    for event in &session.events {
        for (index, block) in event.blocks.iter().enumerate() {
            if let Some(id) = &block.call_id {
                let key = (
                    event.session_id.clone(),
                    event.branch_id.clone(),
                    id.clone(),
                );
                let table = match block.kind.as_str() {
                    "tool_call" => &mut calls,
                    "tool_result" => &mut results,
                    _ => continue,
                };
                let candidates = table.entry(key).or_default();
                candidates.count += 1;
                candidates.unique = if candidates.count == 1 {
                    Some(format!("{}#block/{index}", event.id))
                } else {
                    None
                };
            }
        }
    }
    for event in &mut session.events {
        for (index, block) in event.blocks.iter_mut().enumerate() {
            if !matches!(block.kind.as_str(), "tool_call" | "tool_result") {
                continue;
            }
            let key = block.call_id.as_ref().map(|id| {
                (
                    event.session_id.clone(),
                    event.branch_id.clone(),
                    id.clone(),
                )
            });
            let call = key.as_ref().and_then(|key| calls.get(key));
            let result = key.as_ref().and_then(|key| results.get(key));
            let call_count = call.map_or(0, |c| c.count);
            let result_count = result.map_or(0, |c| c.count);
            let counterpart = if block.kind == "tool_call" {
                result
            } else {
                call
            };
            let status = if block.call_id.is_none() {
                "missing_id"
            } else if event.session_id.is_none() {
                "unknown_session"
            } else if call_count > 1 || result_count > 1 {
                "ambiguous"
            } else if counterpart.is_none() {
                if block.kind == "tool_call" {
                    "pending"
                } else {
                    "orphan"
                }
            } else {
                "matched"
            };
            // Ambiguous relationships retain counts, not a quadratic list of possible edges.
            let matches = if status == "matched" {
                counterpart
                    .and_then(|c| c.unique.clone())
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            };
            if status != "matched" {
                session.findings.push(finding(Some(&event.id), &format!("blocks[{index}].relationship"), status, "unavailable", "Tool correlation is unresolved or ambiguous; no result or relationship was invented."));
            }
            block.relationship = Some(ToolRelationship {
                status: status.into(),
                matches,
                call_count,
                result_count,
            });
        }
    }
}

/// Both the counting pass and readable-artifact buffer fail before exceeding the byte budget.
struct BoundedWriter {
    bytes: Option<Vec<u8>>,
    used: usize,
}
impl BoundedWriter {
    fn counting() -> Self {
        Self {
            bytes: None,
            used: 0,
        }
    }
    fn buffer() -> Self {
        Self {
            bytes: Some(Vec::new()),
            used: 0,
        }
    }
    fn into_string(self) -> Result<String> {
        Ok(String::from_utf8(self.bytes.unwrap_or_default())?)
    }
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_OUTPUT_BYTES - self.used {
            return Err(std::io::Error::other(
                "session conversion output exceeds roundtrip byte limit",
            ));
        }
        if let Some(buffer) = &mut self.bytes {
            buffer.extend_from_slice(bytes);
        }
        self.used += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn check_response_budget(response: &ConversionResponse, pretty: bool) -> Result<()> {
    let mut writer = BoundedWriter::counting();
    if pretty {
        serde_json::to_writer_pretty(&mut writer, response)
    } else {
        serde_json::to_writer(&mut writer, response)
    }
    .context("check session conversion output byte limit")
}

fn finding(
    event_id: Option<&str>,
    field: &str,
    code: &str,
    disposition: &str,
    message: &str,
) -> ConversionFinding {
    ConversionFinding {
        event_id: event_id.map(str::to_owned),
        field: field.into(),
        code: code.into(),
        disposition: disposition.into(),
        message: message.into(),
    }
}
