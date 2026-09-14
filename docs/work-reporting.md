# Direct work reporting

Sessions can submit explicit progress reports through the CLI or MCP.
The desktop **Work** page reads these reports, grouped by project and task.
`implemented` appears as **Ready for review**; acceptance remains the user's decision.
The interfaces can be used directly or through the optional native agent checkpoint wrapper below.

## Try an isolated local report

From the repository root, build the CLI without installing it:

```bash
cargo build --locked -p clio-cli --no-default-features
work_demo=$(mktemp -d)
cat > "$work_demo/report.json" <<JSON
{
  "project": "sample-project", "task": "sample-task", "task_title": "Sample task",
  "source": "sample-client", "session_id": "sample-session", "run_id": "sample-run",
  "sequence": 0, "observed_at": $(date +%s),
  "worktree": "sample-worktree", "revision": "sample-revision",
  "state": "running", "summary": "Started the sample task",
  "next_step": "Run the sample checks", "next_actor": "agent",
  "evidence": [], "evidence_status": "current", "supersedes": null
}
JSON
python3 scripts/work_report_client.py --spool "$work_demo/queue" \
  --db-path "$work_demo/work.sqlite" --clio "$PWD/target/debug/clio" \
  publish < "$work_demo/report.json"
target/debug/clio --local --db-path "$work_demo/work.sqlite" work overview
```

This creates disposable sample data at the location held in `work_demo`.
Use `printf '%s\n' "$work_demo"` to find it; keep its queue until `pending` is zero.
The desktop uses its configured destination; it will not automatically read this sample database.

## Enable reporting in a client

Choose an explicit local database, dedicated private queue and CLI binary.
[`scripts/work_report_client.py`](../scripts/work_report_client.py) requires
`--db-path`, `--spool` and `--clio` for every action. It forces `--local` and pins
the queue to the resolved database path. Remote publishing is not supported.
Direct CLI commands can follow the configured shared route; see the [CLI reference](cli-reference.md#work-reports).

1. Register sequence `0` with explicit project, task, source, session, run,
   worktree and revision. Use the known project context; never infer an issue from a branch name.
2. Publish meaningful progress, waiting, review-ready and stopped observations.
   Once integration is enabled, reporting requires no repeated user prompt.
3. Increment `sequence` for each new observation and set `observed_at` to current Unix seconds.
   The client supplies sequences; the publisher orders queued updates but does not allocate them.
4. Retry the same payload unchanged. A reused identity with different content is rejected.
5. On handover, register a new run at sequence `0` with
   `supersedes: {"source": "previous-source", "run_id": "previous-run"}`.
   Keep the task identity; previous session and worktree reports remain available.

Submit the complete report JSON on stdin to `publish`. Omitted `supersedes`
becomes `null`. `state` is `running`, `waiting`, `stopped` or `implemented`;
`next_actor` is `agent`, `user`, `other` or `none`. Evidence references use
`evidence_status: "current"` or `"unavailable"`; references are not independently verified.

## Queue and retries

The publisher saves private jobs before delivery. It removes a job only after
receiving a receipt with the same report payload and integer receipt fields.
Failed delivery retains the job and blocks later updates for that run, allowing other runs through.
Successful `publish` and `drain` return exit `0`; queued failures return exit `2`
with `confirmed`, `pending` and `errors`. Argument or setup errors fail separately.
Correct the cause and retry; do not change queued identities.

Retry the sample queue once, or opt into a foreground watcher:

```bash
python3 scripts/work_report_client.py --spool "$work_demo/queue" \
  --db-path "$work_demo/work.sqlite" --clio "$PWD/target/debug/clio" drain
python3 scripts/work_report_client.py --spool "$work_demo/queue" \
  --db-path "$work_demo/work.sqlite" --clio "$PWD/target/debug/clio" watch --interval 5
```

`watch` defaults to five seconds between attempts; stop it with Ctrl+C.
It retries queued reports and does not generate progress or install a background service.
Raw CLI and MCP report calls have no durable client queue; their caller owns retry delivery.

## Reading Work

The Work page refreshes on entry and ten seconds after each completed request.
It shows task position, next actor, next step, freshness and evidence availability.
Collapsed projects show the prioritised task’s current summary and next action,
labelled **Agent next**, **Danny next**, **Other next** or **No action required**.
Conflicts and unknown owners remain **Next owner unresolved**. Expanding a project
hides these header lines; each task retains its own summary and action.
Each selected task shows the age of its latest current report's observation, excluding
superseded runs. This is separate from **Dashboard checked**, the last successful
poll time. Ages update on the existing refresh cycle, including failed refreshes;
missing or future report times are labelled explicitly. Exact receipt times remain
inside the evidence disclosure.
The overview lists tasks newest report first across projects, with visible dates
and times. Superseded observations cannot raise a task in the list. Polling keeps
the selected task stable. Compact rows show titles and states; the selected detail
pane shows the current summary, next owner and next action. Needs you and Accepted
filter the same chronological list. Narrow windows use a list/detail flow with
Back to updates. Stale and conflicting work retains a visible status warning.
Implemented changes offer a review action; old terminal observations retain their
age without being presented as stalled work. Acceptance decisions and previous
runs remain collapsed inside the selected task. The original shell, background
and colour tokens are preserved. Expand Reports & evidence for receipts and files.
Local report evidence offers **Open** for existing regular files inside that run's
worktree. Supported extensions are `md`, `txt`, `log`, `json`, `csv`, `pdf`, `png`,
`jpg`, `jpeg`, `webp` and `gif`. Relative paths resolve from the stored worktree;
absolute paths must remain inside it after canonicalisation. URLs, fragments,
line references, traversal, symlink escapes, directories and other file types
remain plain text. Acceptance and recommendation references remain unchanged.
The native command takes a receipt ID and an exact evidence reference, loads its
stored worktree and validates again on every click before using macOS's default
file opener. A changed or missing file fails visibly. No shell, opener dependency,
remote opening, reporting mutation or acceptance action is introduced.
Failed refreshes retain the last result with an unverified warning; remote failures never fall back to local data.
The page has no reporting controls. In local mode, **Review change** opens the
exact report, its evidence and **Accept this change**. **Keep unaccepted** closes
it without a decision. A saved acceptance applies only to that receipt and is
recorded as Danny's decision. Changed or handed-over reports cannot be accepted
from an older review panel. Remote mode does not offer acceptance controls.
Copying a recommendation changes only the clipboard.

Reports become stale after 300 seconds by default. A stale running report shows
`reporting_missing`; waiting for the user shows `needs_user`. Conflicting active
runs remain visible. Neither silence nor local checks establish acceptance.
See the [MCP contract](reference/mcp-contract.md#work_report) and [core proof](work-reporting-proof.md) for exact contracts and isolated verification limits.

## Native agent checkpoints

`scripts/work_report_hook.py` adds automatic reporting instructions to `UserPromptSubmit` and a bounded reporting check at `Stop`. A deployment supplies an explicit config with `clio`, `db_path`, `state_dir` and `projects: [{id, roots}]`; paths must be absolute. Only matching project roots participate. Database and SQLite sidecar symlinks are rejected.

An agent submits compact fields (`task`, `task_title`, `state`, `summary`, `next_step`, `next_actor`, `evidence`, `evidence_status`, optional `supersedes`) through the command supplied by the hook. The wrapper supplies identity, sequence and timestamps, preserves pending payloads and retries on subsequent hook/report activity. A returned receipt confirms delivery; queued status does not. At most one Stop continuation is requested before a visible incomplete-reporting warning.

This wrapper needs no transcript extraction, additional model or permanent service. Hook configuration alone is not proof of coverage: Codex requires trust of each new definition, and participating clients must load their updated configuration. Validate a native event before calling a client connected. The optional wrapper is local-only; remote CLI/MCP support is unchanged.

## Human acceptance and next-task guidance (local pilot)

Agent reports remain observations. The local dashboard's **Review change → Accept
this change** flow uses the existing acceptance store. Its core command takes a
receipt ID and holds a write transaction while verifying it is still the current,
non-superseded, implemented report. It saves the report title as the scope and
retains the immutable receipt binding. Repeated confirmation returns the stored
decision. Later reports require their own review; acceptance never starts another
task. A lost response can be retried without duplicate decisions. The UI pins the
report being reviewed across refreshes and never silently targets a newer report.

An operator may also record an **explicit human
acceptance** using `clio --local --db-path <dedicated-db> work accept -` with JSON
fields `receipt_id`, `scope`, `accepted_by`, `accepted_at` and `evidence`.
Look up the implementation receipt with `work history` first. Cite the actual
human decision; never infer acceptance from a successful build or a report.
`accepted_at` records when that decision was entered, bounded by the receipt's
observation time and the current clock. The decision is immutable and an exact
retry is idempotent. It accepts only that receipt's stated scope, not subsequent
work in the same run, another run, or the whole project. Original reports remain
unchanged. Accepted current receipts no longer request attention; acceptance of
older receipts remains visible alongside current progress.

Use `clio --local --db-path <dedicated-db> work recommend -` with `project`,
`parent_task`, `task`, `task_title`, `reason`, `next_actor`, `prompt`, `evidence`
and `checked_at` to store one prepared recommendation per reporting task. The
agent must first check the cited local records for unresolved work. There is no
planner or model service: this is a durable projection of that evidence-backed
choice. Newer guidance replaces older guidance; same-time conflicting or older
writes are rejected. The target task must not already have reports, and the
existing overview hides the recommendation once that task reports.

Both commands require `--local` and an explicit database path and never route to
Atlas. They have no hook or MCP write entry point. The dashboard's existing
refresh loads decisions and guidance with reports, displays recommendations
separately from active work, and exposes their source references behind details.
**Copy session prompt** copies text only; it does not authorise work, launch a
session or accept anything. A failed copy leaves a selectable read-only prompt.
These inputs are operator-supplied evidence, not independently verified facts;
unreported completion elsewhere still needs reconciliation with the local records.
