"""SQLite-backed memory store."""

from __future__ import annotations

import json
import sqlite3
import uuid
from contextlib import contextmanager
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterator

from .models import LinkInput, MemoryInput, MemoryRecord, RecallResult, SearchInput


MIGRATIONS: list[tuple[str, str]] = [
    (
        "001_init",
        """
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memories (
            id TEXT NOT NULL UNIQUE,
            namespace TEXT NOT NULL DEFAULT 'global',
            kind TEXT NOT NULL DEFAULT 'note',
            title TEXT,
            summary TEXT,
            content TEXT NOT NULL,
            tags_text TEXT NOT NULL DEFAULT '',
            source TEXT,
            source_ref TEXT,
            confidence REAL,
            importance INTEGER NOT NULL DEFAULT 3,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            valid_from TEXT,
            valid_until TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_id ON memories(id);
        CREATE INDEX IF NOT EXISTS idx_memories_namespace ON memories(namespace);
        CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);
        CREATE INDEX IF NOT EXISTS idx_memories_updated_at ON memories(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_memories_archived_at ON memories(archived_at);
        CREATE INDEX IF NOT EXISTS idx_memories_source ON memories(source);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_source_ref
            ON memories(source, source_ref)
            WHERE source IS NOT NULL AND source_ref IS NOT NULL;

        CREATE TABLE IF NOT EXISTS memory_tags (
            memory_id TEXT NOT NULL,
            tag TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (memory_id, tag),
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_memory_tags_tag ON memory_tags(tag);

        CREATE TABLE IF NOT EXISTS memory_links (
            from_memory_id TEXT NOT NULL,
            to_memory_id TEXT NOT NULL,
            relationship TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            PRIMARY KEY (from_memory_id, to_memory_id, relationship),
            FOREIGN KEY (from_memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (to_memory_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_memory_links_from ON memory_links(from_memory_id);
        CREATE INDEX IF NOT EXISTS idx_memory_links_to ON memory_links(to_memory_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
            title,
            summary,
            content,
            tags_text,
            content='memories',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );

        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memory_fts(rowid, title, summary, content, tags_text)
            VALUES (new.rowid, new.title, new.summary, new.content, new.tags_text);
        END;

        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memory_fts(memory_fts, rowid, title, summary, content, tags_text)
            VALUES ('delete', old.rowid, old.title, old.summary, old.content, old.tags_text);
        END;

        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memory_fts(memory_fts, rowid, title, summary, content, tags_text)
            VALUES ('delete', old.rowid, old.title, old.summary, old.content, old.tags_text);
            INSERT INTO memory_fts(rowid, title, summary, content, tags_text)
            VALUES (new.rowid, new.title, new.summary, new.content, new.tags_text);
        END;
        """,
    ),
]


def utc_now() -> str:
    """Return an ISO-8601 UTC timestamp."""
    return datetime.now(UTC).isoformat(timespec="seconds")


def make_memory_id() -> str:
    """Return a sortable UUID when available."""
    generator = getattr(uuid, "uuid7", uuid.uuid4)
    return str(generator())


class MemoryStore:
    """High-level access to the shared memory database."""

    def __init__(self, database_path: Path) -> None:
        self.database_path = Path(database_path)

    def initialize(self) -> dict[str, Any]:
        """Create directories, apply pragmas, and run migrations."""
        self.database_path.parent.mkdir(parents=True, exist_ok=True)
        with self.connection() as conn:
            self._configure_connection(conn)
            self._apply_migrations(conn)
            return {
                "database_path": str(self.database_path),
                "migrations": len(MIGRATIONS),
                "status": "ready",
            }

    @contextmanager
    def connection(self) -> Iterator[sqlite3.Connection]:
        """Yield a configured SQLite connection."""
        conn = sqlite3.connect(self.database_path)
        conn.row_factory = sqlite3.Row
        try:
            yield conn
            conn.commit()
        finally:
            conn.close()

    def _configure_connection(self, conn: sqlite3.Connection) -> None:
        conn.execute("PRAGMA journal_mode=WAL;")
        conn.execute("PRAGMA foreign_keys=ON;")
        conn.execute("PRAGMA busy_timeout=5000;")
        conn.execute("PRAGMA synchronous=NORMAL;")
        conn.execute("PRAGMA temp_store=MEMORY;")

    def _apply_migrations(self, conn: sqlite3.Connection) -> None:
        conn.execute(
            """
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            )
            """
        )
        applied = {
            row["version"]
            for row in conn.execute("SELECT version FROM schema_migrations").fetchall()
        }
        for version, sql in MIGRATIONS:
            if version in applied:
                continue
            conn.executescript(sql)
            conn.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?, ?)",
                (version, utc_now()),
            )

    def remember(self, data: MemoryInput) -> MemoryRecord:
        """Insert or update a memory."""
        self.initialize()
        existing_id: str | None = None
        if data.upsert and data.source and data.source_ref:
            with self.connection() as conn:
                self._configure_connection(conn)
                existing = conn.execute(
                    """
                    SELECT id
                    FROM memories
                    WHERE source = ? AND source_ref = ?
                    LIMIT 1
                    """,
                    (data.source, data.source_ref),
                ).fetchone()
                if existing:
                    existing_id = str(existing["id"])

        if existing_id:
            return self.update(existing_id, data)

        memory_id = make_memory_id()
        now = utc_now()
        payload = data.model_dump()
        tags_text = " ".join(payload["tags"])
        with self.connection() as conn:
            self._configure_connection(conn)
            conn.execute(
                """
                INSERT INTO memories (
                    id, namespace, kind, title, summary, content, tags_text,
                    source, source_ref, confidence, importance, metadata_json,
                    valid_from, valid_until, archived_at, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
                """,
                (
                    memory_id,
                    payload["namespace"],
                    payload["kind"],
                    payload["title"],
                    payload["summary"],
                    payload["content"],
                    tags_text,
                    payload["source"],
                    payload["source_ref"],
                    payload["confidence"],
                    payload["importance"],
                    json.dumps(payload["metadata"], sort_keys=True),
                    self._serialize_datetime(payload["valid_from"]),
                    self._serialize_datetime(payload["valid_until"]),
                    now,
                    now,
                ),
            )
            self._replace_tags(conn, memory_id, payload["tags"])
        return self.get(memory_id)

    def update(self, memory_id: str, data: MemoryInput) -> MemoryRecord:
        """Update an existing memory in place."""
        now = utc_now()
        payload = data.model_dump()
        tags_text = " ".join(payload["tags"])
        with self.connection() as conn:
            self._configure_connection(conn)
            updated = conn.execute(
                """
                UPDATE memories
                SET namespace = ?,
                    kind = ?,
                    title = ?,
                    summary = ?,
                    content = ?,
                    tags_text = ?,
                    source = ?,
                    source_ref = ?,
                    confidence = ?,
                    importance = ?,
                    metadata_json = ?,
                    valid_from = ?,
                    valid_until = ?,
                    updated_at = ?
                WHERE id = ?
                """,
                (
                    payload["namespace"],
                    payload["kind"],
                    payload["title"],
                    payload["summary"],
                    payload["content"],
                    tags_text,
                    payload["source"],
                    payload["source_ref"],
                    payload["confidence"],
                    payload["importance"],
                    json.dumps(payload["metadata"], sort_keys=True),
                    self._serialize_datetime(payload["valid_from"]),
                    self._serialize_datetime(payload["valid_until"]),
                    now,
                    memory_id,
                ),
            )
            if updated.rowcount == 0:
                msg = f"Memory '{memory_id}' was not found."
                raise KeyError(msg)
            self._replace_tags(conn, memory_id, payload["tags"])
        return self.get(memory_id)

    def get(self, memory_id: str) -> MemoryRecord:
        """Fetch a single memory."""
        self.initialize()
        with self.connection() as conn:
            self._configure_connection(conn)
            row = conn.execute(
                """
                SELECT *
                FROM memories
                WHERE id = ?
                LIMIT 1
                """,
                (memory_id,),
            ).fetchone()
            if row is None:
                msg = f"Memory '{memory_id}' was not found."
                raise KeyError(msg)
            return self._row_to_memory(conn, row)

    def archive(self, memory_id: str) -> MemoryRecord:
        """Soft-archive a memory."""
        self.initialize()
        archived_at = utc_now()
        with self.connection() as conn:
            self._configure_connection(conn)
            result = conn.execute(
                """
                UPDATE memories
                SET archived_at = ?, updated_at = ?
                WHERE id = ?
                """,
                (archived_at, archived_at, memory_id),
            )
            if result.rowcount == 0:
                msg = f"Memory '{memory_id}' was not found."
                raise KeyError(msg)
        return self.get(memory_id)

    def link(self, data: LinkInput) -> dict[str, Any]:
        """Create or replace a typed link between two memories."""
        self.initialize()
        now = utc_now()
        with self.connection() as conn:
            self._configure_connection(conn)
            for memory_id in (data.from_memory_id, data.to_memory_id):
                found = conn.execute(
                    "SELECT 1 FROM memories WHERE id = ? LIMIT 1",
                    (memory_id,),
                ).fetchone()
                if found is None:
                    msg = f"Memory '{memory_id}' was not found."
                    raise KeyError(msg)

            conn.execute(
                """
                INSERT OR REPLACE INTO memory_links (
                    from_memory_id, to_memory_id, relationship, metadata_json, created_at
                ) VALUES (?, ?, ?, ?, ?)
                """,
                (
                    data.from_memory_id,
                    data.to_memory_id,
                    data.relationship,
                    json.dumps(data.metadata, sort_keys=True),
                    now,
                ),
            )
        return {
            "from_memory_id": data.from_memory_id,
            "to_memory_id": data.to_memory_id,
            "relationship": data.relationship,
            "metadata": data.metadata,
            "created_at": now,
        }

    def search(self, data: SearchInput) -> RecallResult:
        """Search or list memories."""
        self.initialize()
        with self.connection() as conn:
            self._configure_connection(conn)
            if data.query:
                items = self._search_fts(conn, data)
            else:
                items = self._list_recent(conn, data)
            total = self._count_results(conn, data)
            return RecallResult(
                total=total,
                count=len(items),
                offset=data.offset,
                limit=data.limit,
                items=items,
            )

    def export_jsonl(self, output_path: Path) -> dict[str, Any]:
        """Export all memories as JSONL."""
        self.initialize()
        output_path.parent.mkdir(parents=True, exist_ok=True)
        count = 0
        with self.connection() as conn, output_path.open("w", encoding="utf-8") as handle:
            self._configure_connection(conn)
            rows = conn.execute(
                "SELECT * FROM memories ORDER BY created_at ASC"
            ).fetchall()
            for row in rows:
                record = self._row_to_memory(conn, row)
                handle.write(record.model_dump_json())
                handle.write("\n")
                count += 1
        return {"output_path": str(output_path), "count": count}

    def schema_snapshot(self) -> dict[str, Any]:
        """Return schema and migration metadata for inspection."""
        self.initialize()
        with self.connection() as conn:
            self._configure_connection(conn)
            tables = [
                dict(row)
                for row in conn.execute(
                    """
                    SELECT name, type
                    FROM sqlite_master
                    WHERE type IN ('table', 'view')
                    ORDER BY name
                    """
                ).fetchall()
            ]
            migrations = [
                dict(row)
                for row in conn.execute(
                    "SELECT version, applied_at FROM schema_migrations ORDER BY version"
                ).fetchall()
            ]
        return {"tables": tables, "migrations": migrations}

    def _replace_tags(
        self,
        conn: sqlite3.Connection,
        memory_id: str,
        tags: list[str],
    ) -> None:
        conn.execute("DELETE FROM memory_tags WHERE memory_id = ?", (memory_id,))
        now = utc_now()
        conn.executemany(
            "INSERT INTO memory_tags(memory_id, tag, created_at) VALUES (?, ?, ?)",
            [(memory_id, tag, now) for tag in tags],
        )
        conn.execute(
            "UPDATE memories SET tags_text = ?, updated_at = ? WHERE id = ?",
            (" ".join(tags), now, memory_id),
        )

    def _search_fts(
        self,
        conn: sqlite3.Connection,
        data: SearchInput,
    ) -> list[MemoryRecord]:
        query = """
            SELECT m.*, bm25(memory_fts, 4.0, 2.0, 1.0, 0.5) AS rank
            FROM memory_fts
            JOIN memories m ON m.rowid = memory_fts.rowid
        """
        filters = ["memory_fts MATCH ?"]
        params: list[Any] = [data.query]
        query += self._build_filter_sql(filters, params, data)
        query += " ORDER BY rank ASC, m.updated_at DESC LIMIT ? OFFSET ?"
        params.extend([data.limit, data.offset])
        rows = conn.execute(query, params).fetchall()
        return [self._row_to_memory(conn, row) for row in rows]

    def _list_recent(
        self,
        conn: sqlite3.Connection,
        data: SearchInput,
    ) -> list[MemoryRecord]:
        query = "SELECT m.*, NULL AS rank FROM memories m"
        filters: list[str] = []
        params: list[Any] = []
        query += self._build_filter_sql(filters, params, data)
        query += " ORDER BY m.updated_at DESC LIMIT ? OFFSET ?"
        params.extend([data.limit, data.offset])
        rows = conn.execute(query, params).fetchall()
        return [self._row_to_memory(conn, row) for row in rows]

    def _count_results(self, conn: sqlite3.Connection, data: SearchInput) -> int:
        if data.query:
            query = """
                SELECT COUNT(DISTINCT m.id) AS total
                FROM memory_fts
                JOIN memories m ON m.rowid = memory_fts.rowid
            """
            filters = ["memory_fts MATCH ?"]
            params: list[Any] = [data.query]
        else:
            query = "SELECT COUNT(DISTINCT m.id) AS total FROM memories m"
            filters = []
            params = []
        query += self._build_filter_sql(filters, params, data, include_order=False)
        row = conn.execute(query, params).fetchone()
        return int(row["total"]) if row else 0

    def _build_filter_sql(
        self,
        filters: list[str],
        params: list[Any],
        data: SearchInput,
        *,
        include_order: bool = True,
    ) -> str:
        if not data.include_archived:
            filters.append("m.archived_at IS NULL")
        if data.namespace:
            filters.append("m.namespace = ?")
            params.append(data.namespace)
        if data.kind:
            filters.append("m.kind = ?")
            params.append(data.kind)
        tag_filter = self._tag_filter_sql(data, params)
        if tag_filter:
            filters.append(tag_filter)
        if not filters:
            return ""
        prefix = " WHERE " if include_order else " WHERE "
        return prefix + " AND ".join(filters)

    def _tag_filter_sql(self, data: SearchInput, params: list[Any]) -> str | None:
        if not data.tags:
            return None
        placeholders = ", ".join("?" for _ in data.tags)
        params.extend(data.tags)
        if data.match_all_tags:
            return (
                "m.id IN ("
                "SELECT memory_id FROM memory_tags "
                f"WHERE tag IN ({placeholders}) "
                "GROUP BY memory_id "
                f"HAVING COUNT(DISTINCT tag) = {len(data.tags)})"
            )
        return (
            "EXISTS ("
            "SELECT 1 FROM memory_tags "
            "WHERE memory_tags.memory_id = m.id "
            f"AND tag IN ({placeholders}))"
        )

    def _row_to_memory(
        self,
        conn: sqlite3.Connection,
        row: sqlite3.Row,
    ) -> MemoryRecord:
        tags = [
            str(item["tag"])
            for item in conn.execute(
                "SELECT tag FROM memory_tags WHERE memory_id = ? ORDER BY tag ASC",
                (row["id"],),
            ).fetchall()
        ]
        return MemoryRecord(
            id=str(row["id"]),
            namespace=str(row["namespace"]),
            kind=str(row["kind"]),
            title=row["title"],
            summary=row["summary"],
            content=str(row["content"]),
            tags=tags,
            source=row["source"],
            source_ref=row["source_ref"],
            confidence=row["confidence"],
            importance=int(row["importance"]),
            metadata=json.loads(row["metadata_json"] or "{}"),
            valid_from=row["valid_from"],
            valid_until=row["valid_until"],
            archived_at=row["archived_at"],
            created_at=str(row["created_at"]),
            updated_at=str(row["updated_at"]),
            rank=float(row["rank"]) if row["rank"] is not None else None,
        )

    def _serialize_datetime(self, value: datetime | None) -> str | None:
        if value is None:
            return None
        if value.tzinfo is None:
            return value.replace(tzinfo=UTC).isoformat(timespec="seconds")
        return value.astimezone(UTC).isoformat(timespec="seconds")
