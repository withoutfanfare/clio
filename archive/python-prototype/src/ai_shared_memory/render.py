"""Formatting helpers for CLI and MCP responses."""

from __future__ import annotations

import json

from .models import MemoryRecord, RecallResult


def render_memory_markdown(record: MemoryRecord) -> str:
    """Render a single memory as Markdown."""
    parts = [
        f"# {record.title or record.id}",
        "",
        f"- ID: `{record.id}`",
        f"- Namespace: `{record.namespace}`",
        f"- Kind: `{record.kind}`",
        f"- Importance: `{record.importance}`",
        f"- Tags: {', '.join(f'`{tag}`' for tag in record.tags) if record.tags else 'none'}",
        f"- Source: `{record.source}`" if record.source else "- Source: none",
        f"- Source Ref: `{record.source_ref}`" if record.source_ref else "- Source Ref: none",
        f"- Confidence: `{record.confidence}`" if record.confidence is not None else "- Confidence: none",
        f"- Created: `{record.created_at}`",
        f"- Updated: `{record.updated_at}`",
        f"- Archived: `{record.archived_at}`" if record.archived_at else "- Archived: active",
        "",
    ]
    if record.summary:
        parts.extend(["## Summary", "", record.summary, ""])
    parts.extend(["## Content", "", record.content, ""])
    if record.metadata:
        parts.extend(["## Metadata", "", "```json", json.dumps(record.metadata, indent=2, sort_keys=True), "```"])
    return "\n".join(parts)


def render_recall_markdown(result: RecallResult) -> str:
    """Render a recall result as Markdown."""
    parts = [
        f"# Recall Results",
        "",
        f"- Total: `{result.total}`",
        f"- Returned: `{result.count}`",
        f"- Offset: `{result.offset}`",
        f"- Limit: `{result.limit}`",
        "",
    ]
    if not result.items:
        parts.append("No memories matched.")
        return "\n".join(parts)

    for item in result.items:
        title = item.title or item.id
        parts.extend(
            [
                f"## {title}",
                "",
                f"- ID: `{item.id}`",
                f"- Namespace: `{item.namespace}`",
                f"- Kind: `{item.kind}`",
                f"- Tags: {', '.join(f'`{tag}`' for tag in item.tags) if item.tags else 'none'}",
                f"- Updated: `{item.updated_at}`",
                f"- Rank: `{item.rank:.4f}`" if item.rank is not None else "- Rank: n/a",
                "",
                item.summary or item.content[:280],
                "",
            ]
        )
    return "\n".join(parts)
