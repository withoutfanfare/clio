# Clio Documentation

Welcome to the Clio documentation. This index organises all available documentation by audience and task.

---

## Getting Started

New to Clio? Start here:

1. **[Getting Started Guide](getting-started.md)** — Install Clio, initialise the database, store and recall your first memories. Takes about ten minutes.

2. **[MCP Agent Setup](mcp-agent-setup.md)** — Connect your AI coding assistants (Claude Code, Codex, Cursor, Windsurf, Gemini, Copilot, OpenCode, Kilo, Kimi) to Clio.

3. **[CLI Reference](cli-reference.md)** — Quick reference for all CLI commands and flags.

---

## User Documentation

### Core Guides

| Document | Description |
|----------|-------------|
| [CLI Reference](cli-reference.md) | All commands, flags, and examples |
| [MCP Agent Setup](mcp-agent-setup.md) | Connecting AI agents to Clio with one-command setup |
| [Desktop App (Tauri)](tauri-app.md) | Building, using, and extending the desktop app |
| [Resource Limits](resource-limits.md) | Sizing constraints and thresholds |

### Reference Documentation

| Document | Description |
|----------|-------------|
| [Settings Reference](reference/settings.md) | All configuration keys and defaults |
| [Schema Reference](reference/schema.md) | SQLite tables, indexes, FTS, triggers |
| [MCP Contract](reference/mcp-contract.md) | Full MCP tool and resource definitions |

---

## AI Agent Documentation

For AI agents using Clio via MCP:

| Document | Description |
|----------|-------------|
| [MCP Agent Setup](mcp-agent-setup.md) | Connection configs, workflows, and prompting patterns |
| [MCP Contract](reference/mcp-contract.md) | Full tool and resource definitions, input/output schemas |

---

## Contributor Documentation

### Technical Reference

| Document | Description |
|----------|-------------|
| [Architecture](../context/ARCHITECTURE.md) | System diagram, crate boundaries, tech stack |
| [Schema Reference](reference/schema.md) | SQLite tables, indexes, FTS, triggers |
| [MCP Contract](reference/mcp-contract.md) | MCP tool and resource definitions |
| [Settings Reference](reference/settings.md) | All configuration keys and defaults |
| [Desktop App (Tauri)](tauri-app.md) | Tauri commands, frontend architecture, design system |
| [Security Review](security-review.md) | Codebase security audit findings |

### Project Context

| Document | Description |
|----------|-------------|
| [Rationale](rationale.md) | Why Clio exists and architectural choices |
| [Implementation Plan](plan/implementation-plan.md) | Full delivery plan |
| [Critical Warnings](../context/CRITICAL_WARNINGS.md) | Important invariants and gotchas |
| [Domain Rules](../context/DOMAIN_RULES.md) | Business rules and constraints |
| [Changelog](../CHANGELOG.md) | Release history |

---

## By Task

### "I want to..."

| Task | Document |
|------|----------|
| Install and set up Clio | [Getting Started](getting-started.md) |
| Connect an AI agent | [MCP Agent Setup](mcp-agent-setup.md) |
| Learn CLI commands | [CLI Reference](cli-reference.md) |
| Configure Clio | [Settings Reference](reference/settings.md) |
| Understand the database schema | [Schema Reference](reference/schema.md) |
| Build the desktop app | [Desktop App (Tauri)](tauri-app.md) |
| Understand the architecture | [Architecture](../context/ARCHITECTURE.md) |
| Contribute to the codebase | [Architecture](../context/ARCHITECTURE.md) + [Schema Reference](reference/schema.md) |

---

## Documentation Structure

```text
docs/
├── README.md              ← You are here
├── getting-started.md     ← Start here if new
├── cli-reference.md       ← CLI quick reference
├── mcp-agent-setup.md     ← AI agent connections
├── tauri-app.md           ← Desktop app guide
├── resource-limits.md     ← Sizing constraints
├── rationale.md           ← Why Clio exists
├── security-review.md     ← Security audit
├── performance-audit.md   ← Performance analysis
├── reference/
│   ├── schema.md          ← Database schema
│   ├── mcp-contract.md    ← MCP tool definitions
│   └── settings.md        ← Configuration keys
├── guides/
│   └── moving-project.md  ← Project migration guide
└── plan/
    └── implementation-plan.md  ← Delivery plan
```
