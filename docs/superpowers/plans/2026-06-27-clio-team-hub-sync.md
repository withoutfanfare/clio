# Clio Team Hub Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first production-shaped Clio Team Hub slice: explicit namespace sync between local Clio workstations and an authenticated remote HTTP hub.

**Architecture:** Local Clio remains SQLite-first and useful offline. `clio-core` owns sync state, event logging, conflict rules, and remote-event application; CLI and hub binaries are thin adapters. A new `clio-hub` HTTP service stores shared memories by replaying the same event contract through `clio-core`, while workstations push and pull only namespaces they have explicitly subscribed to.

**Tech Stack:** Rust 1.85, SQLite via `rusqlite`, `reqwest` for the local HTTP client, `axum` + `tokio` for the hub API, existing `serde`/`serde_json`/`time`/`uuid` crates, `cargo test`.

## Global Constraints

- Clio remains local-first. No remote sync runs unless the user explicitly configures a remote and namespace subscription.
- Do not expose the existing local daemon outside localhost. Team Hub is a separate binary and service boundary.
- All business logic lives in `crates/clio-core`; CLI and hub code parse requests, call core, and render responses.
- Archive means hidden, not deleted. Sync archive events by setting `archived_at`; do not translate archive into hard delete.
- Tags and FTS data must stay in sync. Any remote-applied write must use core repository helpers that refresh `memory_tags`, `tags_text`, and FTS triggers.
- Upserts keyed by `source + source_ref` must not create duplicates. Remote application must resolve matching `source + source_ref` before inserting a conflicting row.
- Access tracking remains fire-and-forget and must not generate sync events.
- Auto-links keep the `auto:relates_to` prefix distinction.
- Sync uses additive migrations only. Do not edit migrations `001_initial` through `006_scoped_recall_indexes`.
- Secrets are not stored directly in settings. Remote tokens are read from an environment variable named in settings.
- British English in documentation, comments, and user-facing text.
- Every task must leave `cargo test -p clio-core` green. Tasks touching CLI or hub must also run the package-specific tests listed in that task.

---

## Scope

This plan implements the first usable sync slice:

- Local event log for explicit memory write operations.
- Per-remote, per-namespace sync checkpoints.
- Core transport abstraction and sync orchestration.
- HTTP client for the workstation.
- CLI commands for configuring and running sync manually.
- Minimal authenticated hub API that stores shared memory in a normal Clio database.
- Documentation for operating a hub and connecting workstations.

This plan deliberately excludes live background sync, team admin UI, user invitations, per-memory ACLs, Slack/Linear/GitHub connectors, and remote semantic search fallback. Those depend on this event/checkpoint foundation.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/clio-core/src/migrations.rs` | Adds migration `007_team_sync` for event log, sync checkpoints, remote configuration, namespace subscriptions, and event deduplication. |
| `crates/clio-core/src/models.rs` | Adds sync-facing domain structs: `SyncEvent`, `SyncEventType`, `SyncRemote`, `SyncNamespaceSubscription`, `SyncMode`, `SyncCheckpoint`, `SyncBatch`, `SyncReport`. |
| `crates/clio-core/src/sync.rs` | New core module for recording events, listing pending events, applying remote events, updating checkpoints, and running push/pull orchestration. |
| `crates/clio-core/src/sync_http.rs` | New optional HTTP transport implementation using `reqwest`; compiled behind the `sync-http` feature. |
| `crates/clio-core/src/repository.rs` | Emits sync events from write paths and exposes a remote-apply helper that can preserve remote IDs while respecting `source + source_ref`. |
| `crates/clio-core/src/settings.rs` | Adds `sync` settings with `node_id` and default batch size. Remote secrets stay out of settings. |
| `crates/clio-core/src/lib.rs` | Exposes `sync` and feature-gated `sync_http`. |
| `crates/clio-core/Cargo.toml` | Adds the `sync-http` feature and optional `reqwest`/`tokio` linkage if needed. |
| `crates/clio-cli/src/main.rs` | Adds `clio sync remote add`, `clio sync namespace add`, `clio sync status`, `clio sync push`, `clio sync pull`, and `clio sync run`. |
| `crates/clio-hub/Cargo.toml` | New HTTP hub crate. |
| `crates/clio-hub/src/main.rs` | Thin Axum adapter exposing health, push, and pull endpoints over `clio-core`. |
| `Cargo.toml` | Adds `crates/clio-hub` to the workspace and shared `axum`/`tower-http` dependencies if used. |
| `docs/reference/schema.md` | Documents the new sync tables and migration. |
| `docs/reference/settings.md` | Documents sync settings and token-env behaviour. |
| `docs/reference/team-hub-api.md` | New API contract for hub endpoints. |
| `docs/team-hub.md` | Operator guide for running a hub and connecting workstations. |

---

## Task 1: Add Sync Schema And Domain Types

**Files:**
- Modify: `crates/clio-core/src/migrations.rs`
- Modify: `crates/clio-core/src/models.rs`
- Modify: `crates/clio-core/src/settings.rs`
- Modify: `crates/clio-core/src/lib.rs`
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Produces: `SyncEvent`, `SyncEventType`, `SyncMode`, `SyncRemote`, `SyncNamespaceSubscription`, `SyncCheckpoint`, `SyncBatch`, `SyncReport`.
- Produces tables: `memory_events`, `sync_remotes`, `sync_namespace_subscriptions`, `sync_checkpoints`, `sync_applied_events`.
- Consumed by: Tasks 2 through 7.

- [ ] **Step 1: Write failing migration test**

Add this test to `crates/clio-core/tests/integration.rs`:

```rust
#[test]
fn migration_creates_team_sync_tables() {
    let conn = setup();

    let tables = [
        "memory_events",
        "sync_remotes",
        "sync_namespace_subscriptions",
        "sync_checkpoints",
        "sync_applied_events",
    ];

    for table in tables {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
}
```

Run: `cargo test -p clio-core migration_creates_team_sync_tables`

Expected: FAIL because the tables do not exist.

- [ ] **Step 2: Add migration `007_team_sync`**

Append this migration to `MIGRATIONS` in `crates/clio-core/src/migrations.rs`:

```rust
Migration {
    version: "007_team_sync",
    sql: r#"
        CREATE TABLE memory_events (
            id TEXT PRIMARY KEY,
            memory_id TEXT,
            namespace TEXT NOT NULL,
            event_type TEXT NOT NULL CHECK(event_type IN (
                'memory_upserted',
                'memory_archived',
                'memory_unarchived',
                'memory_deleted',
                'memory_linked'
            )),
            payload_json TEXT NOT NULL,
            origin_node_id TEXT NOT NULL,
            origin_event_id TEXT,
            created_at TEXT NOT NULL,
            pushed_at TEXT,
            CHECK (length(namespace) BETWEEN 1 AND 120),
            CHECK (json_valid(payload_json))
        );

        CREATE INDEX idx_memory_events_push
            ON memory_events(namespace, pushed_at, created_at)
            WHERE pushed_at IS NULL;

        CREATE INDEX idx_memory_events_created
            ON memory_events(namespace, created_at, id);

        CREATE TABLE sync_remotes (
            name TEXT PRIMARY KEY,
            base_url TEXT NOT NULL,
            token_env TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            CHECK (length(name) BETWEEN 1 AND 80),
            CHECK (length(base_url) BETWEEN 1 AND 500),
            CHECK (length(token_env) BETWEEN 1 AND 120)
        );

        CREATE TABLE sync_namespace_subscriptions (
            remote_name TEXT NOT NULL,
            namespace TEXT NOT NULL,
            mode TEXT NOT NULL CHECK(mode IN ('push_pull', 'push_only', 'pull_only')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (remote_name, namespace),
            FOREIGN KEY (remote_name) REFERENCES sync_remotes(name) ON DELETE CASCADE,
            CHECK (length(namespace) BETWEEN 1 AND 120)
        );

        CREATE TABLE sync_checkpoints (
            remote_name TEXT NOT NULL,
            namespace TEXT NOT NULL,
            pull_cursor TEXT,
            last_push_at TEXT,
            last_pull_at TEXT,
            last_error TEXT,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (remote_name, namespace),
            FOREIGN KEY (remote_name) REFERENCES sync_remotes(name) ON DELETE CASCADE
        );

        CREATE TABLE sync_applied_events (
            remote_name TEXT NOT NULL,
            origin_node_id TEXT NOT NULL,
            origin_event_id TEXT NOT NULL,
            local_event_id TEXT,
            applied_at TEXT NOT NULL,
            PRIMARY KEY (remote_name, origin_node_id, origin_event_id)
        );
    "#,
},
```

Run: `cargo test -p clio-core migration_creates_team_sync_tables`

Expected: PASS.

- [ ] **Step 3: Add sync domain structs**

Add these definitions near the other model structs in `crates/clio-core/src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncEventType {
    MemoryUpserted,
    MemoryArchived,
    MemoryUnarchived,
    MemoryDeleted,
    MemoryLinked,
}

impl SyncEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MemoryUpserted => "memory_upserted",
            Self::MemoryArchived => "memory_archived",
            Self::MemoryUnarchived => "memory_unarchived",
            Self::MemoryDeleted => "memory_deleted",
            Self::MemoryLinked => "memory_linked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEvent {
    pub id: String,
    pub memory_id: Option<String>,
    pub namespace: String,
    pub event_type: SyncEventType,
    pub payload: serde_json::Value,
    pub origin_node_id: String,
    pub origin_event_id: Option<String>,
    pub created_at: String,
    pub pushed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    PushPull,
    PushOnly,
    PullOnly,
}

impl SyncMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PushPull => "push_pull",
            Self::PushOnly => "push_only",
            Self::PullOnly => "pull_only",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRemote {
    pub name: String,
    pub base_url: String,
    pub token_env: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncNamespaceSubscription {
    pub remote_name: String,
    pub namespace: String,
    pub mode: SyncMode,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCheckpoint {
    pub remote_name: String,
    pub namespace: String,
    pub pull_cursor: Option<String>,
    pub last_push_at: Option<String>,
    pub last_pull_at: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBatch {
    pub remote_name: String,
    pub namespace: String,
    pub events: Vec<SyncEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncReport {
    pub remote_name: String,
    pub namespace: String,
    pub pushed: usize,
    pub pulled: usize,
    pub skipped: usize,
}
```

- [ ] **Step 4: Add sync settings**

Add this to `crates/clio-core/src/settings.rs` before `Settings`:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncConfig {
    /// Stable local node id used to prevent echoing remote events back forever.
    #[serde(default)]
    pub node_id: Option<String>,

    /// Maximum number of events per push or pull request.
    #[serde(default = "default_sync_batch_size")]
    pub batch_size: u32,
}

fn default_sync_batch_size() -> u32 {
    100
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            node_id: None,
            batch_size: default_sync_batch_size(),
        }
    }
}
```

Add a `sync` field to `Settings`:

```rust
#[serde(default)]
pub sync: SyncConfig,
```

Add it to `Default for Settings`:

```rust
sync: SyncConfig::default(),
```

- [ ] **Step 5: Register the module**

Add this to `crates/clio-core/src/lib.rs`:

```rust
pub mod sync;
```

Create `crates/clio-core/src/sync.rs` with only imports and a module-level comment for now:

```rust
//! Team Hub sync primitives.
//!
//! This module owns local sync state, event logging, remote event application,
//! and sync orchestration. Adapters must not write sync tables directly.
```

- [ ] **Step 6: Run the core tests**

Run: `cargo test -p clio-core`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/clio-core/src/migrations.rs crates/clio-core/src/models.rs crates/clio-core/src/settings.rs crates/clio-core/src/lib.rs crates/clio-core/src/sync.rs crates/clio-core/tests/integration.rs
git commit -m "feat(core): add team sync schema"
```

---

## Task 2: Record Local Memory Events

**Files:**
- Modify: `crates/clio-core/src/sync.rs`
- Modify: `crates/clio-core/src/repository.rs`
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Produces: `sync::record_memory_event(conn, node_id, event_type, memory_id, namespace, payload) -> Result<SyncEvent>`.
- Produces: `sync::pending_events(conn, namespace, limit) -> Result<Vec<SyncEvent>>`.
- Consumed by: Tasks 3, 4, 5, and 6.

- [ ] **Step 1: Write failing event-log test**

Add this to `crates/clio-core/tests/integration.rs`:

```rust
#[test]
fn remember_records_pending_sync_event() {
    let conn = setup();

    let memory = clio_core::repository::remember(
        &conn,
        clio_core::models::RememberInput {
            namespace: "project:clio".into(),
            kind: "decision".into(),
            title: Some("Share memory through Team Hub".into()),
            summary: None,
            content: "Team-visible memories sync through explicit namespaces.".into(),
            tags: vec!["sync".into()],
            source: Some("codex".into()),
            source_ref: Some("team-hub-plan".into()),
            confidence: Some(0.95),
            importance: 4,
            metadata: serde_json::json!({ "origin": "test" }),
            valid_from: None,
            valid_until: None,
            upsert: true,
        },
    )
    .unwrap();

    let events = clio_core::sync::pending_events(&conn, "project:clio", 10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].memory_id.as_deref(), Some(memory.id.as_str()));
    assert_eq!(events[0].namespace, "project:clio");
    assert_eq!(
        events[0].event_type,
        clio_core::models::SyncEventType::MemoryUpserted
    );
    assert_eq!(events[0].payload["memory"]["content"], memory.content);
}
```

Run: `cargo test -p clio-core remember_records_pending_sync_event`

Expected: FAIL because `sync::pending_events` is not implemented and `repository::remember` does not record events.

- [ ] **Step 2: Implement sync event helpers**

Replace the contents of `crates/clio-core/src/sync.rs` with:

```rust
//! Team Hub sync primitives.

use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{now_utc, Memory, SyncEvent, SyncEventType};

const DEFAULT_NODE_ID: &str = "local";

pub fn record_memory_event(
    conn: &Connection,
    node_id: Option<&str>,
    event_type: SyncEventType,
    memory_id: Option<&str>,
    namespace: &str,
    payload: serde_json::Value,
) -> Result<SyncEvent> {
    let event = SyncEvent {
        id: Uuid::now_v7().to_string(),
        memory_id: memory_id.map(str::to_string),
        namespace: namespace.to_string(),
        event_type,
        payload,
        origin_node_id: node_id.unwrap_or(DEFAULT_NODE_ID).to_string(),
        origin_event_id: None,
        created_at: now_utc(),
        pushed_at: None,
    };

    conn.execute(
        "INSERT INTO memory_events
            (id, memory_id, namespace, event_type, payload_json, origin_node_id,
             origin_event_id, created_at, pushed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
        params![
            event.id,
            event.memory_id,
            event.namespace,
            event.event_type.as_str(),
            serde_json::to_string(&event.payload)?,
            event.origin_node_id,
            event.origin_event_id,
            event.created_at,
        ],
    )?;

    Ok(event)
}

pub fn event_payload_for_memory(memory: &Memory) -> serde_json::Value {
    serde_json::json!({ "memory": memory })
}

pub fn pending_events(conn: &Connection, namespace: &str, limit: u32) -> Result<Vec<SyncEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, memory_id, namespace, event_type, payload_json,
                origin_node_id, origin_event_id, created_at, pushed_at
         FROM memory_events
         WHERE namespace = ?1 AND pushed_at IS NULL
         ORDER BY created_at ASC, id ASC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![namespace, limit], sync_event_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn sync_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SyncEvent> {
    let event_type_raw: String = row.get(3)?;
    let event_type = match event_type_raw.as_str() {
        "memory_upserted" => SyncEventType::MemoryUpserted,
        "memory_archived" => SyncEventType::MemoryArchived,
        "memory_unarchived" => SyncEventType::MemoryUnarchived,
        "memory_deleted" => SyncEventType::MemoryDeleted,
        "memory_linked" => SyncEventType::MemoryLinked,
        _ => SyncEventType::MemoryUpserted,
    };

    let payload_json: String = row.get(4)?;
    let payload = serde_json::from_str(&payload_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })?;

    Ok(SyncEvent {
        id: row.get(0)?,
        memory_id: row.get(1)?,
        namespace: row.get(2)?,
        event_type,
        payload,
        origin_node_id: row.get(5)?,
        origin_event_id: row.get(6)?,
        created_at: row.get(7)?,
        pushed_at: row.get(8)?,
    })
}
```

- [ ] **Step 3: Emit events from `remember`**

In `crates/clio-core/src/repository.rs`, after a successful insert or update returns the final `Memory`, call:

```rust
let memory = get(conn, &id)?;
let _event = crate::sync::record_memory_event(
    conn,
    None,
    crate::models::SyncEventType::MemoryUpserted,
    Some(&memory.id),
    &memory.namespace,
    crate::sync::event_payload_for_memory(&memory),
)?;
Ok(memory)
```

Do this only once per `remember` call, after tags are synced and the stored memory can be read back. Keep repository validation and upsert semantics unchanged.

- [ ] **Step 4: Run the event-log test**

Run: `cargo test -p clio-core remember_records_pending_sync_event`

Expected: PASS.

- [ ] **Step 5: Add archive/unarchive/delete event tests**

Add:

```rust
#[test]
fn lifecycle_operations_record_sync_events() {
    let conn = setup();
    let memory = remember_simple(&conn, "sync lifecycle");

    clio_core::repository::archive(&conn, &memory.id).unwrap();
    clio_core::repository::unarchive(&conn, &memory.id).unwrap();
    clio_core::repository::delete(&conn, &memory.id).unwrap();

    let events = clio_core::sync::pending_events(&conn, "global", 10).unwrap();
    let kinds: Vec<_> = events.iter().map(|e| e.event_type.clone()).collect();

    assert!(kinds.contains(&clio_core::models::SyncEventType::MemoryUpserted));
    assert!(kinds.contains(&clio_core::models::SyncEventType::MemoryArchived));
    assert!(kinds.contains(&clio_core::models::SyncEventType::MemoryUnarchived));
    assert!(kinds.contains(&clio_core::models::SyncEventType::MemoryDeleted));
}
```

Run: `cargo test -p clio-core lifecycle_operations_record_sync_events`

Expected: FAIL until lifecycle operations emit events.

- [ ] **Step 6: Emit lifecycle events**

In `repository::archive`, `repository::unarchive`, and `repository::delete`, call `record_memory_event` after the database mutation succeeds. Use a memory payload for archive/unarchive and a compact payload for delete:

```rust
serde_json::json!({
    "memory_id": memory.id,
    "namespace": memory.namespace,
    "deleted_at": crate::models::now_utc()
})
```

Do not emit events from `touch_accessed`; reads must not sync.

- [ ] **Step 7: Run core tests**

Run: `cargo test -p clio-core`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/clio-core/src/sync.rs crates/clio-core/src/repository.rs crates/clio-core/tests/integration.rs
git commit -m "feat(core): record sync events for memory writes"
```

---

## Task 3: Apply Remote Events Safely

**Files:**
- Modify: `crates/clio-core/src/sync.rs`
- Modify: `crates/clio-core/src/repository.rs`
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Produces: `sync::apply_remote_event(conn, remote_name, event) -> Result<bool>`.
- Produces: `repository::upsert_remote_memory(conn, memory) -> Result<Memory>`.
- Consumed by: Tasks 4 and 6.

- [ ] **Step 1: Write failing remote-apply test**

Add:

```rust
#[test]
fn apply_remote_event_preserves_archive_semantics() {
    let conn = setup();

    let remote_memory = clio_core::models::Memory {
        id: "019f0000-0000-7000-8000-000000000001".into(),
        namespace: "project:clio".into(),
        kind: "fact".into(),
        title: Some("Shared fact".into()),
        summary: None,
        content: "Remote memories can be applied locally.".into(),
        tags: vec!["sync".into()],
        source: Some("hub".into()),
        source_ref: Some("remote-fact-1".into()),
        confidence: Some(0.9),
        importance: 4,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        archived_at: Some("2026-06-27T12:00:00Z".into()),
        created_at: "2026-06-27T11:00:00Z".into(),
        updated_at: "2026-06-27T12:00:00Z".into(),
        last_accessed_at: None,
        access_count: 0,
    };

    let event = clio_core::models::SyncEvent {
        id: "remote-event-1".into(),
        memory_id: Some(remote_memory.id.clone()),
        namespace: remote_memory.namespace.clone(),
        event_type: clio_core::models::SyncEventType::MemoryUpserted,
        payload: serde_json::json!({ "memory": remote_memory }),
        origin_node_id: "workstation-b".into(),
        origin_event_id: Some("remote-event-1".into()),
        created_at: "2026-06-27T12:00:00Z".into(),
        pushed_at: None,
    };

    let applied = clio_core::sync::apply_remote_event(&conn, "team", &event).unwrap();
    assert!(applied);

    let hidden = clio_core::repository::recall(
        &conn,
        clio_core::models::RecallQuery {
            namespace: Some("project:clio".into()),
            query: Some("Remote memories".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hidden.items.len(), 0);

    let visible = clio_core::repository::recall(
        &conn,
        clio_core::models::RecallQuery {
            namespace: Some("project:clio".into()),
            query: Some("Remote memories".into()),
            include_archived: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(visible.items.len(), 1);
}
```

Run: `cargo test -p clio-core apply_remote_event_preserves_archive_semantics`

Expected: FAIL because remote apply does not exist.

- [ ] **Step 2: Add `upsert_remote_memory`**

In `crates/clio-core/src/repository.rs`, add a helper that preserves the remote memory ID when possible and resolves `source + source_ref` conflicts:

```rust
pub fn upsert_remote_memory(conn: &Connection, memory: &Memory) -> Result<Memory> {
    if let (Some(source), Some(source_ref)) = (&memory.source, &memory.source_ref) {
        if let Some(existing_id) = find_by_source_ref(conn, source, source_ref)? {
            update(
                conn,
                &existing_id,
                RememberInput {
                    namespace: memory.namespace.clone(),
                    kind: memory.kind.clone(),
                    title: memory.title.clone(),
                    summary: memory.summary.clone(),
                    content: memory.content.clone(),
                    tags: memory.tags.clone(),
                    source: memory.source.clone(),
                    source_ref: memory.source_ref.clone(),
                    confidence: memory.confidence,
                    importance: memory.importance,
                    metadata: memory.metadata.clone(),
                    valid_from: memory.valid_from.clone(),
                    valid_until: memory.valid_until.clone(),
                    upsert: false,
                },
            )?;
            return get(conn, &existing_id);
        }
    }

    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)",
        [&memory.id],
        |row| row.get(0),
    )?;

    if exists {
        update(conn, &memory.id, RememberInput {
            namespace: memory.namespace.clone(),
            kind: memory.kind.clone(),
            title: memory.title.clone(),
            summary: memory.summary.clone(),
            content: memory.content.clone(),
            tags: memory.tags.clone(),
            source: memory.source.clone(),
            source_ref: memory.source_ref.clone(),
            confidence: memory.confidence,
            importance: memory.importance,
            metadata: memory.metadata.clone(),
            valid_from: memory.valid_from.clone(),
            valid_until: memory.valid_until.clone(),
            upsert: false,
        })?;
        return get(conn, &memory.id);
    }

    conn.execute(
        "INSERT INTO memories
            (id, namespace, kind, title, summary, content, tags_text, source,
             source_ref, confidence, importance, metadata_json, valid_from,
             valid_until, archived_at, created_at, updated_at, last_accessed_at,
             access_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17, ?18)",
        rusqlite::params![
            memory.id,
            memory.namespace,
            memory.kind,
            memory.title,
            memory.summary,
            memory.content,
            memory.source,
            memory.source_ref,
            memory.confidence,
            memory.importance,
            serde_json::to_string(&memory.metadata)?,
            memory.valid_from,
            memory.valid_until,
            memory.archived_at,
            memory.created_at,
            memory.updated_at,
            memory.last_accessed_at,
            memory.access_count,
        ],
    )?;
    replace_tags(conn, &memory.id, &memory.tags)?;
    get(conn, &memory.id)
}
```

If `replace_tags` is private and shaped differently, extract the existing tag replacement block into a private helper and call it from both `update` and `upsert_remote_memory`.

- [ ] **Step 3: Add event dedup and remote apply**

In `crates/clio-core/src/sync.rs`, add:

```rust
pub fn apply_remote_event(conn: &Connection, remote_name: &str, event: &SyncEvent) -> Result<bool> {
    let origin_event_id = event
        .origin_event_id
        .as_deref()
        .unwrap_or(event.id.as_str());

    let already_applied: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sync_applied_events
            WHERE remote_name = ?1 AND origin_node_id = ?2 AND origin_event_id = ?3
         )",
        params![remote_name, event.origin_node_id, origin_event_id],
        |row| row.get(0),
    )?;

    if already_applied {
        return Ok(false);
    }

    match event.event_type {
        SyncEventType::MemoryUpserted => {
            let memory: Memory = serde_json::from_value(event.payload["memory"].clone())?;
            crate::repository::upsert_remote_memory(conn, &memory)?;
        }
        SyncEventType::MemoryArchived => {
            if let Some(id) = event.memory_id.as_deref() {
                let _ = crate::repository::archive(conn, id);
            }
        }
        SyncEventType::MemoryUnarchived => {
            if let Some(id) = event.memory_id.as_deref() {
                let _ = crate::repository::unarchive(conn, id);
            }
        }
        SyncEventType::MemoryDeleted => {
            if let Some(id) = event.memory_id.as_deref() {
                let _ = crate::repository::delete(conn, id);
            }
        }
        SyncEventType::MemoryLinked => {
            let from_id = event.payload["from_memory_id"].as_str();
            let to_id = event.payload["to_memory_id"].as_str();
            let relationship = event.payload["relationship"].as_str();
            if let (Some(from_id), Some(to_id), Some(relationship)) = (from_id, to_id, relationship) {
                let metadata = event
                    .payload
                    .get("metadata")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                crate::repository::link(conn, from_id, to_id, relationship, metadata)?;
            }
        }
    }

    conn.execute(
        "INSERT INTO sync_applied_events
            (remote_name, origin_node_id, origin_event_id, local_event_id, applied_at)
         VALUES (?1, ?2, ?3, NULL, ?4)",
        params![remote_name, event.origin_node_id, origin_event_id, now_utc()],
    )?;

    Ok(true)
}
```

Important follow-up inside this task: applying remote events must not create new pending local events. If repository functions emit events unconditionally, add a private `WriteOrigin` enum in `repository.rs` and route normal local writes through `WriteOrigin::Local`, remote writes through `WriteOrigin::Remote`. Only `Local` emits sync events.

- [ ] **Step 4: Run tests**

Run: `cargo test -p clio-core apply_remote_event_preserves_archive_semantics`

Expected: PASS.

Run: `cargo test -p clio-core`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/clio-core/src/sync.rs crates/clio-core/src/repository.rs crates/clio-core/tests/integration.rs
git commit -m "feat(core): apply remote sync events"
```

---

## Task 4: Add Core Sync Orchestration And HTTP Transport

**Files:**
- Modify: `crates/clio-core/src/sync.rs`
- Create: `crates/clio-core/src/sync_http.rs`
- Modify: `crates/clio-core/src/lib.rs`
- Modify: `crates/clio-core/Cargo.toml`
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Produces trait: `SyncTransport`.
- Produces orchestration functions: `sync::push_once`, `sync::pull_once`, `sync::run_once`.
- Produces HTTP client: `sync_http::HttpTeamHubClient`.
- Consumed by: CLI in Task 5 and hub integration checks in Task 7.

- [ ] **Step 1: Write failing orchestration test with fake transport**

Add:

```rust
#[test]
fn run_once_pushes_pending_events_and_marks_them_pushed() {
    let conn = setup();
    remember_simple(&conn, "sync me");

    struct FakeTransport {
        pushed: std::cell::RefCell<Vec<clio_core::models::SyncEvent>>,
    }

    impl clio_core::sync::SyncTransport for FakeTransport {
        fn push_events(
            &self,
            _remote_name: &str,
            _namespace: &str,
            events: &[clio_core::models::SyncEvent],
        ) -> clio_core::error::Result<()> {
            self.pushed.borrow_mut().extend_from_slice(events);
            Ok(())
        }

        fn pull_events(
            &self,
            _remote_name: &str,
            _namespace: &str,
            _cursor: Option<&str>,
            _limit: u32,
        ) -> clio_core::error::Result<clio_core::models::SyncBatch> {
            Ok(clio_core::models::SyncBatch {
                remote_name: "team".into(),
                namespace: "global".into(),
                events: vec![],
                next_cursor: None,
            })
        }
    }

    let transport = FakeTransport { pushed: std::cell::RefCell::new(vec![]) };
    let report = clio_core::sync::run_once(&conn, &transport, "team", "global", 100).unwrap();

    assert_eq!(report.pushed, 1);
    assert_eq!(transport.pushed.borrow().len(), 1);
    assert!(clio_core::sync::pending_events(&conn, "global", 10).unwrap().is_empty());
}
```

Run: `cargo test -p clio-core run_once_pushes_pending_events_and_marks_them_pushed`

Expected: FAIL because `SyncTransport` and `run_once` do not exist.

- [ ] **Step 2: Add transport trait and orchestration**

In `crates/clio-core/src/sync.rs`, add:

```rust
pub trait SyncTransport {
    fn push_events(&self, remote_name: &str, namespace: &str, events: &[SyncEvent]) -> Result<()>;

    fn pull_events(
        &self,
        remote_name: &str,
        namespace: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::models::SyncBatch>;
}

pub fn mark_events_pushed(conn: &Connection, event_ids: &[String]) -> Result<()> {
    if event_ids.is_empty() {
        return Ok(());
    }

    let now = now_utc();
    for id in event_ids {
        conn.execute(
            "UPDATE memory_events SET pushed_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
    }
    Ok(())
}

pub fn checkpoint_for(
    conn: &Connection,
    remote_name: &str,
    namespace: &str,
) -> Result<Option<crate::models::SyncCheckpoint>> {
    let mut stmt = conn.prepare(
        "SELECT remote_name, namespace, pull_cursor, last_push_at,
                last_pull_at, last_error, updated_at
         FROM sync_checkpoints
         WHERE remote_name = ?1 AND namespace = ?2",
    )?;

    let mut rows = stmt.query(params![remote_name, namespace])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(crate::models::SyncCheckpoint {
            remote_name: row.get(0)?,
            namespace: row.get(1)?,
            pull_cursor: row.get(2)?,
            last_push_at: row.get(3)?,
            last_pull_at: row.get(4)?,
            last_error: row.get(5)?,
            updated_at: row.get(6)?,
        }));
    }
    Ok(None)
}

fn upsert_checkpoint(
    conn: &Connection,
    remote_name: &str,
    namespace: &str,
    pull_cursor: Option<&str>,
    pushed: bool,
    pulled: bool,
    last_error: Option<&str>,
) -> Result<()> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO sync_checkpoints
            (remote_name, namespace, pull_cursor, last_push_at, last_pull_at, last_error, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(remote_name, namespace) DO UPDATE SET
            pull_cursor = excluded.pull_cursor,
            last_push_at = COALESCE(excluded.last_push_at, sync_checkpoints.last_push_at),
            last_pull_at = COALESCE(excluded.last_pull_at, sync_checkpoints.last_pull_at),
            last_error = excluded.last_error,
            updated_at = excluded.updated_at",
        params![
            remote_name,
            namespace,
            pull_cursor,
            pushed.then_some(now.as_str()),
            pulled.then_some(now.as_str()),
            last_error,
            now,
        ],
    )?;
    Ok(())
}

pub fn push_once(
    conn: &Connection,
    transport: &dyn SyncTransport,
    remote_name: &str,
    namespace: &str,
    limit: u32,
) -> Result<usize> {
    let events = pending_events(conn, namespace, limit)?;
    if events.is_empty() {
        return Ok(0);
    }
    transport.push_events(remote_name, namespace, &events)?;
    let ids = events.iter().map(|event| event.id.clone()).collect::<Vec<_>>();
    mark_events_pushed(conn, &ids)?;
    upsert_checkpoint(conn, remote_name, namespace, None, true, false, None)?;
    Ok(events.len())
}

pub fn pull_once(
    conn: &Connection,
    transport: &dyn SyncTransport,
    remote_name: &str,
    namespace: &str,
    limit: u32,
) -> Result<(usize, usize)> {
    let cursor = checkpoint_for(conn, remote_name, namespace)?
        .and_then(|checkpoint| checkpoint.pull_cursor);
    let batch = transport.pull_events(remote_name, namespace, cursor.as_deref(), limit)?;

    let mut applied = 0;
    let mut skipped = 0;
    for event in &batch.events {
        if apply_remote_event(conn, remote_name, event)? {
            applied += 1;
        } else {
            skipped += 1;
        }
    }

    upsert_checkpoint(
        conn,
        remote_name,
        namespace,
        batch.next_cursor.as_deref(),
        false,
        true,
        None,
    )?;
    Ok((applied, skipped))
}

pub fn run_once(
    conn: &Connection,
    transport: &dyn SyncTransport,
    remote_name: &str,
    namespace: &str,
    limit: u32,
) -> Result<crate::models::SyncReport> {
    let pushed = push_once(conn, transport, remote_name, namespace, limit)?;
    let (pulled, skipped) = pull_once(conn, transport, remote_name, namespace, limit)?;
    Ok(crate::models::SyncReport {
        remote_name: remote_name.to_string(),
        namespace: namespace.to_string(),
        pushed,
        pulled,
        skipped,
    })
}
```

- [ ] **Step 3: Add feature-gated HTTP client**

In `crates/clio-core/Cargo.toml`, add:

```toml
sync-http = ["dep:reqwest", "dep:tokio"]
```

Change the optional `reqwest` dependency so the blocking client is available:

```toml
reqwest = { version = "0.12", features = ["json", "blocking"], optional = true }
```

In `crates/clio-core/src/lib.rs`, add:

```rust
#[cfg(feature = "sync-http")]
pub mod sync_http;
```

Create `crates/clio-core/src/sync_http.rs`:

```rust
use crate::error::{ClioError, Result};
use crate::models::{SyncBatch, SyncEvent};
use crate::sync::SyncTransport;

pub struct HttpTeamHubClient {
    base_url: String,
    token: String,
    client: reqwest::blocking::Client,
}

impl HttpTeamHubClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl SyncTransport for HttpTeamHubClient {
    fn push_events(&self, remote_name: &str, namespace: &str, events: &[SyncEvent]) -> Result<()> {
        let url = format!("{}/v1/events/push", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({
                "remote_name": remote_name,
                "namespace": namespace,
                "events": events
            }))
            .send()
            .map_err(|e| ClioError::Config(format!("sync push failed: {e}")))?;

        if !response.status().is_success() {
            return Err(ClioError::Config(format!(
                "sync push failed with status {}",
                response.status()
            )));
        }
        Ok(())
    }

    fn pull_events(
        &self,
        remote_name: &str,
        namespace: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<SyncBatch> {
        let url = format!("{}/v1/events/pull", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({
                "remote_name": remote_name,
                "namespace": namespace,
                "cursor": cursor,
                "limit": limit
            }))
            .send()
            .map_err(|e| ClioError::Config(format!("sync pull failed: {e}")))?;

        if !response.status().is_success() {
            return Err(ClioError::Config(format!(
                "sync pull failed with status {}",
                response.status()
            )));
        }

        response
            .json::<SyncBatch>()
            .map_err(|e| ClioError::Config(format!("sync pull returned invalid JSON: {e}")))
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p clio-core run_once_pushes_pending_events_and_marks_them_pushed`

Expected: PASS.

Run: `cargo test -p clio-core --features sync-http`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/clio-core/Cargo.toml crates/clio-core/src/lib.rs crates/clio-core/src/sync.rs crates/clio-core/src/sync_http.rs crates/clio-core/tests/integration.rs
git commit -m "feat(core): add team sync transport"
```

---

## Task 5: Add CLI Sync Commands

**Files:**
- Modify: `crates/clio-cli/Cargo.toml`
- Modify: `crates/clio-cli/src/main.rs`
- Test: compile via `cargo test -p clio-cli`

**Interfaces:**
- Produces user commands:
  - `clio sync remote add --name team --base-url https://clio.example.com --token-env CLIO_TEAM_TOKEN`
  - `clio sync namespace add --remote team --namespace project:clio --mode push-pull`
  - `clio sync status`
  - `clio sync push --remote team --namespace project:clio`
  - `clio sync pull --remote team --namespace project:clio`
  - `clio sync run --remote team --namespace project:clio`

- [ ] **Step 1: Enable sync HTTP in CLI**

In `crates/clio-cli/Cargo.toml`, change the `clio-core` dependency to include `sync-http`:

```toml
clio-core = { workspace = true, features = ["local-embeddings", "openai-embeddings", "capture", "sync-http"] }
```

- [ ] **Step 2: Add CLI enums**

In `crates/clio-cli/src/main.rs`, add a `Sync` variant to `Command`:

```rust
/// Configure and run Team Hub sync.
Sync {
    #[command(subcommand)]
    command: SyncCommand,
},
```

Add:

```rust
#[derive(Subcommand)]
enum SyncCommand {
    /// Manage remote hubs.
    Remote {
        #[command(subcommand)]
        command: SyncRemoteCommand,
    },
    /// Manage namespace subscriptions.
    Namespace {
        #[command(subcommand)]
        command: SyncNamespaceCommand,
    },
    /// Show configured remotes, subscriptions, and checkpoints.
    Status,
    /// Push pending local events.
    Push(SyncRunArgs),
    /// Pull remote events.
    Pull(SyncRunArgs),
    /// Push and then pull remote events.
    Run(SyncRunArgs),
}

#[derive(Subcommand)]
enum SyncRemoteCommand {
    /// Add or update a remote hub.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        token_env: String,
    },
}

#[derive(Subcommand)]
enum SyncNamespaceCommand {
    /// Subscribe a namespace to a remote hub.
    Add {
        #[arg(long)]
        remote: String,
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value = "push-pull")]
        mode: String,
    },
}

#[derive(Args)]
struct SyncRunArgs {
    #[arg(long)]
    remote: String,
    #[arg(long)]
    namespace: String,
    #[arg(long)]
    limit: Option<u32>,
}
```

In the main `match`, add:

```rust
Command::Sync { command } => cmd_sync(cli.db_path.as_deref(), cli.json, command),
```

- [ ] **Step 3: Add core remote/subscription helpers**

In `crates/clio-core/src/sync.rs`, add helpers used by CLI:

```rust
pub fn upsert_remote(conn: &Connection, name: &str, base_url: &str, token_env: &str) -> Result<()> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO sync_remotes (name, base_url, token_env, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, 1, ?4, ?4)
         ON CONFLICT(name) DO UPDATE SET
            base_url = excluded.base_url,
            token_env = excluded.token_env,
            enabled = 1,
            updated_at = excluded.updated_at",
        params![name, base_url, token_env, now],
    )?;
    Ok(())
}

pub fn upsert_namespace_subscription(
    conn: &Connection,
    remote_name: &str,
    namespace: &str,
    mode: crate::models::SyncMode,
) -> Result<()> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO sync_namespace_subscriptions
            (remote_name, namespace, mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(remote_name, namespace) DO UPDATE SET
            mode = excluded.mode,
            updated_at = excluded.updated_at",
        params![remote_name, namespace, mode.as_str(), now],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Implement `cmd_sync`**

Add this command function in `crates/clio-cli/src/main.rs`:

```rust
fn cmd_sync(
    db_path: Option<&str>,
    json: bool,
    command: SyncCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;

    match command {
        SyncCommand::Remote { command: SyncRemoteCommand::Add { name, base_url, token_env } } => {
            clio_core::sync::upsert_remote(&conn, &name, &base_url, &token_env)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "remote": name,
                    "base_url": base_url,
                    "token_env": token_env
                }))?);
            } else {
                println!("Configured sync remote {name}");
            }
        }
        SyncCommand::Namespace { command: SyncNamespaceCommand::Add { remote, namespace, mode } } => {
            let mode = match mode.as_str() {
                "push-pull" | "push_pull" => clio_core::models::SyncMode::PushPull,
                "push-only" | "push_only" => clio_core::models::SyncMode::PushOnly,
                "pull-only" | "pull_only" => clio_core::models::SyncMode::PullOnly,
                other => return Err(format!("invalid sync mode: {other}").into()),
            };
            clio_core::sync::upsert_namespace_subscription(&conn, &remote, &namespace, mode)?;
            println!("Subscribed {namespace} to remote {remote}");
        }
        SyncCommand::Status => {
            let status = clio_core::sync::status(&conn)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        SyncCommand::Push(args) => {
            let client = sync_client_for(&conn, &args.remote)?;
            let limit = args.limit.unwrap_or(100);
            let pushed = clio_core::sync::push_once(&conn, &client, &args.remote, &args.namespace, limit)?;
            println!("Pushed {pushed} event(s)");
        }
        SyncCommand::Pull(args) => {
            let client = sync_client_for(&conn, &args.remote)?;
            let limit = args.limit.unwrap_or(100);
            let (pulled, skipped) = clio_core::sync::pull_once(&conn, &client, &args.remote, &args.namespace, limit)?;
            println!("Pulled {pulled} event(s), skipped {skipped}");
        }
        SyncCommand::Run(args) => {
            let client = sync_client_for(&conn, &args.remote)?;
            let limit = args.limit.unwrap_or(100);
            let report = clio_core::sync::run_once(&conn, &client, &args.remote, &args.namespace, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Sync complete for {}: pushed {}, pulled {}, skipped {}",
                    report.namespace, report.pushed, report.pulled, report.skipped
                );
            }
        }
    }

    Ok(())
}
```

Add `sync_client_for` after the command:

```rust
fn sync_client_for(
    conn: &rusqlite::Connection,
    remote_name: &str,
) -> Result<clio_core::sync_http::HttpTeamHubClient, Box<dyn std::error::Error>> {
    let remote = clio_core::sync::get_remote(conn, remote_name)?
        .ok_or_else(|| format!("unknown sync remote: {remote_name}"))?;
    let token = std::env::var(&remote.token_env)
        .map_err(|_| format!("sync token environment variable {} is not set", remote.token_env))?;
    Ok(clio_core::sync_http::HttpTeamHubClient::new(remote.base_url, token))
}
```

Add `get_remote` and `status` in `sync.rs` as simple SELECT helpers over `sync_remotes`, `sync_namespace_subscriptions`, and `sync_checkpoints`.

- [ ] **Step 5: Run CLI build/test**

Run: `cargo test -p clio-cli`

Expected: PASS.

Run: `cargo run -p clio-cli -- sync remote add --name team --base-url http://127.0.0.1:8787 --token-env CLIO_TEAM_TOKEN`

Expected: prints `Configured sync remote team`.

- [ ] **Step 6: Commit**

```bash
git add crates/clio-cli/Cargo.toml crates/clio-cli/src/main.rs crates/clio-core/src/sync.rs
git commit -m "feat(cli): add team sync commands"
```

---

## Task 6: Add Minimal Team Hub HTTP Service

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/clio-hub/Cargo.toml`
- Create: `crates/clio-hub/src/main.rs`
- Test: `cargo test -p clio-hub`

**Interfaces:**
- Produces binary: `clio-hub`.
- Produces endpoints:
  - `GET /v1/health`
  - `POST /v1/events/push`
  - `POST /v1/events/pull`
- Consumed by: Task 7 end-to-end verification.

- [ ] **Step 1: Add workspace crate and dependencies**

In root `Cargo.toml`, add `"crates/clio-hub"` to `members` and add shared dependencies:

```toml
axum = "0.8"
tower-http = { version = "0.6", features = ["trace"] }
```

Create `crates/clio-hub/Cargo.toml`:

```toml
[package]
name = "clio-hub"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "HTTP Team Hub for Clio shared memory sync"

[[bin]]
name = "clio-hub"
path = "src/main.rs"

[dependencies]
axum = { workspace = true }
clio-core = { workspace = true }
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "net"] }
tower-http = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 2: Write hub service**

Create `crates/clio-hub/src/main.rs`:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use clio_core::models::{SyncBatch, SyncEvent};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<rusqlite::Connection>>,
    token: String,
}

#[derive(Debug, Deserialize)]
struct PushRequest {
    remote_name: String,
    namespace: String,
    events: Vec<SyncEvent>,
}

#[derive(Debug, Deserialize)]
struct PullRequest {
    remote_name: String,
    namespace: String,
    cursor: Option<String>,
    limit: u32,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();

    let db_path = std::env::var("CLIO_HUB_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("clio-hub.db"));
    let token = std::env::var("CLIO_HUB_TOKEN")
        .map_err(|_| "CLIO_HUB_TOKEN must be set")?;
    let bind = std::env::var("CLIO_HUB_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse::<SocketAddr>()?;

    let conn = clio_core::db::open(&db_path)?;
    let state = AppState {
        conn: Arc::new(Mutex::new(conn)),
        token,
    };

    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/events/push", post(push_events))
        .route("/v1/events/pull", post(pull_events))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "Clio Team Hub listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

fn authorised(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|provided| provided == token)
        .unwrap_or(false)
}

async fn push_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PushRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let conn = state.conn.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for event in &req.events {
        clio_core::sync::apply_remote_event(&conn, &req.remote_name, event)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        clio_core::sync::record_memory_event(
            &conn,
            Some("hub"),
            event.event_type.clone(),
            event.memory_id.as_deref(),
            &req.namespace,
            event.payload.clone(),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(serde_json::json!({ "accepted": req.events.len() })))
}

async fn pull_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PullRequest>,
) -> Result<Json<SyncBatch>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let conn = state.conn.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let events = clio_core::sync::events_after_cursor(
        &conn,
        &req.namespace,
        req.cursor.as_deref(),
        req.limit,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let next_cursor = events.last().map(|event| event.id.clone());

    Ok(Json(SyncBatch {
        remote_name: req.remote_name,
        namespace: req.namespace,
        events,
        next_cursor,
    }))
}
```

Add `events_after_cursor` to `sync.rs`; it should return namespace events ordered by `(created_at, id)` and filter `id > cursor` when a cursor is supplied.

- [ ] **Step 3: Run hub tests/build**

Run: `cargo test -p clio-hub`

Expected: PASS.

Run: `CLIO_HUB_TOKEN=test-token cargo run -p clio-hub`

Expected: process starts and logs that the hub is listening on `127.0.0.1:8787`.

Stop the process with `Ctrl-C`.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/clio-hub crates/clio-core/src/sync.rs
git commit -m "feat(hub): add minimal team sync API"
```

---

## Task 7: Add Documentation And End-To-End Smoke Test

**Files:**
- Create: `docs/reference/team-hub-api.md`
- Create: `docs/team-hub.md`
- Modify: `docs/reference/schema.md`
- Modify: `docs/reference/settings.md`
- Modify: `docs/cli-reference.md`

**Interfaces:**
- Produces operator workflow for running a hub and syncing two local databases.
- Produces source-of-truth API contract for future clients.

- [ ] **Step 1: Document schema changes**

In `docs/reference/schema.md`, add a “Team sync” section containing the exact purpose of:

```text
memory_events — append-only local event log for explicit memory writes.
sync_remotes — configured remote hubs; token values are not stored.
sync_namespace_subscriptions — explicit namespace sync modes per remote.
sync_checkpoints — per-remote/per-namespace pull cursor and health state.
sync_applied_events — idempotency guard for remote events already applied locally.
```

Add `007_team_sync` to the applied migrations table.

- [ ] **Step 2: Document settings**

In `docs/reference/settings.md`, add:

```markdown
## sync

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `node_id` | string? | `null` | Stable workstation id for sync provenance. When unset, local sync uses `local` until configured. |
| `batch_size` | int | `100` | Maximum events per push or pull request. |

Remote hub URLs and token environment variable names live in SQLite tables configured by `clio sync remote add`. Token values are read from the named environment variable and are never written to `clio-settings.json`.
```

- [ ] **Step 3: Add API reference**

Create `docs/reference/team-hub-api.md`:

```markdown
# Team Hub API Reference

Team Hub is an opt-in HTTP API for synchronising explicitly subscribed Clio namespaces across workstations.

## Authentication

Every endpoint except `GET /v1/health` requires:

```text
Authorization: Bearer <token>
```

The hub reads the expected token from `CLIO_HUB_TOKEN`.

## GET /v1/health

Returns:

```json
{ "ok": true }
```

## POST /v1/events/push

Request:

```json
{
  "remote_name": "team",
  "namespace": "project:clio",
  "events": []
}
```

Response:

```json
{ "accepted": 1 }
```

## POST /v1/events/pull

Request:

```json
{
  "remote_name": "team",
  "namespace": "project:clio",
  "cursor": null,
  "limit": 100
}
```

Response:

```json
{
  "remote_name": "team",
  "namespace": "project:clio",
  "events": [],
  "next_cursor": null
}
```
```

- [ ] **Step 4: Add operator guide**

Create `docs/team-hub.md` with this workflow:

```markdown
# Clio Team Hub

Clio Team Hub lets multiple workstations share selected namespaces through an authenticated HTTP service while each workstation keeps its local SQLite database.

## Start a local hub

```bash
export CLIO_HUB_TOKEN='replace-with-a-long-random-token'
export CLIO_HUB_DB_PATH="$HOME/Library/Application Support/clio/team-hub.db"
cargo run -p clio-hub
```

## Configure a workstation

```bash
export CLIO_TEAM_TOKEN='replace-with-a-long-random-token'
clio sync remote add \
  --name team \
  --base-url http://127.0.0.1:8787 \
  --token-env CLIO_TEAM_TOKEN

clio sync namespace add \
  --remote team \
  --namespace project:clio \
  --mode push-pull
```

## Run sync manually

```bash
clio sync run --remote team --namespace project:clio
```

Sync is explicit. Memories in namespaces that have not been subscribed are not pushed or pulled.
```

- [ ] **Step 5: Run end-to-end smoke manually**

Terminal A:

```bash
tmpdir="$(mktemp -d)"
export CLIO_HUB_TOKEN=test-token
export CLIO_HUB_DB_PATH="$tmpdir/hub.db"
cargo run -p clio-hub
```

Terminal B:

```bash
tmpdir="$(mktemp -d)"
export CLIO_TEAM_TOKEN=test-token
export CLIO_DB_PATH="$tmpdir/a.db"
cargo run -p clio-cli -- sync remote add --name team --base-url http://127.0.0.1:8787 --token-env CLIO_TEAM_TOKEN
cargo run -p clio-cli -- sync namespace add --remote team --namespace project:clio --mode push-pull
cargo run -p clio-cli -- remember "Shared from workstation A" --namespace project:clio --kind fact
cargo run -p clio-cli -- sync run --remote team --namespace project:clio
```

Terminal C:

```bash
tmpdir_b="$(mktemp -d)"
export CLIO_TEAM_TOKEN=test-token
export CLIO_DB_PATH="$tmpdir_b/b.db"
cargo run -p clio-cli -- sync remote add --name team --base-url http://127.0.0.1:8787 --token-env CLIO_TEAM_TOKEN
cargo run -p clio-cli -- sync namespace add --remote team --namespace project:clio --mode push-pull
cargo run -p clio-cli -- sync pull --remote team --namespace project:clio
cargo run -p clio-cli -- recall "Shared from workstation A" --namespace project:clio
```

Expected final output includes `Shared from workstation A`.

- [ ] **Step 6: Run final verification**

Run:

```bash
cargo test -p clio-core --features sync-http
cargo test -p clio-cli
cargo test -p clio-hub
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add docs/reference/schema.md docs/reference/settings.md docs/reference/team-hub-api.md docs/team-hub.md docs/cli-reference.md
git commit -m "docs: document clio team hub sync"
```

---

## Self-Review

- **Spec coverage:** The agreed direction was local-first Team Hub sync with explicit namespaces and an authenticated remote API. Tasks 1-4 build local event/checkpoint/client foundations, Task 5 exposes CLI controls, Task 6 creates the minimal hub API, and Task 7 documents operation and proves workstation-to-workstation sync.
- **Scope control:** Background daemon sync, admin UI, remote semantic recall fallback, and connectors are intentionally excluded so this first slice can ship as testable infrastructure.
- **Invariant coverage:** Archive remains soft, source/source_ref conflicts are handled before remote inserts, sync is opt-in by namespace, tokens are read from env vars, and all persistence logic lives in `clio-core`.
- **Type consistency:** `SyncEvent`, `SyncBatch`, `SyncReport`, and `SyncTransport` are introduced before use by CLI and hub tasks.
- **Verification:** Each task has package-level tests and commit instructions; Task 7 includes a two-workstation smoke test.
