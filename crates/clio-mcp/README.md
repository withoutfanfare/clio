# clio-mcp

Thin MCP (Model Context Protocol) adapter over `clio-core`. Runs on stdio and exposes 21 tools for reading, writing, searching, and linking memories.

See [MCP Contract](../../docs/reference/mcp-contract.md) for the full tool and resource definitions.

## Tools

`memory_remember`, `memory_recall`, `memory_get`, `memory_recent`, `memory_search`, `memory_capture`, `memory_link`, `memory_get_links`, `memory_suggest_links`, `memory_archive`, `memory_unarchive`, `memory_namespaces`, `memory_stats`, `memory_activity`, `memory_delete`, `memory_context`, `memory_inbox_list`, `memory_inbox_approve`, `memory_inbox_reject`, `memory_inbox_edit`

Must NOT duplicate persistence logic or invent alternate search semantics.
