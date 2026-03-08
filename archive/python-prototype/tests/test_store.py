from __future__ import annotations

import json

from ai_shared_memory.models import LinkInput, MemoryInput, SearchInput
from ai_shared_memory.store import MemoryStore


def test_remember_and_recall(tmp_path):
    store = MemoryStore(tmp_path / "memory.db")
    first = store.remember(
        MemoryInput(
            namespace="project:test",
            kind="decision",
            title="Choose SQLite",
            content="SQLite is the default store for local shared memory.",
            tags=["sqlite", "memory"],
            source="pytest",
            source_ref="decision-1",
        )
    )

    assert first.id
    result = store.search(SearchInput(query="sqlite", namespace="project:test"))
    assert result.count == 1
    assert result.items[0].id == first.id


def test_upsert_reuses_existing_source_ref(tmp_path):
    store = MemoryStore(tmp_path / "memory.db")
    first = store.remember(
        MemoryInput(
            namespace="global",
            kind="fact",
            title="Original",
            content="First version",
            tags=["fact"],
            source="pytest",
            source_ref="fact-1",
            upsert=True,
        )
    )
    second = store.remember(
        MemoryInput(
            namespace="global",
            kind="fact",
            title="Updated",
            content="Second version",
            tags=["fact", "updated"],
            source="pytest",
            source_ref="fact-1",
            upsert=True,
        )
    )

    assert second.id == first.id
    assert second.title == "Updated"
    assert "updated" in second.tags


def test_link_and_archive(tmp_path):
    store = MemoryStore(tmp_path / "memory.db")
    a = store.remember(MemoryInput(content="A"))
    b = store.remember(MemoryInput(content="B"))

    link = store.link(
        LinkInput(
            from_memory_id=a.id,
            to_memory_id=b.id,
            relationship="supports",
            metadata={"why": "context"},
        )
    )
    archived = store.archive(a.id)

    assert link["relationship"] == "supports"
    assert json.loads(json.dumps(link["metadata"])) == {"why": "context"}
    assert archived.archived_at is not None
