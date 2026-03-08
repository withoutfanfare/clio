"""Shared models for the memory store and MCP surface."""

from __future__ import annotations

from datetime import datetime
from enum import Enum
from typing import Any

from pydantic import BaseModel, ConfigDict, Field, field_validator


class ResponseFormat(str, Enum):
    """Supported output formats."""

    JSON = "json"
    MARKDOWN = "markdown"


class MemoryInput(BaseModel):
    """Input model for storing a memory."""

    model_config = ConfigDict(
        extra="forbid",
        str_strip_whitespace=True,
        validate_assignment=True,
    )

    namespace: str = Field(
        default="global",
        min_length=1,
        max_length=120,
        description="Memory namespace such as global, project:ai, or person:danny.",
    )
    kind: str = Field(
        default="note",
        min_length=1,
        max_length=50,
        description="Memory type such as note, fact, decision, summary, or task.",
    )
    title: str | None = Field(
        default=None,
        max_length=240,
        description="Optional short title for the memory.",
    )
    summary: str | None = Field(
        default=None,
        max_length=1000,
        description="Optional summary to aid search and quick recall.",
    )
    content: str = Field(
        ...,
        min_length=1,
        description="Main memory body in plain text or Markdown.",
    )
    tags: list[str] = Field(
        default_factory=list,
        description="Zero or more tags for filtering and recall.",
        max_length=20,
    )
    source: str | None = Field(
        default=None,
        max_length=120,
        description="Tool or system that created the memory, for example codex or claude-code.",
    )
    source_ref: str | None = Field(
        default=None,
        max_length=240,
        description="External stable key such as a conversation, issue, or document id.",
    )
    confidence: float | None = Field(
        default=None,
        ge=0.0,
        le=1.0,
        description="Optional confidence score between 0 and 1.",
    )
    importance: int = Field(
        default=3,
        ge=1,
        le=5,
        description="Relative importance from 1 (low) to 5 (high).",
    )
    metadata: dict[str, Any] = Field(
        default_factory=dict,
        description="Client-specific JSON metadata for future extensions.",
    )
    valid_from: datetime | None = Field(
        default=None,
        description="Optional timestamp when this memory becomes valid.",
    )
    valid_until: datetime | None = Field(
        default=None,
        description="Optional timestamp when this memory should no longer be treated as current.",
    )
    upsert: bool = Field(
        default=False,
        description="If true and source plus source_ref matches an existing row, update that memory instead of inserting a new one.",
    )

    @field_validator("tags")
    @classmethod
    def validate_tags(cls, value: list[str]) -> list[str]:
        cleaned: list[str] = []
        seen: set[str] = set()
        for item in value:
            tag = item.strip().lower()
            if not tag:
                continue
            if len(tag) > 60:
                raise ValueError("Tags must be 60 characters or fewer.")
            if tag not in seen:
                seen.add(tag)
                cleaned.append(tag)
        return cleaned


class MemoryRecord(BaseModel):
    """Stored memory."""

    model_config = ConfigDict(extra="forbid")

    id: str
    namespace: str
    kind: str
    title: str | None
    summary: str | None
    content: str
    tags: list[str]
    source: str | None
    source_ref: str | None
    confidence: float | None
    importance: int
    metadata: dict[str, Any]
    valid_from: str | None
    valid_until: str | None
    archived_at: str | None
    created_at: str
    updated_at: str
    rank: float | None = None


class SearchInput(BaseModel):
    """Input model for querying memories."""

    model_config = ConfigDict(
        extra="forbid",
        str_strip_whitespace=True,
        validate_assignment=True,
    )

    query: str | None = Field(
        default=None,
        max_length=500,
        description="Optional full-text query. If omitted, the most recent memories are returned.",
    )
    namespace: str | None = Field(
        default=None,
        max_length=120,
        description="Optional namespace filter.",
    )
    kind: str | None = Field(
        default=None,
        max_length=50,
        description="Optional memory kind filter.",
    )
    tags: list[str] = Field(
        default_factory=list,
        description="Optional tag filter.",
        max_length=20,
    )
    match_all_tags: bool = Field(
        default=True,
        description="If true, all tags must be present. If false, any matching tag is enough.",
    )
    include_archived: bool = Field(
        default=False,
        description="Whether to include archived memories in results.",
    )
    limit: int = Field(
        default=10,
        ge=1,
        le=100,
        description="Maximum number of memories to return.",
    )
    offset: int = Field(
        default=0,
        ge=0,
        description="Number of results to skip.",
    )
    response_format: ResponseFormat = Field(
        default=ResponseFormat.MARKDOWN,
        description="Return JSON for machine consumption or Markdown for human-readable output.",
    )

    @field_validator("tags")
    @classmethod
    def validate_tags(cls, value: list[str]) -> list[str]:
        return MemoryInput.validate_tags(value)


class LinkInput(BaseModel):
    """Input model for linking memories."""

    model_config = ConfigDict(extra="forbid", str_strip_whitespace=True)

    from_memory_id: str = Field(..., min_length=1, max_length=80)
    to_memory_id: str = Field(..., min_length=1, max_length=80)
    relationship: str = Field(
        default="relates_to",
        min_length=1,
        max_length=60,
        description="Typed relationship such as relates_to, supports, supersedes, or derived_from.",
    )
    metadata: dict[str, Any] = Field(default_factory=dict)


class ArchiveInput(BaseModel):
    """Input model for archiving a memory."""

    model_config = ConfigDict(extra="forbid")

    memory_id: str = Field(..., min_length=1, max_length=80)


class RecallResult(BaseModel):
    """Response envelope for recall queries."""

    model_config = ConfigDict(extra="forbid")

    total: int
    count: int
    offset: int
    limit: int
    items: list[MemoryRecord]
