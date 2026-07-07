# Follow-up: Clio for Kimi, OpenCode and Gemini

**Status:** deferred — not started. Raised by Danny on 2026-07-07 alongside the
handoff-briefs-and-receipts plan (`2026-07-07-handoff-briefs-and-receipts.md`).
This document exists so the request is not forgotten.

## Goal

Every AI tool on this machine should both **read** shared memory (MCP tools /
session-start brief) and **write** it back (session capture → receipts and
distilled knowledge). Claude Code has both today; Codex gains capture in the
handoff-briefs plan. Remaining tools:

| Tool | Read (MCP) | Session-start brief | Capture (stop hook) |
|------|-----------|---------------------|---------------------|
| Kimi | unknown — check MCP support and config file location | none | none |
| OpenCode | likely easy — `~/.config/opencode/opencode.json` supports MCP servers | none | none — transcripts live in `~/.local/share/opencode/opencode.db` (SQLite, schema unverified); it also has a JS plugin system (`~/.config/opencode/plugins/`, events like `session.idle`) that may be the better capture path |
| Gemini (CLI) | likely easy — Gemini CLI supports MCP servers in `~/.gemini/settings.json` | none | check whether Gemini CLI has lifecycle hooks; if not, capture may need a wrapper or be read-only for now |

## Work items (per tool)

1. **MCP wiring** — register the `clio-mcp` binary as an MCP server in the
   tool's config so `memory_*` tools are available. Verify namespace detection
   works when the tool passes (or omits) `cwd`.
2. **Session-start brief** — if the tool supports startup hooks/context
   injection, port `session_start.py`'s brief (project-brief preset + branch
   recall). If not, rely on the MCP instructions nudging the model to call
   `memory_context` early.
3. **Capture** — port the stop-hook pattern. The pipeline is deliberately
   shared: build a digest of the session, pipe it to
   `clio distill - --source <tool>-session --source-ref <session-id>`.
   `distill_to_clio` in `session_stop.py` already takes a `source` parameter
   (added in the handoff-briefs plan, Task 4). Each tool only needs a
   digest-builder for its own transcript format plus a duplicate-fire guard
   (reuse `claim_session`).

## Notes / constraints

- Sources should follow the existing convention: `kimi-session`,
  `opencode-session`, `gemini-session`.
- Do not port anything until the Codex hook (handoff-briefs plan, Task 4) has
  run for a few days — it proves the shared pipeline and will surface digest
  quality issues cheaply.
- OpenCode discovery task: inspect `opencode.db` schema (`sqlite3
  ~/.local/share/opencode/opencode.db .schema | head`) and the plugin event
  payloads before choosing DB-read vs plugin capture.
- The README's cross-tool claims must be kept honest: only list tools whose
  capture actually works.

## Definition of done

Each tool can (a) recall project memories via MCP in a real session, and
(b) a session in that tool leaves at least a receipt in Clio, visible in
`clio brief --preset handoff` output for the relevant ticket/namespace.
