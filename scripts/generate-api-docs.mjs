#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const outDir = path.resolve("docs/api");
const openapiPath = path.join(outDir, "openapi.json");
const htmlPath = path.join(outDir, "index.html");
const readmePath = path.join(outDir, "README.md");

const ref = (name) => ({ $ref: `#/components/schemas/${name}` });
const arrayOf = (schema) => ({ type: "array", items: schema });
const stringEnum = (values) => ({ type: "string", enum: values });
const nullableString = (extra = {}) => ({ type: ["string", "null"], ...extra });
const int = (extra = {}) => ({ type: "integer", minimum: 0, ...extra });
const bool = () => ({ type: "boolean" });
const mapOf = (schema) => ({ type: "object", additionalProperties: schema });

const sourceSchema = stringEnum(["claude", "codex", "cursor", "grok", "pi"]);
const timestampSchema = { type: "string", format: "date-time" };

const param = (name, schema, description, required = false) => ({
  name,
  in: "query",
  required,
  description,
  schema,
});

const pathParam = (name, schema, description) => ({
  name,
  in: "path",
  required: true,
  description,
  schema,
});

const jsonContent = (schema) => ({
  "application/json": {
    schema,
  },
});

const ok = (schema, description = "Successful response") => ({
  description,
  content: jsonContent(schema),
});

const errorResponses = {
  "400": ok(ref("MmrError"), "Invalid request or unsupported option combination"),
  "401": ok(ref("MmrError"), "Missing or invalid API token"),
  "404": ok(ref("MmrError"), "Requested project, session, event, or bundle was not found"),
  "409": ok(ref("MmrError"), "Ambiguous selector, stale continuation, or conflicting state"),
  "422": ok(ref("MmrError"), "Request is understood but blocked by privacy or safety policy"),
  "502": ok(ref("MmrError"), "Named peer, remote provider, or upstream model call failed"),
  "504": ok(ref("MmrError"), "Named peer or upstream model call timed out"),
};

const operation = ({
  tag,
  operationId,
  summary,
  description,
  parameters = [],
  request,
  response,
  responseDescription,
  extensions = {},
}) => {
  const op = {
    tags: [tag],
    operationId,
    summary,
    description,
    "x-mmr-status": "rest-contract",
    ...extensions,
    responses: {
      "200": ok(response, responseDescription),
      ...errorResponses,
    },
  };
  if (parameters.length > 0) {
    op.parameters = parameters;
  }
  if (request) {
    op.requestBody = {
      required: true,
      content: jsonContent(request),
    };
  }
  return op;
};

const commonParameters = {
  source: param(
    "source",
    sourceSchema,
    "Provider source filter. Omitting it means all provider sources."
  ),
  sourceRequired: param(
    "source",
    sourceSchema,
    "Required provider source filter.",
    true
  ),
  project: param(
    "project",
    { type: "string" },
    "Project path, provider project name, or known project alias."
  ),
  all: param(
    "all",
    bool(),
    "Search across all projects instead of cwd or explicit project scope."
  ),
  remote: param(
    "remote",
    { type: "array", items: { type: "string" } },
    "Explicit trusted SSH peer target. Repeat to merge multiple named peers."
  ),
  limit: param("limit", int(), "Maximum number of returned items."),
  offset: param("offset", int(), "Number of sorted items to skip before returning a page."),
  sortBy: param("sort_by", ref("SortBy"), "Sort key."),
  order: param("order", ref("SortOrder"), "Sort order."),
  format: param("format", stringEnum(["json"]), "REST responses are JSON. CLI-only tree and line formats are excluded."),
};

const fixed = (required, properties) => ({
  type: "object",
  additionalProperties: false,
  required,
  properties,
});

const schemas = {
  ProviderSource: sourceSchema,
  EventSource: {
    type: "string",
    description: "Provider or normalized event source such as claude, codex, cursor, grok, pi, note, or learned_memory.",
  },
  SortBy: stringEnum(["timestamp", "message-count"]),
  SortOrder: stringEnum(["asc", "desc"]),
  MmrError: {
    type: "object",
    additionalProperties: true,
    required: ["status", "error_kind", "message"],
    properties: {
      status: { type: "string", const: "failed" },
      error_kind: { type: "string" },
      message: { type: "string" },
      command: nullableString(),
      host: nullableString(),
      total_sessions_in_scope: { type: ["integer", "null"], minimum: 0 },
      max_selectable_age: { type: ["integer", "null"], minimum: 0 },
      requested_age: { type: ["integer", "null"], minimum: 0 },
      requested_newest_age: { type: ["integer", "null"], minimum: 0 },
      requested_oldest_age: { type: ["integer", "null"], minimum: 0 },
      details: { type: ["object", "null"], additionalProperties: true },
    },
  },
  RestContinuationRequest: fixed(["method", "path"], {
    method: stringEnum(["GET", "POST"]),
    path: { type: "string" },
    query: {
      type: "object",
      additionalProperties: {
        oneOf: [
          { type: "string" },
          { type: "number" },
          { type: "boolean" },
          { type: "array", items: { type: "string" } },
          { type: "null" },
        ],
      },
    },
    body: { type: ["object", "null"], additionalProperties: true },
  }),
  ApiMessageOrigin: fixed(["host", "transport"], {
    host: { type: "string" },
    transport: { type: "string", examples: ["ssh", "local"] },
    remote_mmr_version: nullableString(),
  }),
  ApiPeerResult: fixed(["host", "transport", "command", "status"], {
    host: { type: "string" },
    transport: { type: "string" },
    command: { type: "string" },
    status: { type: "string" },
    remote_mmr_version: nullableString(),
    total_messages: { type: ["integer", "null"], minimum: 0, format: "int64" },
    total_sessions: { type: ["integer", "null"], minimum: 0, format: "int64" },
  }),
  ApiProject: fixed(
    ["name", "source", "original_path", "session_count", "message_count", "last_activity"],
    {
      name: { type: "string" },
      source: ref("EventSource"),
      original_path: { type: "string" },
      aliases: arrayOf({ type: "string" }),
      session_count: int(),
      message_count: int(),
      last_activity: timestampSchema,
      origin: ref("ApiMessageOrigin"),
    }
  ),
  ApiProjectsResponse: fixed(["projects", "total_messages", "total_sessions"], {
    projects: arrayOf(ref("ApiProject")),
    total_messages: int({ format: "int64" }),
    total_sessions: int({ format: "int64" }),
    peer_results: arrayOf(ref("ApiPeerResult")),
  }),
  ApiSession: fixed(
    [
      "session_id",
      "source",
      "project_name",
      "project_path",
      "first_timestamp",
      "last_timestamp",
      "message_count",
      "user_messages",
      "assistant_messages",
      "preview",
    ],
    {
      session_id: { type: "string" },
      source: ref("EventSource"),
      project_name: { type: "string" },
      project_path: { type: "string" },
      first_timestamp: timestampSchema,
      last_timestamp: timestampSchema,
      message_count: int(),
      user_messages: int(),
      assistant_messages: int(),
      preview: { type: "string" },
      origin: ref("ApiMessageOrigin"),
    }
  ),
  ApiSessionsResponse: fixed(["sessions", "total_sessions"], {
    sessions: arrayOf(ref("ApiSession")),
    total_sessions: int({ format: "int64" }),
    peer_results: arrayOf(ref("ApiPeerResult")),
  }),
  ApiMessage: fixed(
    [
      "session_id",
      "source",
      "project_name",
      "role",
      "content",
      "model",
      "timestamp",
      "is_subagent",
      "msg_type",
      "input_tokens",
      "output_tokens",
    ],
    {
      session_id: { type: "string" },
      source: ref("EventSource"),
      project_name: { type: "string" },
      role: { type: "string" },
      content: { type: "string" },
      model: { type: "string" },
      timestamp: timestampSchema,
      is_subagent: bool(),
      msg_type: { type: "string" },
      input_tokens: int({ format: "int64" }),
      output_tokens: int({ format: "int64" }),
      origin: ref("ApiMessageOrigin"),
    }
  ),
  SessionSelectionScope: fixed(["project", "all", "source"], {
    project: nullableString(),
    all: bool(),
    source: nullableString(),
  }),
  SelectedSession: fixed(
    [
      "age",
      "session_id",
      "source",
      "project_name",
      "first_timestamp",
      "last_timestamp",
      "message_count",
      "equivalent_command",
    ],
    {
      age: int(),
      session_id: { type: "string" },
      source: ref("ProviderSource"),
      project_name: { type: "string" },
      first_timestamp: timestampSchema,
      last_timestamp: timestampSchema,
      message_count: int(),
      equivalent_command: { type: "string" },
    }
  ),
  SkippedNewest: fixed(["age", "session_id", "last_timestamp", "assumed_live"], {
    age: int(),
    session_id: { type: "string" },
    last_timestamp: timestampSchema,
    assumed_live: bool(),
  }),
  SessionSelection: fixed(["scope", "axis", "total_sessions_in_scope", "selected"], {
    scope: ref("SessionSelectionScope"),
    axis: stringEnum(["session-back"]),
    total_sessions_in_scope: int({ format: "int64" }),
    selected: arrayOf(ref("SelectedSession")),
    skipped_newest: ref("SkippedNewest"),
  }),
  ApiMessagesResponse: fixed(["messages", "total_messages", "next_page", "next_offset"], {
    messages: arrayOf(ref("ApiMessage")),
    total_messages: int({ format: "int64" }),
    next_page: bool(),
    next_offset: int({ format: "int64" }),
    next_command: { type: ["string", "null"] },
    next_url: nullableString(),
    next_request: { oneOf: [ref("RestContinuationRequest"), { type: "null" }] },
    session_selection: ref("SessionSelection"),
    peer_results: arrayOf(ref("ApiPeerResult")),
  }),
  ContextResponse: fixed(
    ["command", "scope", "source", "project", "total_sessions", "total_messages", "sessions", "messages"],
    {
      command: { type: "string", examples: ["context/project"] },
      scope: stringEnum(["project", "source"]),
      source: nullableString(),
      project: nullableString(),
      total_sessions: int({ format: "int64" }),
      total_messages: int({ format: "int64" }),
      sessions: arrayOf(ref("ApiSession")),
      messages: arrayOf(ref("ApiMessage")),
      peer_results: arrayOf(ref("ApiPeerResult")),
    }
  ),
  SearchResult: fixed(
    [
      "project_id",
      "source",
      "session_id",
      "event_id",
      "event_type",
      "role",
      "timestamp",
      "citation",
      "line_number",
      "snippet",
      "before",
      "after",
    ],
    {
      project_id: { type: "string" },
      source: ref("EventSource"),
      session_id: { type: "string" },
      event_id: { type: "string" },
      event_type: { type: "string" },
      role: { type: "string" },
      timestamp: timestampSchema,
      citation: { type: "string", examples: ["mmr://event/event:v1:abc123"] },
      line_number: int(),
      snippet: { type: "string" },
      before: arrayOf({ type: "string" }),
      after: arrayOf({ type: "string" }),
    }
  ),
  SearchResponse: fixed(["query", "total_results", "results"], {
    query: { type: "string" },
    results: arrayOf(ref("SearchResult")),
    total_results: int(),
  }),
  RetrieveLimits: fixed(["max_sessions", "before_messages", "after_messages", "max_messages_per_session", "limit", "offset"], {
    max_sessions: int(),
    before_messages: int(),
    after_messages: int(),
    max_messages_per_session: int(),
    limit: int(),
    offset: int(),
  }),
  RetrieveScope: fixed(["all_projects", "all_sources", "source_filter", "total_projects_searched", "projects"], {
    all_projects: bool(),
    all_sources: bool(),
    source_filter: nullableString(),
    total_projects_searched: int(),
    projects: arrayOf({ type: "string" }),
  }),
  RetrieveDebug: fixed(["limits", "scope", "total_ranked_sessions"], {
    limits: ref("RetrieveLimits"),
    scope: ref("RetrieveScope"),
    total_ranked_sessions: int(),
  }),
  RetrieveSessionIdentity: fixed(["source", "project_name", "source_session_id"], {
    source: ref("ProviderSource"),
    project_name: { type: "string" },
    source_session_id: { type: "string" },
  }),
  RetrieveRankReason: fixed(["match_count", "latest_match_timestamp", "tie_break"], {
    match_count: int(),
    latest_match_timestamp: timestampSchema,
    tie_break: arrayOf({ type: "string" }),
  }),
  RetrieveMessageWindow: fixed(["before_messages", "after_messages", "max_messages_per_session", "truncated"], {
    before_messages: int(),
    after_messages: int(),
    max_messages_per_session: int(),
    truncated: bool(),
  }),
  RetrieveMatch: fixed(
    [
      "source",
      "project_name",
      "source_session_id",
      "event_id",
      "event_type",
      "role",
      "timestamp",
      "citation",
      "line_number",
      "snippet",
      "before",
      "after",
    ],
    {
      source: ref("EventSource"),
      project_name: { type: "string" },
      source_session_id: { type: "string" },
      event_id: { type: "string" },
      event_type: { type: "string" },
      role: { type: "string" },
      timestamp: timestampSchema,
      citation: { type: "string" },
      line_number: int(),
      snippet: { type: "string" },
      before: arrayOf({ type: "string" }),
      after: arrayOf({ type: "string" }),
    }
  ),
  RetrieveUnreadableMatch: fixed(
    [
      "source",
      "project_name",
      "event_id",
      "event_type",
      "role",
      "timestamp",
      "citation",
      "line_number",
      "snippet",
      "before",
      "after",
      "reason",
    ],
    {
      source: ref("EventSource"),
      project_name: { type: "string" },
      source_session_id: nullableString(),
      event_id: { type: "string" },
      event_type: { type: "string" },
      role: { type: "string" },
      timestamp: timestampSchema,
      citation: { type: "string" },
      line_number: int(),
      snippet: { type: "string" },
      before: arrayOf({ type: "string" }),
      after: arrayOf({ type: "string" }),
      reason: { type: "string" },
    }
  ),
  RetrieveSelectedSession: fixed(
    [
      "rank",
      "source",
      "project_name",
      "source_session_id",
      "rank_reason",
      "match_count",
      "first_match_citation",
      "matches",
    ],
    {
      rank: int({ minimum: 1 }),
      source: ref("ProviderSource"),
      project_name: { type: "string" },
      source_session_id: { type: "string" },
      rank_reason: ref("RetrieveRankReason"),
      match_count: int(),
      first_match_citation: { type: "string" },
      matches: arrayOf(ref("RetrieveMatch")),
      message_window: ref("RetrieveMessageWindow"),
      messages: arrayOf(ref("ApiMessage")),
    }
  ),
  RetrieveResponse: fixed(["query", "total_matches", "total_selected_sessions", "selected_sessions", "unreadable_matches", "suggested_next_action"], {
    query: { type: "string" },
    debug: ref("RetrieveDebug"),
    total_matches: int(),
    total_selected_sessions: int(),
    selected_sessions: arrayOf(ref("RetrieveSelectedSession")),
    unreadable_matches: arrayOf(ref("RetrieveUnreadableMatch")),
    next_page: bool(),
    next_offset: int(),
    next_command: { type: ["string", "null"] },
    next_url: nullableString(),
    next_request: { oneOf: [ref("RestContinuationRequest"), { type: "null" }] },
    suggested_next_action: { type: ["string", "null"] },
  }),
  RetrieveRequest: fixed(["query"], {
    query: { type: "string" },
    project: nullableString(),
    all_projects: bool(),
    all_sources: bool(),
    source: ref("ProviderSource"),
    debug: bool(),
    full_message_history: bool(),
    session: nullableString(),
    role: nullableString(),
    event_type: nullableString(),
    ignore_case: bool(),
    context: int(),
    max_sessions: int(),
    before_messages: int(),
    after_messages: int(),
    max_messages_per_session: int(),
    limit: int(),
    offset: int(),
    pinned_sessions: arrayOf(ref("RetrieveSessionIdentity")),
  }),
  RememberResponse: fixed(["backend", "model", "text"], {
    backend: { type: "string" },
    model: { type: "string" },
    text: { type: "string" },
  }),
  SummaryRequest: {
    ...fixed(["scope"], {
      scope: stringEnum(["project", "source", "session"]),
      project: nullableString(),
      session_id: nullableString(),
      source: ref("ProviderSource"),
      limit: int(),
      offset: int(),
      remotes: arrayOf({ type: "string" }),
      instructions: nullableString(),
      output_format: stringEnum(["json", "md"]),
      model: nullableString(),
    }),
    allOf: [
      {
        if: { properties: { scope: { const: "source" } }, required: ["scope"] },
        then: { required: ["source"], properties: { source: ref("ProviderSource") } },
      },
      {
        if: { properties: { scope: { const: "session" } }, required: ["scope"] },
        then: { required: ["session_id"], properties: { session_id: nullableString() } },
      },
    ],
  },
  CompactResponseMessage: fixed(["role", "content"], {
    role: { type: "string" },
    content: { type: "string" },
    compacted_line_ranges: arrayOf(ref("LineRange")),
    kept_line_ranges: arrayOf(ref("LineRange")),
  }),
  LineRange: fixed(["start", "end"], {
    start: int(),
    end: int(),
  }),
  CompactUsage: fixed(["input_tokens", "output_tokens", "compression_ratio", "processing_time_ms"], {
    input_tokens: int(),
    output_tokens: int(),
    compression_ratio: { type: "number", minimum: 0 },
    processing_time_ms: int(),
  }),
  CompactResponse: fixed(["backend", "model", "output", "messages"], {
    backend: { type: "string", const: "morph-compact" },
    model: { type: "string" },
    id: nullableString(),
    output: { type: "string" },
    messages: arrayOf(ref("CompactResponseMessage")),
    usage: ref("CompactUsage"),
  }),
  CompactRequest: {
    ...fixed(["scope"], {
      scope: stringEnum(["project", "source", "session"]),
      project: nullableString(),
      session_id: nullableString(),
      source: ref("ProviderSource"),
      remotes: arrayOf({ type: "string" }),
      query: nullableString(),
      compression_ratio: { type: ["number", "null"], minimum: 0, maximum: 1 },
      preserve_recent: { type: ["integer", "null"], minimum: 0 },
      no_line_ranges: bool(),
      no_markers: bool(),
      output_format: stringEnum(["json", "md"]),
      model: nullableString(),
    }),
    allOf: [
      {
        if: { properties: { scope: { const: "source" } }, required: ["scope"] },
        then: { required: ["source"], properties: { source: ref("ProviderSource") } },
      },
      {
        if: { properties: { scope: { const: "session" } }, required: ["scope"] },
        then: { required: ["session_id"], properties: { session_id: nullableString() } },
      },
    ],
  },
  PiiCoverage: fixed(["status", "detector", "reason"], {
    status: stringEnum(["available", "degraded"]),
    detector: { type: "string" },
    reason: { type: "string" },
  }),
  RedactionSpan: fixed(["kind", "start_byte", "end_byte", "replacement", "confidence", "blocks_sync"], {
    kind: { type: "string" },
    start_byte: int(),
    end_byte: int(),
    replacement: { type: "string" },
    confidence: { type: "number", minimum: 0, maximum: 1 },
    blocks_sync: bool(),
  }),
  RedactedEventSummary: fixed(["event_id", "status", "span_count", "blocking_findings", "kinds"], {
    event_id: { type: "string" },
    status: { type: "string" },
    span_count: int(),
    blocking_findings: int(),
    kinds: arrayOf({ type: "string" }),
  }),
  RedactScanResponse: fixed(["project_id", "policy_id", "events_scanned", "passed", "blocked", "pii_coverage", "events"], {
    project_id: { type: "string" },
    policy_id: { type: "string" },
    events_scanned: int(),
    passed: int(),
    blocked: int(),
    pii_coverage: ref("PiiCoverage"),
    events: arrayOf(ref("RedactedEventSummary")),
  }),
  RedactExplainResponse: fixed(["event_id", "policy_id", "status", "blocking_findings", "spans", "redacted_text"], {
    event_id: { type: "string" },
    policy_id: { type: "string" },
    status: { type: "string" },
    blocking_findings: int({ format: "int64" }),
    spans: arrayOf(ref("RedactionSpan")),
    redacted_text: { type: "string" },
  }),
  RedactScanRequest: fixed([], {
    project: nullableString(),
    source: ref("ProviderSource"),
  }),
  SyncDryRunEvent: fixed(["event_id", "source", "status", "would_sync", "blocked_reasons"], {
    event_id: { type: "string" },
    source: ref("EventSource"),
    status: { type: "string" },
    would_sync: bool(),
    payload_preview: nullableString(),
    blocked_reasons: arrayOf({ type: "string" }),
  }),
  SyncDryRunResponse: fixed(["dry_run", "project_id", "remote", "policy_id", "total_events", "syncable_events", "blocked_events", "pii_coverage", "events"], {
    dry_run: { type: "boolean", const: true },
    project_id: { type: "string" },
    remote: { type: "string" },
    policy_id: { type: "string" },
    total_events: int(),
    syncable_events: int(),
    blocked_events: int(),
    pii_coverage: ref("PiiCoverage"),
    events: arrayOf(ref("SyncDryRunEvent")),
  }),
  RemoteSummary: fixed(["descriptor", "backend", "available", "auth_status", "created"], {
    descriptor: { type: "string" },
    backend: { type: "string" },
    available: bool(),
    auth_status: { type: "string" },
    created: bool(),
  }),
  HydrationReport: fixed(["remote_events", "inserted_events", "existing_events", "remote_learned_memory", "inserted_learned_memory", "existing_learned_memory"], {
    remote_events: int(),
    inserted_events: int(),
    existing_events: int(),
    remote_learned_memory: int(),
    inserted_learned_memory: int(),
    existing_learned_memory: int(),
  }),
  SyncBlockedEvent: fixed(["event_id", "source", "event_type", "reasons"], {
    event_id: { type: "string" },
    source: ref("EventSource"),
    event_type: { type: "string" },
    reasons: arrayOf({ type: "string" }),
  }),
  SyncReport: fixed(
    [
      "status",
      "remote",
      "policy_id",
      "manifest_id",
      "root_hash",
      "total_events",
      "synced_events",
      "uploaded_events",
      "uploaded_search_documents",
      "synced_learned_memory",
      "uploaded_learned_memory",
      "blocked_events",
      "blocked_learned_memory",
      "remote_events",
      "remote_learned_memory",
      "append_only",
      "pii_coverage",
      "blocked",
    ],
    {
      status: { type: "string" },
      remote: ref("RemoteSummary"),
      policy_id: { type: "string" },
      manifest_id: { type: "string" },
      root_hash: { type: "string" },
      total_events: int(),
      synced_events: int(),
      uploaded_events: int(),
      uploaded_search_documents: int(),
      synced_learned_memory: int(),
      uploaded_learned_memory: int(),
      blocked_events: int(),
      blocked_learned_memory: int(),
      remote_events: int(),
      remote_learned_memory: int(),
      append_only: bool(),
      pii_coverage: ref("PiiCoverage"),
      blocked: arrayOf(ref("SyncBlockedEvent")),
    }
  ),
  SyncRequest: fixed([], {
    project: nullableString(),
    source: ref("ProviderSource"),
  }),
  ProjectStatus: fixed(["id", "display_name", "path_hash"], {
    id: { type: "string" },
    display_name: { type: "string" },
    path_hash: { type: "string" },
  }),
  StatusResponse: fixed(["command", "store", "project", "remote", "status", "diagnostics"], {
    command: { type: "string", const: "status" },
    store: ref("StatusStoreDiagnostics"),
    project: { oneOf: [ref("ProjectStatus"), { type: "null" }] },
    remote: ref("RemoteSummary"),
    status: ref("StatusProjectSnapshot"),
    diagnostics: ref("StatusDiagnostics"),
  }),
  StoreStatus: fixed(["schema_version"], {
    schema_version: int({ format: "int64" }),
  }),
  StatusStoreDiagnostics: fixed(["db_path", "exists", "existed_before_command", "schema_version", "expected_schema_version", "schema_status"], {
    db_path: { type: "string" },
    exists: bool(),
    existed_before_command: bool(),
    schema_version: int({ format: "int64" }),
    expected_schema_version: int({ format: "int64" }),
    schema_status: { type: "string" },
  }),
  StatusProjectSnapshot: fixed(["linked", "sync_status", "events_total", "source_counts", "sync_status_counts", "redaction", "sync"], {
    linked: bool(),
    sync_status: { type: "string" },
    events_total: int(),
    source_counts: mapOf(int()),
    sync_status_counts: mapOf(int()),
    redaction: ref("StatusRedactionSnapshot"),
    sync: ref("StatusSyncSnapshot"),
  }),
  StatusRedactionSnapshot: fixed(["policy_id", "redacted_or_synced", "blocked", "pending"], {
    policy_id: { type: "string" },
    redacted_or_synced: int(),
    blocked: int(),
    pending: int(),
  }),
  StatusSyncSnapshot: fixed(["remote_events", "local_manifests", "latest_manifest_id", "blocked_events", "unsynced_events"], {
    remote_events: int(),
    local_manifests: int(),
    latest_manifest_id: nullableString(),
    blocked_events: int(),
    unsynced_events: int(),
  }),
  StatusDiagnostics: fixed(["schema", "remote", "sources", "privacy_filter", "summary_runner", "dream_runner", "actions"], {
    schema: ref("StatusSchemaDiagnostic"),
    remote: ref("StatusRemoteDiagnostic"),
    sources: arrayOf(ref("StatusSourceDiagnostic")),
    privacy_filter: ref("StatusPrivacyDiagnostic"),
    summary_runner: ref("StatusSummaryRunnerDiagnostic"),
    dream_runner: ref("StatusDreamRunnerDiagnostic"),
    actions: arrayOf({ type: "string" }),
  }),
  StatusSchemaDiagnostic: fixed(["status", "current_version", "expected_version"], {
    status: { type: "string" },
    current_version: int({ format: "int64" }),
    expected_version: int({ format: "int64" }),
    action: nullableString(),
  }),
  StatusRemoteDiagnostic: fixed(["status", "descriptor", "backend", "available", "auth_status"], {
    status: { type: "string" },
    descriptor: { type: "string" },
    backend: { type: "string" },
    available: bool(),
    auth_status: { type: "string" },
    action: nullableString(),
  }),
  StatusSourceDiagnostic: fixed(["source", "status", "event_count"], {
    source: ref("EventSource"),
    status: { type: "string" },
    event_count: int(),
    source_root: nullableString(),
    action: nullableString(),
  }),
  StatusPrivacyDiagnostic: fixed(["status", "detector", "reason"], {
    status: stringEnum(["available", "degraded"]),
    detector: { type: "string" },
    reason: { type: "string" },
    action: nullableString(),
  }),
  StatusSummaryRunnerDiagnostic: fixed(["backend", "status", "endpoint", "model", "config_file", "api_key_env"], {
    backend: { type: "string" },
    status: { type: "string" },
    endpoint: { type: "string" },
    model: { type: "string" },
    config_file: { type: "string" },
    api_key_env: arrayOf({ type: "string" }),
    action: nullableString(),
  }),
  StatusDreamRunnerDiagnostic: fixed(["runner", "status", "command_configured", "command_env"], {
    runner: { type: "string" },
    status: { type: "string" },
    command_configured: bool(),
    command_env: { type: "string" },
    action: nullableString(),
  }),
  LinkResponse: fixed(
    [
      "command",
      "already_linked",
      "store",
      "project",
      "remote",
      "hydration",
      "imports",
      "rebuilt_search_documents",
      "sync",
      "status",
      "suggested_import_commands",
    ],
    {
    command: { type: "string", const: "init" },
    already_linked: bool(),
    store: ref("StoreStatus"),
    project: ref("ProjectStatus"),
    remote: ref("RemoteSummary"),
    hydration: ref("HydrationReport"),
    imports: arrayOf(ref("SourceImportStatus")),
    rebuilt_search_documents: int(),
    sync: ref("SyncReport"),
    status: ref("StatusProjectSnapshot"),
    suggested_import_commands: arrayOf({ type: "string" }),
  }),
  InitRequest: fixed([], {
    project: nullableString(),
    link_only: bool(),
    source: ref("ProviderSource"),
  }),
  SourceImportStatus: fixed(["source", "status", "discovered_sessions", "imported_events", "warnings"], {
    source: ref("EventSource"),
    status: { type: "string" },
    discovered_sessions: int(),
    imported_events: int(),
    warnings: arrayOf({ type: "string" }),
  }),
  IngestEventsRequest: fixed(["project", "source"], {
    project: { type: "string" },
    source: stringEnum(["codex", "claude", "cursor"]),
    source_root: nullableString(),
  }),
  IngestEventsResponse: fixed(["project_id", "source", "discovered_sessions", "imported_events", "warnings", "event_ids"], {
    project_id: { type: "string" },
    source: ref("EventSource"),
    discovered_sessions: int(),
    imported_events: int(),
    warnings: arrayOf({ type: "string" }),
    event_ids: arrayOf({ type: "string" }),
  }),
  NoteRequest: fixed(["text"], {
    text: { type: "string" },
    project: nullableString(),
  }),
  NoteResponse: fixed(["project_id", "event_id", "source", "citation"], {
    project_id: { type: "string" },
    event_id: { type: "string" },
    source: { type: "string", const: "note" },
    citation: { type: "string", examples: ["mmr://event/event:v1:abc123"] },
  }),
  DreamResponse: fixed(["command", "mode", "scope", "evidence", "system_prompt", "runbook", "output_contract", "guardrails", "suggested_commands"], {
    command: { type: "string", examples: ["assimilate/project"] },
    mode: { type: "string" },
    scope: stringEnum(["project", "source"]),
    project_id: nullableString(),
    source: nullableString(),
    per_project_limit: { type: ["integer", "null"], minimum: 0 },
    since: nullableString({ format: "date-time" }),
    evidence: ref("DreamEvidenceResponse"),
    system_prompt: { type: "string" },
    runbook: arrayOf(ref("DreamRunbookStep")),
    output_contract: ref("DreamOutputContract"),
    guardrails: arrayOf({ type: "string" }),
    suggested_commands: arrayOf({ type: "string" }),
  }),
  DreamEvidenceResponse: fixed(["access", "included_events", "omitted_events", "evidence_hash", "pii_coverage", "events", "omitted"], {
    access: { type: "string" },
    included_events: int(),
    omitted_events: int(),
    evidence_hash: { type: "string" },
    pii_coverage: stringEnum(["available", "degraded"]),
    events: arrayOf({ type: "object", additionalProperties: true }),
    omitted: arrayOf({ type: "object", additionalProperties: true }),
  }),
  DreamRunbookStep: fixed(["step", "objective", "instructions"], {
    step: { type: "string" },
    objective: { type: "string" },
    instructions: arrayOf({ type: "string" }),
  }),
  DreamOutputContract: fixed(["format", "required_sections", "memory_candidate_fields", "refusal_conditions"], {
    format: { type: "string" },
    required_sections: arrayOf({ type: "string" }),
    memory_candidate_fields: arrayOf({ type: "string" }),
    refusal_conditions: arrayOf({ type: "string" }),
  }),
  AssimilationRequest: {
    ...fixed(["scope"], {
      scope: stringEnum(["project", "source"]),
      project: nullableString(),
      source: ref("ProviderSource"),
      evidence_mode: stringEnum(["shared-safe", "local-raw"]),
      allow_raw_evidence: bool(),
      per_project_limit: int(),
      since: nullableString({ format: "date-time" }),
    }),
    allOf: [
      {
        if: { properties: { scope: { const: "source" } }, required: ["scope"] },
        then: { required: ["source"], properties: { source: ref("ProviderSource") } },
      },
    ],
  },
  TeleportStatus: {
    type: "string",
    enum: ["ok", "skipped", "partial", "failed", "unsupported"],
  },
  TeleportFidelity: stringEnum(["native", "shared-safe"]),
  PackArtifactSummary: fixed(["path", "sha256", "required"], {
    path: { type: "string" },
    sha256: { type: "string" },
    required: bool(),
  }),
  TeleportScanSummary: fixed(["blocking_findings", "redacted_findings"], {
    blocking_findings: int(),
    redacted_findings: int(),
    pii_coverage: { type: "string" },
  }),
  PackSessionSummary: fixed(["source", "source_session_id"], {
    source: ref("ProviderSource"),
    project_name: { type: "string" },
    source_session_id: { type: "string" },
  }),
  PackResponse: fixed(["command", "status", "bundle_id", "fidelity", "session", "artifacts", "scan", "dry_run"], {
    command: { type: "string", examples: ["teleport/pack"] },
    status: ref("TeleportStatus"),
    bundle_id: { type: "string" },
    bundle_path: { type: "string" },
    bytes: int({ format: "int64" }),
    sha256: { type: "string" },
    fidelity: ref("TeleportFidelity"),
    session: ref("PackSessionSummary"),
    artifacts: arrayOf(ref("PackArtifactSummary")),
    scan: ref("TeleportScanSummary"),
    dry_run: bool(),
  }),
  SendRemoteApplySummary: fixed(["attempted", "status"], {
    attempted: bool(),
    status: { type: "string" },
    mode: { type: "string" },
  }),
  SendPlannedCommands: fixed(["probe_remote_mmr", "stream_apply", "mkdir_inbox", "scp_bundle"], {
    probe_remote_mmr: arrayOf({ type: "string" }),
    stream_apply: arrayOf({ type: "string" }),
    mkdir_inbox: arrayOf({ type: "string" }),
    scp_bundle: arrayOf({ type: "string" }),
  }),
  FileSendPlan: fixed(["inbox_path", "bundle_path", "sha256_path", "ready_path"], {
    inbox_path: { type: "string" },
    bundle_path: { type: "string" },
    sha256_path: { type: "string" },
    ready_path: { type: "string" },
  }),
  SendResponse: fixed(["command", "status", "bundle_id", "bundle_path", "transport", "to", "session", "dry_run"], {
    command: { type: "string", examples: ["share/session"] },
    status: ref("TeleportStatus"),
    bundle_id: { type: "string" },
    bundle_path: { type: "string" },
    sha256: { type: "string" },
    bytes: int({ format: "int64" }),
    transport: stringEnum(["ssh", "file"]),
    to: { type: "string" },
    inbox_path: { type: "string" },
    ready_path: { type: "string" },
    remote_inbox: { type: "string" },
    session: ref("PackSessionSummary"),
    remote_apply: ref("SendRemoteApplySummary"),
    fidelity: ref("TeleportFidelity"),
    next_command: nullableString(),
    planned_commands: ref("SendPlannedCommands"),
    planned_inbox: ref("FileSendPlan"),
    dry_run: bool(),
  }),
  ImportSessionResponse: fixed(["command", "status", "transport", "from", "bundle_id", "bundle_path", "remote_mmr_version", "read_only"], {
    command: { type: "string", const: "import/session" },
    status: ref("TeleportStatus"),
    transport: { type: "string" },
    from: { type: "string" },
    bundle_id: { type: "string" },
    bundle_path: { type: "string" },
    remote_mmr_version: { type: "string" },
    read_only: bool(),
    read: { type: "object", additionalProperties: true },
    apply: { type: "object", additionalProperties: true },
  }),
  ReadSessionSummary: fixed(["source", "source_session_id"], {
    source: ref("ProviderSource"),
    source_session_id: { type: "string" },
    project_name: { type: "string" },
  }),
  ReceiveStagedSummary: fixed(["bundle_id", "inbox_path", "bundle_path", "sha256"], {
    bundle_id: { type: "string" },
    inbox_path: { type: "string" },
    bundle_path: { type: "string" },
    sha256: { type: "string" },
  }),
  TeleportReadResponse: fixed(["command", "status", "transport", "locator", "cached", "bundle_id", "bundle_path", "session", "message_count", "messages", "dry_run"], {
    command: { type: "string" },
    status: ref("TeleportStatus"),
    transport: stringEnum(["file", "http"]),
    locator: { type: "string" },
    cached: arrayOf(ref("ReceiveStagedSummary")),
    bundle_id: { type: "string" },
    bundle_path: { type: "string" },
    session: ref("ReadSessionSummary"),
    message_count: int({ format: "int64" }),
    messages: arrayOf(ref("ApiMessage")),
    dry_run: bool(),
    text: { type: "string" },
  }),
  ApplyNativeSummary: fixed(["written", "paths"], {
    written: bool(),
    paths: arrayOf({ type: "string" }),
  }),
  ApplyStoreSummary: fixed(["imported_events", "skipped_events"], {
    imported_events: int({ format: "int64" }),
    skipped_events: int({ format: "int64" }),
  }),
  ApplyResumeSummary: fixed(["provider", "documented_command", "agent_resume", "status"], {
    provider: { type: "string" },
    documented_command: { type: "string" },
    agent_resume: { type: "string" },
    status: { type: "string" },
  }),
  TeleportApplyResponse: fixed(["command", "status", "bundle_id", "target_project", "native", "store", "resume", "path_remap_applied", "dry_run"], {
    command: { type: "string" },
    status: ref("TeleportStatus"),
    bundle_id: { type: "string" },
    target_project: { type: "string" },
    native: ref("ApplyNativeSummary"),
    store: ref("ApplyStoreSummary"),
    resume: ref("ApplyResumeSummary"),
    path_remap_applied: bool(),
    dry_run: bool(),
  }),
  ReceiveResponse: fixed(["command", "status", "transport", "locator", "staged", "dry_run"], {
    command: { type: "string", const: "import/bundle" },
    status: ref("TeleportStatus"),
    transport: stringEnum(["file", "http"]),
    locator: { type: "string" },
    staged: arrayOf(ref("ReceiveStagedSummary")),
    apply: { oneOf: [ref("TeleportApplyResponse"), { type: "null" }] },
    dry_run: bool(),
  }),
  BundleImportResponse: {
    oneOf: [ref("TeleportReadResponse"), ref("ReceiveResponse"), ref("TeleportApplyResponse")],
  },
  SessionBundleRequest: fixed([], {
    selector: nullableString(),
    session: nullableString(),
    latest: bool(),
    project: nullableString(),
    source: ref("ProviderSource"),
    output_path: nullableString(),
    fidelity: ref("TeleportFidelity"),
    dry_run: bool(),
  }),
  SessionShareRequest: fixed([], {
    selector: nullableString(),
    session: nullableString(),
    latest: bool(),
    project: nullableString(),
    source: ref("ProviderSource"),
    to: nullableString(),
    via: stringEnum(["auto", "ssh", "http", "file"]),
    bind: nullableString(),
    timeout: int(),
    dry_run: bool(),
  }),
  SessionImportRequest: fixed(["from"], {
    from: { type: "string" },
    session: nullableString(),
    latest: bool(),
    project: nullableString(),
    source: ref("ProviderSource"),
    read_only: bool(),
    apply: bool(),
    force: bool(),
  }),
  BundleImportRequest: fixed([], {
    locator: nullableString(),
    to: nullableString(),
    read_only: bool(),
    apply: bool(),
    project: nullableString(),
    force: bool(),
    output_format: stringEnum(["json", "md"]),
    dry_run: bool(),
  }),
};

const spec = {
  openapi: "3.1.0",
  info: {
    title: "MMR REST API",
    version: "0.2.0",
    summary: "REST contract for the MMR local memory fabric.",
    description:
      "This OpenAPI contract maps MMR's current intent-first CLI and product specs into a REST API surface. Responses preserve the machine-readable JSON shapes used by the CLI where practical. Operations that can expose raw transcripts, contact SSH peers, or write provider files are marked as local-sensitive and require bearer-token authentication in HTTP deployments.",
    license: {
      name: "UNLICENSED",
      identifier: "LicenseRef-UNLICENSED",
    },
  },
  jsonSchemaDialect: "https://json-schema.org/draft/2020-12/schema",
  servers: [
    {
      url: "http://127.0.0.1:8765",
      description: "Local development server",
    },
    {
      url: "http://{tailscale_host}:8765",
      description: "Private Tailscale-accessible host",
      variables: {
        tailscale_host: {
          default: "100.x.y.z",
          description: "Tailscale IP or MagicDNS name for the machine running MMR.",
        },
      },
    },
  ],
  security: [{ bearerAuth: [] }],
  tags: [
    { name: "Status", description: "Setup, store health, and sync readiness." },
    { name: "History", description: "Projects, sessions, raw reads, and previous-session recall." },
    { name: "Search", description: "Exact search and search-to-read retrieval." },
    { name: "Context", description: "Bounded context packets for agents." },
    { name: "Models", description: "Summarization, compacting, and assimilation handoffs." },
    { name: "Memory", description: "Notes, event ingestion, redaction, and sync." },
    { name: "Movement", description: "Trusted peer reads and native session bundle movement." },
  ],
  paths: {
    "/v1/status": {
      get: operation({
        tag: "Status",
        operationId: "getStatus",
        summary: "Inspect local project, store, remote, redaction, and provider readiness",
        description: "REST equivalent of `mmr status`. Without a project parameter, the server uses its current directory or configured project context.",
        parameters: [commonParameters.project, commonParameters.source],
        response: ref("StatusResponse"),
      }),
    },
    "/v1/init": {
      post: operation({
        tag: "Status",
        operationId: "initProject",
        summary: "Set up or repair the local MMR store and project link",
        description: "REST equivalent of `mmr init`. This can import local provider history and may sync redacted state when remote auth is available.",
        request: ref("InitRequest"),
        response: ref("LinkResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/projects": {
      get: operation({
        tag: "History",
        operationId: "listProjects",
        summary: "List known projects with source coverage and recency metadata",
        description: "REST equivalent of `mmr list projects`. Remote targets are explicit trusted SSH peers.",
        parameters: [commonParameters.source, commonParameters.limit, commonParameters.offset, commonParameters.sortBy, commonParameters.order, commonParameters.remote],
        response: ref("ApiProjectsResponse"),
      }),
    },
    "/v1/sessions": {
      get: operation({
        tag: "History",
        operationId: "listSessions",
        summary: "List sessions in a project, source, or all-project scope",
        description: "REST equivalent of `mmr list sessions`. When project and all are omitted, cwd project auto-discovery semantics apply.",
        parameters: [commonParameters.project, commonParameters.all, commonParameters.source, commonParameters.limit, commonParameters.offset, commonParameters.sortBy, commonParameters.order, commonParameters.remote],
        response: ref("ApiSessionsResponse"),
      }),
    },
    "/v1/sessions/{session_id}/messages": {
      get: operation({
        tag: "History",
        operationId: "readSessionMessages",
        summary: "Read one explicit session by source session ID",
        description: "REST equivalent of `mmr read session <session-id>`. Without project or source, the session is searched globally.",
        parameters: [
          pathParam("session_id", { type: "string" }, "Public source session identifier."),
          commonParameters.project,
          commonParameters.source,
          commonParameters.limit,
          commonParameters.offset,
          commonParameters.remote,
        ],
        response: ref("ApiMessagesResponse"),
      }),
    },
    "/v1/messages": {
      get: operation({
        tag: "History",
        operationId: "readMessages",
        summary: "Read project-scoped or source-scoped history",
        description: "REST equivalent of `mmr read project` or `mmr read source`. Use scope=project for cwd/explicit project history and scope=source with an explicit source for harness-wide history.",
        parameters: [
          param("scope", stringEnum(["project", "source"]), "History scope to read.", true),
          commonParameters.project,
          commonParameters.source,
          commonParameters.limit,
          commonParameters.offset,
          commonParameters.remote,
        ],
        response: ref("ApiMessagesResponse"),
      }),
    },
    "/v1/recall": {
      get: operation({
        tag: "History",
        operationId: "recallPreviousSession",
        summary: "Retrieve a previous stable session in scope",
        description: "REST equivalent of `mmr recall`. Age 0 is held back unless include_newest is true; paged responses pin the resolved concrete session in the continuation.",
        parameters: [
          param("n", int({ minimum: 1 }), "How many sessions back to read. 1 means previous stable session."),
          commonParameters.project,
          commonParameters.all,
          commonParameters.source,
          param("include_newest", bool(), "Make newest visible session, age 0, addressable."),
          commonParameters.limit,
          commonParameters.offset,
          commonParameters.remote,
        ],
        response: ref("ApiMessagesResponse"),
      }),
    },
    "/v1/find": {
      get: operation({
        tag: "Search",
        operationId: "findEvents",
        summary: "Search normalized events and learned memory with literal text",
        description: "REST equivalent of `mmr find` JSON output. The CLI-only tab-delimited line format is intentionally excluded.",
        parameters: [
          param("query", { type: "string" }, "Literal query or pattern to find.", true),
          commonParameters.project,
          commonParameters.source,
          param("session", { type: "string" }, "Public source session id to search."),
          param("role", { type: "string" }, "Event role filter."),
          param("event_type", { type: "string" }, "Event type filter."),
          param("ignore_case", bool(), "Case-insensitive literal matching."),
          param("context", int(), "Context lines before and after each match."),
        ],
        response: ref("SearchResponse"),
      }),
    },
    "/v1/retrievals": {
      post: operation({
        tag: "Search",
        operationId: "retrieveContext",
        summary: "Search matching events, rank sessions, and optionally return provider message windows",
        description: "REST equivalent of `mmr retrieve`. Broad local scopes are explicit; remote retrieval is intentionally out of v1.",
        request: ref("RetrieveRequest"),
        response: ref("RetrieveResponse"),
      }),
    },
    "/v1/context/project": {
      get: operation({
        tag: "Context",
        operationId: "getProjectContext",
        summary: "Produce project-specific context across sources",
        description: "REST equivalent of `mmr context project`.",
        parameters: [commonParameters.project, commonParameters.source, commonParameters.limit, commonParameters.remote],
        response: ref("ContextResponse"),
      }),
    },
    "/v1/context/source": {
      get: operation({
        tag: "Context",
        operationId: "getSourceContext",
        summary: "Produce source-wide context for one explicit provider source",
        description: "REST equivalent of `mmr --source <source> context source`.",
        parameters: [commonParameters.sourceRequired, commonParameters.limit, commonParameters.remote],
        response: ref("ContextResponse"),
      }),
    },
    "/v1/summaries": {
      post: operation({
        tag: "Models",
        operationId: "createSummary",
        summary: "Run a stateless summary over project, source, or session history",
        description: "REST equivalent of `mmr summarize project|source|session`. The server calls the configured OpenAI-compatible summarizer.",
        request: ref("SummaryRequest"),
        response: ref("RememberResponse"),
        extensions: {
          "x-local-sensitive": true,
        },
      }),
    },
    "/v1/compactions": {
      post: operation({
        tag: "Models",
        operationId: "createCompaction",
        summary: "Compact scoped history with Morph Compact without rewriting surviving lines",
        description: "REST equivalent of `mmr compact project|source|session`.",
        request: ref("CompactRequest"),
        response: ref("CompactResponse"),
        extensions: {
          "x-local-sensitive": true,
        },
      }),
    },
    "/v1/assimilation-handoffs": {
      post: operation({
        tag: "Models",
        operationId: "createAssimilationHandoff",
        summary: "Return prompt, runbook, output contract, and evidence for memory assimilation",
        description: "REST equivalent of `mmr assimilate project|source`. It returns an agent handoff and does not run a provider as a side effect.",
        request: ref("AssimilationRequest"),
        response: ref("DreamResponse"),
        extensions: {
          "x-local-sensitive": true,
        },
      }),
    },
    "/v1/notes": {
      post: operation({
        tag: "Memory",
        operationId: "createNote",
        summary: "Add a human-authored project event",
        description: "REST equivalent of `mmr note`.",
        request: ref("NoteRequest"),
        response: ref("NoteResponse"),
        extensions: {
          "x-writes-files": true,
        },
      }),
    },
    "/v1/ingest/events": {
      post: operation({
        tag: "Memory",
        operationId: "ingestEvents",
        summary: "Ingest provider history into normalized local events",
        description: "REST equivalent of `mmr --source codex|claude|cursor ingest events`. Provider source is required.",
        request: ref("IngestEventsRequest"),
        response: ref("IngestEventsResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/redactions/scan": {
      post: operation({
        tag: "Memory",
        operationId: "scanRedactions",
        summary: "Scan linked project events and persist redaction runs",
        description: "REST equivalent of `mmr redact scan`.",
        request: ref("RedactScanRequest"),
        response: ref("RedactScanResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/redactions/events/{event_id}": {
      get: operation({
        tag: "Memory",
        operationId: "explainRedaction",
        summary: "Explain the latest redaction result for one event",
        description: "REST equivalent of `mmr redact explain <event-id>`.",
        parameters: [pathParam("event_id", { type: "string" }, "Normalized event id.")],
        response: ref("RedactExplainResponse"),
      }),
    },
    "/v1/sync-dry-runs": {
      post: operation({
        tag: "Memory",
        operationId: "dryRunSync",
        summary: "Preview sync safety without contacting a remote",
        description: "REST equivalent of `mmr sync --dry-run`. It must not mutate redaction or sync state.",
        request: ref("SyncRequest"),
        response: ref("SyncDryRunResponse"),
      }),
    },
    "/v1/sync-runs": {
      post: operation({
        tag: "Memory",
        operationId: "runSync",
        summary: "Safely reconcile redacted project memory with the configured remote",
        description: "REST equivalent of `mmr sync`. Unsafe events and degraded PII coverage block upload.",
        request: ref("SyncRequest"),
        response: ref("SyncReport"),
        extensions: {
          "x-local-sensitive": true,
          "x-contacts-peer": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/session-shares": {
      post: operation({
        tag: "Movement",
        operationId: "shareSession",
        summary: "Share one native provider session over SSH, file inbox, or one-shot HTTP",
        description: "REST equivalent of `mmr share session`. This is local-sensitive and may contact a peer or serve a one-shot locator.",
        request: ref("SessionShareRequest"),
        response: ref("SendResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-contacts-peer": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/session-bundles": {
      post: operation({
        tag: "Movement",
        operationId: "packSessionBundle",
        summary: "Create or preview a local native provider session bundle",
        description: "REST adapter for the native bundle packing step used by `mmr share session`; mirrors the internal `teleport/pack` JSON response.",
        request: ref("SessionBundleRequest"),
        response: ref("PackResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/session-imports": {
      post: operation({
        tag: "Movement",
        operationId: "importSession",
        summary: "Pull a selected native session bundle from an SSH peer",
        description: "REST equivalent of `mmr import session`. Apply mode writes native provider files.",
        request: ref("SessionImportRequest"),
        response: ref("ImportSessionResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-contacts-peer": true,
          "x-writes-files": true,
        },
      }),
    },
    "/v1/bundle-imports": {
      post: operation({
        tag: "Movement",
        operationId: "importBundle",
        summary: "Read or apply an existing bundle locator",
        description: "REST equivalent of `mmr import bundle`. Locators may be local paths, inbox entries, stdin streams in CLI form, or one-shot HTTP locators.",
        request: ref("BundleImportRequest"),
        response: ref("BundleImportResponse"),
        extensions: {
          "x-local-sensitive": true,
          "x-writes-files": true,
        },
      }),
    },
  },
  components: {
    securitySchemes: {
      bearerAuth: {
        type: "http",
        scheme: "bearer",
        bearerFormat: "opaque local token",
        description: "Required for HTTP deployments because MMR exposes local transcript and project memory.",
      },
    },
    schemas,
  },
};

fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(openapiPath, `${JSON.stringify(spec, null, 2)}\n`);

const generatedSpec = JSON.parse(fs.readFileSync(openapiPath, "utf8"));

const escapeHtml = (value) =>
  String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");

const methods = Object.entries(generatedSpec.paths).flatMap(([route, verbs]) =>
  Object.entries(verbs).map(([method, op]) => ({ route, method: method.toUpperCase(), op }))
);

const byTag = new Map();
for (const item of methods) {
  const tag = item.op.tags?.[0] ?? "Other";
  if (!byTag.has(tag)) byTag.set(tag, []);
  byTag.get(tag).push(item);
}

const tagNav = generatedSpec.tags
  .map((tag) => `<a href="#tag-${escapeHtml(tag.name)}">${escapeHtml(tag.name)}</a>`)
  .join("");

const endpointSections = generatedSpec.tags
  .filter((tag) => byTag.has(tag.name))
  .map((tag) => {
    const rows = byTag
      .get(tag.name)
      .map(
        ({ route, method, op }) => `
          <article class="endpoint">
            <div class="endpoint-head">
              <span class="method ${escapeHtml(method.toLowerCase())}">${escapeHtml(method)}</span>
              <code>${escapeHtml(route)}</code>
            </div>
            <h3>${escapeHtml(op.summary)}</h3>
            <p>${escapeHtml(op.description ?? "")}</p>
            <details>
              <summary>Operation details</summary>
              <dl>
                <dt>operationId</dt><dd><code>${escapeHtml(op.operationId)}</code></dd>
                <dt>response</dt><dd><code>${escapeHtml(
                  op.responses?.["200"]?.content?.["application/json"]?.schema?.$ref?.replace("#/components/schemas/", "") ?? "inline"
                )}</code></dd>
              </dl>
            </details>
          </article>`
      )
      .join("");
    return `
      <section id="tag-${escapeHtml(tag.name)}">
        <h2>${escapeHtml(tag.name)}</h2>
        <p>${escapeHtml(tag.description ?? "")}</p>
        <div class="grid">${rows}</div>
      </section>`;
  })
  .join("");

const schemaList = Object.keys(generatedSpec.components.schemas)
  .sort()
  .map((name) => `<li><code>${escapeHtml(name)}</code></li>`)
  .join("");

const html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escapeHtml(generatedSpec.info.title)} Docs</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f7f8f6;
      --ink: #17211b;
      --muted: #5a665f;
      --line: #d9ded8;
      --panel: #ffffff;
      --accent: #176f5d;
      --post: #235d9f;
      --get: #1f7a4d;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font: 15px/1.45 ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      color: var(--ink);
      background: var(--bg);
    }
    header {
      padding: 28px min(5vw, 56px) 20px;
      border-bottom: 1px solid var(--line);
      background: var(--panel);
    }
    h1 { margin: 0 0 8px; font-size: 28px; letter-spacing: 0; }
    h2 { margin: 34px 0 6px; font-size: 22px; letter-spacing: 0; }
    h3 { margin: 12px 0 6px; font-size: 16px; letter-spacing: 0; }
    p { margin: 0; color: var(--muted); max-width: 920px; }
    nav {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      margin-top: 18px;
    }
    nav a, .spec-link {
      color: var(--accent);
      border: 1px solid var(--line);
      background: #fff;
      padding: 7px 10px;
      border-radius: 6px;
      text-decoration: none;
      font-weight: 600;
    }
    main { padding: 0 min(5vw, 56px) 56px; }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      margin-top: 18px;
      color: var(--muted);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
      gap: 12px;
      margin-top: 14px;
    }
    .endpoint {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 14px;
    }
    .endpoint-head {
      display: flex;
      align-items: center;
      gap: 9px;
      min-width: 0;
    }
    code {
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
      overflow-wrap: anywhere;
    }
    .method {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 48px;
      padding: 4px 6px;
      border-radius: 5px;
      color: #fff;
      font-size: 12px;
      font-weight: 800;
    }
    .method.get { background: var(--get); }
    .method.post { background: var(--post); }
    details { margin-top: 10px; color: var(--muted); }
    summary { cursor: pointer; font-weight: 700; color: var(--ink); }
    dl { display: grid; grid-template-columns: max-content 1fr; gap: 6px 12px; }
    dt { font-weight: 700; color: var(--ink); }
    ul.schema-list {
      columns: 3 220px;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 16px 16px 16px 34px;
    }
  </style>
</head>
<body>
  <header>
    <h1>${escapeHtml(generatedSpec.info.title)}</h1>
    <p>${escapeHtml(generatedSpec.info.description)}</p>
    <div class="meta">
      <span>OpenAPI ${escapeHtml(generatedSpec.openapi)}</span>
      <span>${methods.length} operations</span>
      <span>${Object.keys(generatedSpec.components.schemas).length} schemas</span>
      <a class="spec-link" href="./openapi.json">openapi.json</a>
    </div>
    <nav>${tagNav}</nav>
  </header>
  <main>
    ${endpointSections}
    <section>
      <h2>Schemas</h2>
      <p>Component schemas are emitted from the same OpenAPI JSON used to render these docs.</p>
      <ul class="schema-list">${schemaList}</ul>
    </section>
  </main>
</body>
</html>
`;

fs.writeFileSync(htmlPath, html);

const readme = `# MMR REST API

Generated API documentation for MMR's REST contract.

- OpenAPI source: [openapi.json](./openapi.json)
- Static docs: [index.html](./index.html)
- OpenAPI version: ${generatedSpec.openapi}
- Operations: ${methods.length}
- Schemas: ${Object.keys(generatedSpec.components.schemas).length}

Regenerate after contract edits:

\`\`\`sh
node scripts/generate-api-docs.mjs
\`\`\`
`;

fs.writeFileSync(readmePath, readme);
console.log(`wrote ${path.relative(process.cwd(), openapiPath)}`);
console.log(`wrote ${path.relative(process.cwd(), htmlPath)}`);
console.log(`wrote ${path.relative(process.cwd(), readmePath)}`);
