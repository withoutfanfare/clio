# archive — reference only

Local contract for archived material. Inherits repo-wide rules from the root
CLAUDE.md. This is a frozen boundary.

## Purpose

Historical reference kept for context, not for use:

- `python-prototype/` — the original Python implementation, superseded by the
  Rust workspace.
- `clio-rationale-transcript.md` — design rationale transcript.

## Rule

**Do not extend, build, or import anything here.** `python-prototype/` is
reference only. New work belongs in `crates/` (logic) or the relevant adapter.
Read these files to understand prior decisions; do not modify them as part of
feature work.
