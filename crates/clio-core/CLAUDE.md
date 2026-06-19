# clio-core — business logic home

Local contract for the core library. Inherits repo-wide rules from the root
CLAUDE.md and the workspace rules from `crates/CLAUDE.md`. Every business rule in
Clio lives here; the adapters are thin and must not reimplement any of it.

## Purpose

The single home for all logic: storage (SQLite, WAL, FTS5), recall and scoring,
embeddings, migrations, capture, deduplication, auto-linking, backup/export.
Module map: `repository.rs`, `db.rs`, `migrations.rs`/`migrate.rs`, `models.rs`,
`embeddings.rs`, `assembly.rs`, `context.rs`, `deduplication.rs`, `integrity.rs`,
`settings.rs`, `config.rs`, `capture.rs`, `review.rs`, `backup.rs`, `export.rs`.

## Critical Invariants (bugs if violated)

- **Archive means hidden, not deleted.** Archived records stay excluded from
  default recall.
- **Tags and FTS must stay in sync** — triggers handle this; preserve them.
- **Upsert keyed on `source + source_ref`** must not create duplicates.
- **Access tracking is fire-and-forget** — never fail the parent operation.
- **Auto-links use the `auto:relates_to` prefix** to distinguish from human links.
- **`decay_lambda = 0.0` must preserve original ranking** (backwards-compatible).
- SQLite is the system of record; preserve schema, trigger, and FTS behaviour.

## Migrations

- Every schema change ships as a **new** migration — never edit an applied one.
- Schema contract lives in `docs/reference/schema.md`; keep it in step with code.

## Work Guidance

- Keep adapters thin: expose logic here, don't push it outward.
- Read `context/CRITICAL_WARNINGS.md` and `context/DOMAIN_RULES.md` before
  significant changes.

## Verification

- Tests: `cargo test -p clio-core` (inline tests + `tests/integration.rs`).
- Single test: `cargo test -p clio-core <name>`.
- Lint/format: `cargo clippy` / `cargo fmt`.
