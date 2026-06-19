# docs — user & contributor documentation

Local contract for the documentation tree. Inherits repo-wide rules from the root
CLAUDE.md. This doc owns documentation standards and the contract source-of-truth
rule. For the deep internal invariant reference, see `context/` (its own doc).

## Purpose

Audience-facing documentation: getting started, CLI reference, MCP agent setup,
desktop app guide, plus the machine-relevant **contracts** and planning records.

## Contracts (source of truth)

These define behaviour the code must match — keep them in lock-step with the
implementation in `clio-core`:

- `reference/schema.md` — SQLite schema, indexes, FTS, triggers.
- `reference/mcp-contract.md` — MCP tool/resource definitions.
- `reference/settings.md` — config keys and defaults.

## Plans & specs

- `superpowers/plans/` and `superpowers/specs/` hold dated design and
  implementation records. Name new files `YYYY-MM-DD-<slug>.md`; never rewrite a
  past record — add a new one.

## Work Guidance

- British English throughout.
- Update the relevant doc when behaviour, commands, or contracts change — a
  contract change in `clio-core` and its `reference/` doc land together.
- Keep `README.md` (the docs index) current when adding or moving documents.

## Verification

No automated docs check exists yet. When code changes a documented contract,
confirm the matching `reference/` file was updated in the same change.
