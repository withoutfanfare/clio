"""FastMCP server exposing the shared memory store."""

from __future__ import annotations

from typing import Any

from mcp.server.fastmcp import FastMCP

from .config import get_database_path
from .models import ArchiveInput, LinkInput, MemoryInput, SearchInput
from .render import render_memory_markdown, render_recall_markdown
from .store import MemoryStore


mcp = FastMCP("ai_shared_memory_mcp")
store = MemoryStore(get_database_path())


def format_result(payload: Any, response_format: str) -> Any:
    """Return JSON-serializable data or Markdown based on caller preference."""
    if response_format == "json":
        if hasattr(payload, "model_dump"):
            return payload.model_dump()
        return payload
    if hasattr(payload, "items"):
        return render_recall_markdown(payload)
    return render_memory_markdown(payload)


@mcp.tool(
    name="memory_remember",
    annotations={
        "title": "Remember Memory",
        "readOnlyHint": False,
        "destructiveHint": False,
        "idempotentHint": False,
        "openWorldHint": False,
    },
)
async def memory_remember(params: MemoryInput) -> dict[str, Any]:
    """Store a memory in the shared local database."""
    record = store.remember(params)
    return record.model_dump()


@mcp.tool(
    name="memory_recall",
    annotations={
        "title": "Recall Memory",
        "readOnlyHint": True,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def memory_recall(params: SearchInput) -> Any:
    """Search memories by full-text query, namespace, kind, and tags."""
    result = store.search(params)
    return format_result(result, params.response_format.value)


@mcp.tool(
    name="memory_get",
    annotations={
        "title": "Get Memory",
        "readOnlyHint": True,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def memory_get(memory_id: str, response_format: str = "markdown") -> Any:
    """Fetch one memory by id."""
    record = store.get(memory_id)
    return format_result(record, response_format)


@mcp.tool(
    name="memory_recent",
    annotations={
        "title": "Recent Memory",
        "readOnlyHint": True,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def memory_recent(
    namespace: str | None = None,
    limit: int = 10,
    response_format: str = "markdown",
) -> Any:
    """List the most recently updated memories."""
    result = store.search(
        SearchInput(
            namespace=namespace,
            limit=limit,
            response_format=response_format,
        )
    )
    return format_result(result, response_format)


@mcp.tool(
    name="memory_link",
    annotations={
        "title": "Link Memories",
        "readOnlyHint": False,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def memory_link(params: LinkInput) -> dict[str, Any]:
    """Create a typed link between two memories."""
    return store.link(params)


@mcp.tool(
    name="memory_archive",
    annotations={
        "title": "Archive Memory",
        "readOnlyHint": False,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def memory_archive(params: ArchiveInput) -> dict[str, Any]:
    """Soft-archive a memory."""
    record = store.archive(params.memory_id)
    return record.model_dump()


@mcp.resource("memory://schema")
def memory_schema() -> str:
    """Expose schema metadata for clients that want to inspect the store."""
    return store.schema_snapshot().__repr__()


@mcp.resource("memory://item/{memory_id}")
def memory_item(memory_id: str) -> str:
    """Expose a single memory as a resource."""
    record = store.get(memory_id)
    return render_memory_markdown(record)


@mcp.resource("memory://recent/{namespace}")
def memory_recent_resource(namespace: str) -> str:
    """Expose recent memories from a namespace as a resource."""
    result = store.search(SearchInput(namespace=namespace, limit=10))
    return render_recall_markdown(result)


def main() -> None:
    """Run the MCP server over stdio."""
    store.initialize()
    mcp.run()


if __name__ == "__main__":
    main()
