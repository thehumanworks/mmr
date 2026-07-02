# MCP Server from OpenAPI json

**framework:** `FastMCP`

## Relevant FastMCP docs

- **OpenAPI integration**: `FastMCP.from_openapi(openapi_spec, client, ...)`, auth via `httpx.AsyncClient`, route maps, `mcp_names`, `mcp_component_fn`, tags, and exclusions. <citation refs="KcHdsVe3ql-lzKvaXewXg">FastMCP converts an OpenAPI spec into MCP components and supports `mcp_names`, route maps, and `mcp_component_fn` customization.</citation>
- **Tool transformation**: rename tools, rewrite descriptions, rename/hide/default arguments using `ToolTransform` or `Tool.from_tool`. <citation refs="NWAhtnl-NEC9g5P5Xc39f">FastMCP supports deferred `ToolTransform` for provider/generated tools and immediate `Tool.from_tool()` for direct tools.</citation>
- **Transform pipeline**: useful because OpenAPI-generated tools come from a provider, and server/provider transforms can reshape what clients see. <citation refs="yJMMRpO-cobWaXPTN2vAf">Transforms modify components as they flow from providers to clients, and transform order affects names.</citation>
- **Running servers**: default is stdio, HTTP is available with `mcp.run(transport="http", host=..., port=...)`. <citation refs="iFUCaKutQSnBUfkprHCTk">FastMCP supports stdio by default and HTTP for network services.</citation>
- **CLI/dev inspector**: `fastmcp run server.py`, `fastmcp run server.py --transport http`, and `fastmcp dev inspector server.py`. <citation refs="eqfHqjuQ2Fb80gaoC5evz">The CLI starts servers over stdio by default and can serve HTTP with flags.</citation>
- **Client testing**: `Client(server)` or `Client("server.py")`, then `list_tools()` and `call_tool()`. <citation refs="S1kNlVqgxoqc-XSHlY3dA">The FastMCP client can list tools and call them for deterministic testing.</citation>
- **Large APIs**: use search transforms so the model sees `search_tools`/`call_tool` instead of hundreds of tools. <citation refs="G3QVLDOnE5f3Fx7-vTbKU">Search transforms hide huge tool catalogs behind searchable discovery tools.</citation>

## Practical starter `server.py`

```python
import os
import httpx

from fastmcp import FastMCP
from fastmcp.server.providers.openapi import (
    RouteMap,
    MCPType,
    OpenAPITool,
    OpenAPIResource,
    OpenAPIResourceTemplate,
)
from fastmcp.utilities.openapi import HTTPRoute
from fastmcp.server.transforms import ToolTransform
from fastmcp.tools.tool_transform import ToolTransformConfig, ArgTransformConfig

API_BASE_URL = os.environ["API_BASE_URL"].rstrip("/")
OPENAPI_URL = os.getenv("OPENAPI_URL", f"{API_BASE_URL}/openapi.json")
API_TOKEN = os.getenv("API_TOKEN")

headers = {"Authorization": f"Bearer {API_TOKEN}"} if API_TOKEN else {}

# Load the OpenAPI spec from the REST API.
openapi_spec = httpx.get(OPENAPI_URL, timeout=30).json()

# operationId -> curated MCP-facing metadata.
TOOL_REMAPS = {
    "list_users__with_pagination": {
        "name": "list_users",
        "description": "List users in the account. Use this to browse or search users before fetching a specific user.",
        "tags": {"users", "read"},
    },
    "get_user_details__admin_required": {
        "name": "get_user_profile",
        "description": "Get profile details for one user by user_id. Use after list_users returns the target user.",
        "tags": {"users", "read"},
    },
    "create_user__admin_required": {
        "name": "create_user",
        "description": "Create a new user account. Requires name, email, and role.",
        "tags": {"users", "write"},
    },
}

mcp_names = {operation_id: cfg["name"] for operation_id, cfg in TOOL_REMAPS.items()}
tool_descriptions = {cfg["name"]: cfg for cfg in TOOL_REMAPS.values()}


def customize_components(
    route: HTTPRoute,
    component: OpenAPITool | OpenAPIResource | OpenAPIResourceTemplate,
) -> None:
    component.tags.add("rest-api")

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


api_client = httpx.AsyncClient(
    base_url=API_BASE_URL,
    headers=headers,
    timeout=30.0,
)

mcp = FastMCP.from_openapi(
    openapi_spec=openapi_spec,
    client=api_client,
    name="My REST API MCP Server",
    mcp_names=mcp_names,
    mcp_component_fn=customize_components,
    route_maps=[
        RouteMap(pattern=r"^/admin/.*", mcp_type=MCPType.EXCLUDE),
        RouteMap(tags={"internal"}, mcp_type=MCPType.EXCLUDE),
        RouteMap(mcp_type=MCPType.TOOL),
    ],
    tags={"my-api"},
)

# Optional post-generation cleanup by generated/remapped tool name.
mcp.add_transform(ToolTransform({
    "list_users": ToolTransformConfig(
        description="Search or list users. Prefer this before get_user_profile.",
        arguments={
            "q": ArgTransformConfig(
                name="query",
                description="Name, email, or other user search text.",
            ),
            "limit": ArgTransformConfig(
                description="Maximum users to return.",
                default=20,
            ),
        },
    )
}))

if __name__ == "__main__":
    mcp.run()  # stdio
    # or: mcp.run(transport="http", host="127.0.0.1", port=8000)
```

## Helper to discover operationIds

```python
import os
import httpx

spec = httpx.get(os.environ["OPENAPI_URL"], timeout=30).json()

for path, methods in spec.get("paths", {}).items():
    for method, operation in methods.items():
        if method.lower() not in {"get", "post", "put", "patch", "delete"}:
            continue
        print(
            repr(operation.get("operationId")),
            method.upper(),
            path,
            "-",
            operation.get("summary", ""),
        )
```

## Test the generated MCP tools

```python
import asyncio
from fastmcp import Client
from server import mcp

async def main():
    async with Client(mcp) as client:
        tools = await client.list_tools()
        for tool in tools:
            print(tool.name, "-", tool.description)

asyncio.run(main())
```

Key advice: use OpenAPI conversion to bootstrap, but curate aggressively. The FastMCP author explicitly warns that raw REST APIs often create too many atomic, confusing tools for agents, and recommends transforming/curating them. <citation refs="RIFkXiUO3829rgECOZ-jc">The recommendation is “Bootstrap, Don’t Deploy” and “Curate Aggressively” rather than blindly exposing large REST APIs.</citation>
