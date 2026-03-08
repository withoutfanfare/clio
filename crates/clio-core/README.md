# clio-core

All Clio business logic lives here. Every other crate is a thin consumer of this library.

## Modules

- `config.rs` — path resolution, DB location
- `db.rs` — connection setup, pragmas, migrations
- `models.rs` — Memory, Tag, MemoryLink, RecallQuery, RecallResult
- `repository.rs` — CRUD, upsert, archive, link, recall, temporal scoring
- `embeddings.rs` — pluggable backends (local fastembed, OpenAI), cosine similarity, semantic recall
- `settings.rs` — load/save clio-settings.json
- `capture.rs` — LLM-based capture pipeline (feature-gated)
- `context.rs` — automatic namespace detection from working directory
- `stats.rs` — analytics queries and activity feed
- `daemon.rs` — daemon configuration, lifecycle, and health types
- `review.rs` — review queue for low-confidence captures
- `assembly.rs` — context assembly for agent consumption
- `validate.rs` — input validation

## Boundary Rules

This crate must NOT depend on Tauri UI code, MCP-specific types, or CLI formatting. All persistence logic, validation, and search ranking live here exclusively.
