#!/usr/bin/env python3
"""FastMCP OpenAPI bootstrap for exposing the mmr REST API as MCP."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

try:
    import httpx
    from fastmcp import FastMCP
    from fastmcp.server.providers.openapi import (
        MCPType,
        OpenAPIResource,
        OpenAPIResourceTemplate,
        OpenAPITool,
        RouteMap,
    )
    from fastmcp.server.transforms import ToolTransform
    from fastmcp.tools.tool_transform import ArgTransformConfig, ToolTransformConfig
    from fastmcp.utilities.openapi import HTTPRoute
except ModuleNotFoundError as exc:
    missing = exc.name or "fastmcp/httpx"
    sys.stderr.write(
        "Missing Python dependency for the mmr REST MCP bootstrap: "
        f"{missing}.\nInstall dependencies with: python3 -m pip install fastmcp httpx\n"
    )
    raise SystemExit(2) from exc


API_BASE_URL = os.getenv("API_BASE_URL", "http://127.0.0.1:8765").rstrip("/")
OPENAPI_URL = os.getenv("OPENAPI_URL", f"{API_BASE_URL}/openapi.json")
API_TOKEN_ENV = os.getenv("API_TOKEN_ENV", "API_TOKEN")
API_TOKEN = os.getenv(API_TOKEN_ENV)
MCP_TRANSPORT = os.getenv("MCP_TRANSPORT", "stdio").lower()
MCP_HOST = os.getenv("MCP_HOST", "127.0.0.1")
MCP_PORT = int(os.getenv("MCP_PORT", "8766"))

HEADERS = {"Authorization": f"Bearer {API_TOKEN}"} if API_TOKEN else {}


TOOL_REMAPS: dict[str, dict[str, Any]] = {
    "getStatus": {
        "name": "mmr_status",
        "description": "Inspect local mmr REST status, linked project state, and sync/redaction blockers.",
        "tags": {"mmr", "status", "read"},
    },
    "listProjects": {
        "name": "mmr_list_projects",
        "description": "List known mmr projects with source coverage and recency metadata.",
        "tags": {"mmr", "history", "read"},
    },
    "listSessions": {
        "name": "mmr_list_sessions",
        "description": "List mmr sessions in a project, source, or all-project scope.",
        "tags": {"mmr", "history", "read"},
    },
    "readSessionMessages": {
        "name": "mmr_read_session",
        "description": "Read messages for one explicit mmr session id.",
        "tags": {"mmr", "history", "read"},
    },
    "readMessages": {
        "name": "mmr_read_messages",
        "description": "Read chronological mmr messages for a project, source, or session query.",
        "tags": {"mmr", "history", "read"},
    },
    "recallPreviousSession": {
        "name": "mmr_recall",
        "description": "Retrieve a previous stable session for immediate coding continuity.",
        "tags": {"mmr", "continuity", "read"},
    },
    "findEvents": {
        "name": "mmr_find",
        "description": "Search normalized local mmr events and learned memory.",
        "tags": {"mmr", "search", "read"},
    },
    "getProjectContext": {
        "name": "mmr_context_project",
        "description": "Produce a project-specific context brief across configured sources.",
        "tags": {"mmr", "context", "read"},
    },
    "getSourceContext": {
        "name": "mmr_context_source",
        "description": "Produce source-wide context for one explicit source.",
        "tags": {"mmr", "context", "read"},
    },
    "explainRedaction": {
        "name": "mmr_redaction_explain",
        "description": "Explain why one event is blocked by local redaction policy.",
        "tags": {"mmr", "redaction", "read"},
    },
    "dryRunSync": {
        "name": "mmr_sync_dry_run",
        "description": "Preview sync behavior without uploading or mutating remote state.",
        "tags": {"mmr", "sync", "read"},
    },
}

mcp_names = {operation_id: cfg["name"] for operation_id, cfg in TOOL_REMAPS.items()}
tool_descriptions = {cfg["name"]: cfg for cfg in TOOL_REMAPS.values()}


def load_openapi_spec(location: str) -> dict[str, Any]:
    if location.startswith(("http://", "https://")):
        response = httpx.get(location, headers=HEADERS, timeout=30.0)
        response.raise_for_status()
        return response.json()

    with Path(location).expanduser().open("r", encoding="utf-8") as handle:
        return json.load(handle)


def customize_components(
    route: HTTPRoute,
    component: OpenAPITool | OpenAPIResource | OpenAPIResourceTemplate,
) -> None:
    component.tags.add("mmr-rest")

    if isinstance(component, OpenAPITool):
        cfg = tool_descriptions.get(component.name)
        if cfg:
            component.description = cfg["description"]
            component.tags.update(cfg.get("tags", set()))
        else:
            component.description = (
                f"{component.description}\n\n"
                f"HTTP backing route: {route.method} {route.path}"
            )


openapi_spec = load_openapi_spec(OPENAPI_URL)
api_client = httpx.AsyncClient(
    base_url=API_BASE_URL,
    headers=HEADERS,
    timeout=30.0,
)

mcp = FastMCP.from_openapi(
    openapi_spec=openapi_spec,
    client=api_client,
    name="mmr REST API MCP Bootstrap",
    mcp_names=mcp_names,
    mcp_component_fn=customize_components,
    route_maps=[
        RouteMap(
            pattern=(
                r"^/v1/(init|notes|ingest/events|redactions/scan|sync-runs|"
                r"session-shares|session-bundles|session-imports|bundle-imports)$"
            ),
            mcp_type=MCPType.EXCLUDE,
        ),
        RouteMap(
            pattern=r"^/v1/(summaries|compactions|assimilation-handoffs|retrievals)$",
            mcp_type=MCPType.EXCLUDE,
        ),
        RouteMap(
            pattern=(
                r"^/v1/(status|projects|sessions|sessions/(?:[^/]+|\{session_id\})/messages|messages|"
                r"recall|find|context/project|context/source|redactions/events/[^/]+|"
                r"redactions/events/\{event_id\}|sync-dry-runs)$"
            ),
            mcp_type=MCPType.TOOL,
        ),
        RouteMap(pattern=r".*", mcp_type=MCPType.EXCLUDE),
    ],
    tags={"mmr", "rest-api"},
)

mcp.add_transform(
    ToolTransform(
        {
            "mmr_find": ToolTransformConfig(
                description="Search local mmr history. Prefer this before reading broad project history.",
                arguments={
                    "q": ArgTransformConfig(
                        name="query",
                        description="Search text for normalized events or learned memory.",
                    ),
                    "limit": ArgTransformConfig(
                        description="Maximum matches to return.",
                        default=10,
                    ),
                },
            ),
            "mmr_list_sessions": ToolTransformConfig(
                description="List sessions before selecting one for mmr_read_session.",
                arguments={
                    "limit": ArgTransformConfig(default=20),
                },
            ),
        }
    )
)


def main() -> None:
    if MCP_TRANSPORT == "stdio":
        mcp.run()
        return
    if MCP_TRANSPORT == "http":
        mcp.run(transport="http", host=MCP_HOST, port=MCP_PORT)
        return
    sys.stderr.write("MCP_TRANSPORT must be 'stdio' or 'http'.\n")
    raise SystemExit(2)


if __name__ == "__main__":
    main()
