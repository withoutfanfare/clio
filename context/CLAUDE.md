# context — deep reference

Local contract for the deep-reference docs. Inherits repo-wide rules from the
root CLAUDE.md. These are the canonical internal references to read **before**
significant implementation work; both the root CLAUDE.md and `AGENTS.md` point
here.

## Purpose

- `ARCHITECTURE.md` — crate boundaries, system diagram, module listings, stack.
- `CRITICAL_WARNINGS.md` — invariants that produce bugs if violated.
- `DOMAIN_RULES.md` — entities, namespaces, workflows, search semantics.

## Work Guidance

- British English throughout.
- These describe how the system actually behaves — when an invariant or boundary
  in `clio-core` changes, update the matching file here so it never drifts from
  the code it documents.
- `CRITICAL_WARNINGS.md` is the long-form companion to the invariants summarised
  in `crates/clio-core/CLAUDE.md`; keep the two consistent.

## Verification

No automated check. After changing core behaviour or invariants, confirm the
relevant reference file still matches.
