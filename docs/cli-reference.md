# CLI Reference

Quick reference for all Clio CLI commands and flags.

For a full walkthrough with explanations, see [Getting Started](getting-started.md). For connecting AI agents, see [MCP Agent Setup](mcp-agent-setup.md).

---

## Setup

```sh
# Install both binaries
cargo install --path crates/clio-cli
cargo install --path crates/clio-mcp

# Initialise the database
clio init

# Initialise with a project namespace
clio init --namespace project:my-project
```

---

## Storing Memories

```sh
# Basic
clio remember --content "Your content here"

# Full options
clio remember \
  --content "We use WAL mode for SQLite" \
  --title "SQLite WAL decision" \
  --kind decision \
  --tags sqlite,architecture \
  --importance 4

# From stdin
echo "Redis for caching" | clio remember --content -

# Upsert (idempotent write)
clio remember \
  --content "Updated content" \
  --source notes \
  --source-ref unique-key-001 \
  --upsert
```

**Kinds:** `note` `fact` `decision` `snippet` `preference` `process` `knowledgebase` `observation`

**Importance:** 1 (low) to 5 (critical), default 3

---

## Retrieving Memories

```sh
# Full-text search (keyword matching)
clio recall --query "redis caching"

# Semantic search (meaning-based)
clio search --query "database performance optimisation"

# Recent memories (supports all filters)
clio recent
clio recent --kind decision --importance-min 4
clio recent --sort importance-desc --limit 20
clio recent --tags rust,sqlite --match-any

# Show one memory
clio show <id>

# Filters (work with recall, recent, and search)
--namespace project:my-project
--kind decision
--tags sqlite,architecture
--match-any              # match ANY tag instead of ALL
--importance-min 3       # minimum importance (1–5)
--importance-max 5       # maximum importance (1–5)
--sort importance-desc   # sort order (see below)
--include-archived
--limit 20
--offset 40

# JSON output
clio recall --query "redis" --json
```

**Sort orders:** `updated-desc` (default) `updated-asc` `importance-desc` `importance-asc` `created-desc` `created-asc`

---

## Knowledge Graph

```sh
# Link two memories
clio link <from-id> <to-id> --relationship supports

# Get links from a memory
clio show <id>    # links shown in detail view

# Find similar unlinked memories
clio suggest-links --memory-id <id>
clio suggest-links --memory-id <id> --threshold 0.8 --limit 10
```

**Relationships:** `relates_to` `supports` `contradicts` `supersedes` `derived_from` or any custom string

---

## Capture Pipeline

Sends unstructured text to an LLM for automatic classification.

```sh
# Enable capture (one-time setup)
clio settings use-capture --api-key sk-...

# Preview classification without storing
clio capture --text "We decided to use Redis for caching" --dry-run

# Capture and store
clio capture --text "We decided to use Redis for caching"
```

### Distil (transcript → durable memories)

`distill` sends a long body of text — typically a whole session transcript — to
the LLM and extracts **zero or more** self-contained, durable memories
(decisions, facts, constraints, insights). Routine input yields nothing, so
noise is filtered by design. Uses the same capture pipeline (review routing,
auto-embed) per extracted memory.

```sh
# Preview the durable memories without storing
clio distill - --dry-run < session-digest.txt

# Distil and store, tagging provenance
clio distill - --source claude-code-session --source-ref <session-id> < session-digest.txt
```

Pass `-` to read the text from stdin. `--namespace` overrides the namespace for
every extracted memory. Requires the capture pipeline to be enabled.

---

## Archiving & Deletion

```sh
clio archive <id>       # soft-archive (hidden, restorable)
clio unarchive <id>     # restore
clio delete <id>        # permanent delete of a single memory
```

---

## Namespace Management

```sh
# Check detected namespace
clio context

# List all namespaces
clio namespaces
```

Detection order: `.clio-namespace` file > `.git` dir > `Cargo.toml`/`package.json` > `global`

### Cleanup (stale namespaces)

`cleanup` finds namespaces that are no longer useful and can purge them. It is
**dry-run by default** — pass `--execute` to actually delete, and a database
backup is always taken first.

```sh
# Dry run — show stale candidates and why they were flagged (all criteria)
clio cleanup

# Restrict to specific criteria
clio cleanup --stale-months 6      # no activity for 6 months
clio cleanup --archived            # every memory already archived
clio cleanup --folder-gone         # project:<slug> with no folder on disk

# Actually purge (backup taken first)
clio cleanup --folder-gone --execute
```

Criteria:
- **stale-months** — last activity older than N months (default from settings).
- **archived** — the namespace has no live memories (all archived).
- **folder-gone** — a `project:<slug>` namespace whose folder is not found under
  any configured dev root (a heuristic — see `cleanup.dev_roots` in settings).

With no criterion flag, all three are applied. The `global` namespace is never
flagged. See `reference/settings.md` for `cleanup.*` configuration.

### Consolidate (project memory)

`consolidate` rolls a namespace's atomic memories into a single AI-curated
"consolidated memory" document — a coherent, deduplicated summary of the
project. It is stored as a singleton memory per namespace (`kind = summary`,
upserted in place), and the session-start brief leads with it.

```sh
# Consolidate the current project (namespace auto-detected from cwd)
clio consolidate

# Consolidate a specific namespace
clio consolidate --namespace project:clio
```

The document is a **derived cache**: each run reconciles it from the current
atomic memories (no iterative self-editing, so it can't drift), and the atomic
memories are left untouched. Requires the capture pipeline to be enabled.

Triggers:

```sh
# Every namespace
clio consolidate --all

# Only namespaces with enough new memories since last run (the configured
# consolidate.auto_threshold) — cheap to run often, no-op when nothing's due
clio consolidate --if-due
clio consolidate --all --if-due
```

The Stop hook (see `clio-hooks`) runs `clio consolidate --if-due` after each
session that produced memories, so the consolidated document refreshes
automatically once a project accrues enough new material.

**Scheduling (macOS launchd):** to also refresh on a timer, drop a LaunchAgent
at `~/Library/LaunchAgents/com.clio.consolidate.plist` and
`launchctl load` it:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>com.clio.consolidate</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/YOU/.cargo/bin/clio</string>
        <string>consolidate</string>
        <string>--all</string>
        <string>--if-due</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict><key>Hour</key><integer>6</integer><key>Minute</key><integer>0</integer></dict>
</dict>
</plist>
```

---

## Stats & Activity

```sh
clio stats
clio stats --namespace project:clio
clio activity
clio activity --namespace project:clio --limit 20
```

---

## Import & Export

```sh
# Export
clio export --output memories.jsonl
clio export --output memories.jsonl --namespace project:clio

# Import
clio import --input memories.jsonl

# Migrate from AI assistants
clio migrate --source claude --file conversations.json
clio migrate --source chatgpt --file export.json
clio migrate --source claude --file conversations.json --classify --dry-run
```

---

## Embeddings

```sh
# Backfill all unembedded memories
clio embed --all

# Embed a specific memory
clio embed --id <id>
```

---

## Settings

```sh
# View current settings
clio settings show

# Embedding providers
clio settings use-local-embeddings                    # default, no API key needed
clio settings use-openai-embeddings --api-key sk-...  # higher quality, needs key

# Auto-embed toggle
clio settings auto-embed --enable
clio settings auto-embed --disable

# Capture pipeline
clio settings use-capture --api-key sk-... --model gpt-4o-mini
```

---

## Global Flags

| Flag | Description |
|---|---|
| `--db-path <path>` | Override database location |
| `--json` | JSON output |

Default DB: `~/Library/Application Support/clio/memory.db` (macOS)

---

## Related Documentation

- [Getting Started](getting-started.md) — full walkthrough with explanations
- [MCP Agent Setup](mcp-agent-setup.md) — connecting AI agents to Clio
- [Settings Reference](reference/settings.md) — all configuration keys and defaults
- [Schema Reference](reference/schema.md) — database structure
- [Documentation Index](README.md) — all available documentation
