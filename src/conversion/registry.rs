//! Explicit registration of trusted, in-process Rust adapters; this is not a sandbox.

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::Value;

use super::{CanonicalBlock, NativeSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterCapabilities {
    pub readable_history: bool,
    pub native_resume: bool,
    pub api_replay: bool,
    /// Unsupported canonical kinds (`text`, `tool_call`, `tool_result`, `attachment`)
    /// or provider-specific categories, which the adapter must retain as `opaque`.
    /// Returning an interpreted block matching a declared category is rejected.
    pub unsupported_content: Vec<String>,
}

impl AdapterCapabilities {
    pub fn readable_only(unsupported_content: Vec<String>) -> Self {
        Self {
            readable_history: true,
            native_resume: false,
            api_replay: false,
            unsupported_content,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterDescriptor {
    pub source_id: String,
    pub version: String,
    pub capabilities: AdapterCapabilities,
}

/// Source-specific mapping only. The engine supplies immutable provenance and correlations.
/// Use `Some(Value::Null)` for a present null and `None` for an absent JSON value.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RecordProjection {
    /// None leaves the current session/branch context unchanged.
    pub session_id: Option<String>,
    pub branch_id: Option<String>,
    pub source_event_id: Option<String>,
    pub parent_id: Option<String>,
    pub record_type: Option<String>,
    pub role: Option<String>,
    pub source_role: Option<String>,
    pub timestamp: Option<Value>,
    pub blocks: Vec<CanonicalBlock>,
    pub findings: Vec<AdapterFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterFinding {
    pub field: String,
    pub code: String,
    pub disposition: String,
    pub message: String,
}

/// Implementations are trusted Rust code with the caller's privileges, not isolated plugins.
/// A mapping/capability change must use a new descriptor version for replay consistency.
pub trait HistoryAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;
    fn project_record(&self, record: &Value) -> Result<RecordProjection>;
}

struct RegisteredAdapter {
    descriptor: AdapterDescriptor,
    adapter: Box<dyn HistoryAdapter>,
}

/// No discovery or implicit loading: every adapter must be supplied by trusted caller code.
pub struct AdapterRegistry {
    entries: BTreeMap<String, RegisteredAdapter>,
}

impl AdapterRegistry {
    /// Empty, so absence can be tested and applications can restrict their supported sources.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry
            .register(super::adapters::CodexHistoryAdapter)
            .expect("static Codex descriptor is valid");
        registry
            .register(super::adapters::ClaudeHistoryAdapter)
            .expect("static Claude descriptor is valid");
        registry
    }

    pub fn register<A: HistoryAdapter + 'static>(&mut self, adapter: A) -> Result<()> {
        self.register_boxed(Box::new(adapter))
    }

    pub fn register_boxed(&mut self, adapter: Box<dyn HistoryAdapter>) -> Result<()> {
        let descriptor = adapter.descriptor();
        NativeSource::from_id(&descriptor.source_id)?;
        ensure!(
            valid_identifier(&descriptor.version),
            "adapter version must be a 1–64 byte identifier"
        );
        ensure!(
            !descriptor.capabilities.native_resume && !descriptor.capabilities.api_replay,
            "history adapters cannot declare native resumption or API replay capabilities"
        );
        ensure!(
            descriptor.capabilities.unsupported_content.len() <= 64
                && descriptor
                    .capabilities
                    .unsupported_content
                    .iter()
                    .all(|item| valid_identifier(item)),
            "adapter unsupported-content declarations exceed identifier/count limits"
        );
        ensure!(
            !self.entries.contains_key(&descriptor.source_id),
            "duplicate history adapter source registration"
        );
        self.entries.insert(
            descriptor.source_id.clone(),
            RegisteredAdapter {
                descriptor,
                adapter,
            },
        );
        Ok(())
    }

    pub fn descriptor(&self, source_id: &str) -> Option<&AdapterDescriptor> {
        self.entries.get(source_id).map(|entry| &entry.descriptor)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &AdapterDescriptor> {
        self.entries.values().map(|entry| &entry.descriptor)
    }

    pub(super) fn resolve(
        &self,
        source_id: &str,
    ) -> Result<(&AdapterDescriptor, &dyn HistoryAdapter)> {
        let entry = self
            .entries
            .get(source_id)
            .ok_or_else(|| anyhow::anyhow!("history adapter is not registered"))?;
        Ok((&entry.descriptor, &*entry.adapter))
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

pub(super) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Registry-aware native input selection. Unlike CLI InputFormat, this can name a custom source.
#[derive(Debug, Clone, Copy)]
pub enum RegisteredInput<'a> {
    Native(&'a str),
    Canonical,
}
