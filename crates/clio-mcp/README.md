# clio-mcp

Thin MCP (Model Context Protocol) adapter over `clio-core`. Runs on stdio and
exposes 23 tools for reading, writing, searching, capture, attention, review,
and cache control.

See [MCP Contract](../../docs/reference/mcp-contract.md) for the full tool and resource definitions.

## Tools

`memory_remember`, `memory_update`, `memory_recall`, `memory_get`,
`memory_recent` (deprecated), `memory_link`, `memory_archive`,
`memory_unarchive`, `memory_delete`, `memory_move`, `memory_namespaces`,
`memory_get_links`, `memory_capture`, `memory_session_checkpoint`,
`memory_search`, `memory_stats`, `memory_activity`, `memory_suggest_links`,
`memory_context`, `memory_inbox`, `memory_resume`, `memory_action`,
`memory_cache_clear`

The process opens one SQLite connection and creates its embedding backend once.
It reloads non-embedding settings at most every 30 seconds; embedding changes
require a process restart. Provider calls for semantic search queries, capture
classification, checkpoint distillation and automatic embeddings following
remember, update, capture and checkpoint writes run without holding the shared
SQLite mutex. A generated record embedding is stored only if that record has
not changed while the provider call was in flight.

Must NOT duplicate persistence logic or invent alternate search semantics.
