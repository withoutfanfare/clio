# Direct work-reporting proof

This isolated proof lets synthetic sessions report task progress directly to
`clio-core`. Reports are observations: `implemented` never means human acceptance.
The proof is source code on the proof branch, with no installed integration.

## Run it

From the repository root:

```bash
cargo run --locked -p clio-core --no-default-features --example work_report_proof
```

The example opens a temporary SQLite database and uses three concurrent,
independent connections. It prints actual overview snapshots as JSON to stdout,
checks its assertions and removes the temporary database before exiting.
All sessions, projects, worktrees, revisions, timestamps and evidence are synthetic.
It never opens the configured Clio database or starts a service.

The example covers concurrent tasks, exact retries, delayed updates, rejected
conflicting duplicates, missing reporting, successor worktrees and unavailable evidence.
The focused integration checks run with:

```bash
cargo test --locked -p clio-core --no-default-features --test work_reports
```

## Core API

Use the functions in `clio_core::work_reports` with an explicit SQLite connection:

| Function | Result |
|---|---|
| `report(conn, report, received_at)` | Stores a validated report and returns its receipt |
| `history(conn, source, run_id)` | Returns the run's retained receipts in sequence order |
| `overview(conn, now, stale_after_secs)` | Groups latest run reports into tasks with freshness and conflict information |
| `overview_for_project(conn, now, stale_after_secs, project)` | Filters by an exact optional project identity |

`WorkReport` carries project, task and title; source, session and run identity;
sequence and observation time; worktree and revision; state, summary, next step,
next actor, evidence, evidence availability and an optional predecessor run.
Times are non-negative Unix seconds. Callers supply receipt and read times explicitly.
`report` owns its transaction and returns after commit; caller-owned transactions fail.

- `(project, task)` identifies a task; its title is a display label.
- `(source, run_id)` identifies a run. Session and worktree identify its origin.
- A run starts at sequence `0`. Later sequences belong to that same project,
  task, session and worktree; the reported revision can change.
- `(source, run_id, sequence)` identifies a report. An identical retry returns
  the original receipt; a changed payload under that identity is rejected.
- Lower sequences remain in history without replacing the latest report.
- A new worktree uses a new run with explicit `supersedes: Some(RunKey)`.
  The predecessor must belong to the same task; its later reports remain historical.

## Read the overview

Reports use `running`, `waiting`, `stopped` or `implemented`. The next actor is
`agent`, `user`, `other` or `none`. Evidence is a list of references, with
`current` or `unavailable` availability; the proof does not verify those references.

Freshness is calculated when reading. Replaying a receipt does not refresh it,
and receiving an old observation late does not make it current.
A stale running session appears as `reporting_missing`, retaining its last report.
Waiting for the user appears as `needs_user`. Other recorded states remain visible
with their freshness flags. Runs without successors produce `conflict` when their
state, next actor or next step disagree, even when stale.
`source_unavailable` exposes unavailable evidence alongside the reported task state.

Read each run's receipt, observation time, freshness and worktree before relying
on a task summary. `implemented` records the reporter's claim and needs human review.
No acceptance state or authority is inferred from local checks or silence.

## Limits

This proves core persistence and aggregation with disposable synthetic data.
It does not prove reporting by real sessions, live evidence access, delivery
reliability, installed app behaviour or human acceptance.
CLI, MCP, desktop and optional client interfaces are documented in
[Direct Work Reporting](work-reporting.md); the example above exercises the core only.
No live client hooks are installed and no separate user tracking file is needed.
No application installation, real memory write or external service is required.

See the [schema reference](reference/schema.md) for the persistence contract.
