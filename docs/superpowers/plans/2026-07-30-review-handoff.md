# Review handoff — 30 July 2026

A single long session made eleven commits to Clio, changed live configuration on two
machines, mutated the production memory database six times, and added a monitored cron schedule. None of it has
been reviewed by anyone but its author. This is the brief for that review.

Use the prompt at the end to start a fresh session.

## Commits to review

All on `develop`, oldest first:

| SHA | Subject |
|---|---|
| `dc25858` | `feat(settings)`: prefer `OPENAI_API_KEY_CLIO` over the shared key |
| `5acf1e5` | `feat(capture)`: record cached input tokens from the provider |
| `557d7e3` | `docs(operations)`: record the GPT-4.1 switch and measured prompt caching |
| `fcd009e` | `fix(cli)`: report the namespace a dry run would actually store |
| `b26d511` | `fix(embeddings)`: enforce `max_links_per_memory` as a total, not per run |
| `1aca253` | `feat(cli)`: add auto-link pass and drain MCP sessions on release |
| `7e3ab6a` | `fix(cli)`: drive auto-link to completion instead of one batch |
| `6ebc05f` | `feat(embeddings)`: exclude boilerplate kinds from auto-linking |
| `2ed253c` | `test(bench)`: add retrieval benchmarks for tuning on measured outcomes |
| `4c8a9ad` | `docs(plans)`: add a review handoff for the 30 July session |
| `1c3cc1e` | `feat(ops)`: add a liveness signal and Slack alerting for background work |

```sh
git log --oneline 223816f..HEAD
git diff 223816f..HEAD --stat
```

## Where the risk is concentrated

Ranked by how much damage a defect would do, not by lines changed.

1. **`crates/clio-core/src/embeddings.rs` — auto-link changes.** Three separate
   changes landed here: a cumulative link cap, kind exclusion, and a dynamically
   built SQL `IN` clause with hand-numbered placeholders (`?5`, `?6`, …) in
   `suggest_links_excluding_kinds`. The placeholder numbering is the most fragile
   thing written all session: it must stay in step with the fixed `?1`–`?4`
   bindings, and nothing tests it with more than one excluded kind.

2. **`scripts/atlas-release.sh` — `drain_mcp`.** Sends `SIGTERM` to processes on a
   production host that also serves several live websites. It was tested against a
   differently-named stand-in and then observed draining two real sessions. Check
   the `pgrep -x` exactness, that it cannot match unrelated processes, and that
   failure to drain never aborts a deploy midway.

3. **`crates/clio-cli/src/main.rs` — `cmd_auto_link`.** An unbounded `loop` whose
   only exits are "no memories processed" and "watermark did not advance". A logic
   error here spins against a database. The first version of this function was wrong
   in exactly this area — it processed one batch and stopped.

4. **`crates/clio-core/src/settings.rs` — key resolution and `exclude_kinds`.**
   Secret handling: `api_key_from_env` falls back to a shared key and warns. Confirm
   the warning cannot leak the key itself, and that `exclude_kinds` defaults
   correctly for settings files written before the field existed.

5. **`crates/clio-cli/src/main.rs` — dry-run namespace resolution.** Duplicated
   precedence logic between `cmd_capture` (inline) and `cmd_distill` (via
   `resolve_distill_namespace`). Those two can drift apart.

6. **`scripts/clio-healthcheck.sh`.** New, and it holds a Slack webhook path and
   builds shell arrays from command output. Check that a missing log, an unreadable
   database or an absent `crontab` binary degrade to a reported problem rather than a
   crash — a monitor that dies silently is worse than none. Also confirm the webhook
   cannot reach a log or an error message.

## Things known to be unverified

Do not treat these as reviewed and passed. They are gaps, listed so the review can
close them rather than rediscover them.

- **The cumulative link cap has never run in production.** `auto_link` was disabled
  when it was written. It is proven only by unit tests plus benchmark runs on copies.
- **No test covers `exclude_kinds` with more than one entry**, which is the case that
  would expose a placeholder-numbering error.
- **`drain_mcp` has no automated test.** Verified by hand only.
- **Consolidation gating at the raised `auto_threshold` of 50 has never been
  observed** — it needs fifty new memories in a namespace to trigger.
- **Retrieval quality rests on a tag-overlap proxy.** Tags come from the same model
  pass as the content, so the benchmarks rank configurations against each other but
  do not measure absolute relevance.
- **`recall-eval.py` scores `brief`, not `resume`.** The prompt hook calls `resume`,
  which assembles a different mix and is unmeasured.
- **`clio-healthcheck` has never fired from cron.** Every path was exercised by hand
  on Atlas and the Slack messages were confirmed received, but the first scheduled run
  at `:47` had not happened when this was written.
- **87 archived memories were classified by a title regex, not read individually.** If
  that regex over-matched, something substantive is now hidden. Reversible with
  `clio unarchive`; the pre-prune backup is listed above.
- **55 links point at or out of archived memories** (23 inbound, 32 outbound), left
  behind by the prune. Linked recall is supposed to honour archive visibility, and that
  has not been tested. Roadmap CLIO-OPS-007.

## Live state a reviewer should know

Changed outside version control, and not recoverable from the repository:

- **Atlas** (`145.239.6.222`, also serving live websites) — capture model `gpt-4.1`
  (was `gpt-5.6-terra`); `consolidate.auto_threshold` 50 (was 10);
  `auto_link.threshold` 0.6 (was 0.8); `max_links_per_memory` 5 (was 3);
  `exclude_kinds` `["receipt"]`; `auto_link.enabled` true. Two hourly cron entries:
  `clio auto-link` at `:17` and `clio-healthcheck` at `:47`. `sqlite3` CLI installed.
  Slack webhook at `~/.config/clio/alerting.env`, mode `0600` — a secret that exists
  nowhere else and is not in version control.
- **Mac** — `auto_link.enabled` false, so it cannot link its own copy.
  `consolidate.auto_threshold` 50. SSH multiplexing added for `atlas` in
  `~/.ssh/config` (backup: `~/.ssh/config.bak-premux-20260730-030838`). The local
  database was replaced with a consistent Atlas snapshot on 30 July — it had been
  three weeks stale at 3,257 memories against Atlas's 3,788 (backup:
  `mac-pre-atlas-sync-20260730T123627Z.db`).
- **Hooks** (`~/.claude/personal-skills/clio-hooks`, commit `c2372db`) —
  `MAX_DIGEST_CHARS` 24,000 (was 80,000); `MAX_ATTEMPTS` 4 (was 8); a skip gate for
  deltas containing no human input, on both the Claude and Codex paths.

Database mutations, each with a backup in `~/.local/share/clio/backups/`:

| Action | Backup |
|---|---|
| Deleted 557 stale auto-links | `pre-linkclean-20260730T014756Z.db` |
| Merged `project:clio` into `clio` (44 memories) | `pre-nsmerge-20260730T081227Z.db` |
| Cleared and rebuilt all auto-links | `pre-relink-20260730T081406Z.db` |
| Threshold 0.8 → 0.6, rebuilt links | `pre-threshold-20260730T100616Z.db` |
| Cap 3 → 5 with receipts excluded, rebuilt links | `pre-cap5-20260730T111425Z.db` |
| Archived 87 memories (noise, duplicates, superseded) | `pre-prune-20260730T121111Z.db` |

Also: 13 review-queue items were approved, 9 remain pending — of those 9, all are
believed complete, but rejecting them was never authorised.

## The outcome that justifies all of it

The session began with an unexplained OpenAI bill. Measured on the provider dashboard:
**947 requests and \$8.93** the day before, against **67 requests and \$1.43** the day
after — an 84% cost reduction and a 93% fall in request volume. The request drop
exceeds the cost drop, which says the volume fixes did more work than the model swap.

Separately, linked recall was shown to be the dominant driver of brief quality: briefs
go from 2.3 results at 33.2% precision without links to 7.7 at 59.6% with them, and
the memories linking adds are 57.4% relevant against a 6.3% chance baseline.

Both figures are worth re-deriving rather than trusting. The cost figure came from the
operator reading a dashboard; the recall figure rests on a tag-overlap proxy that
shares its origin with the embeddings it scores.

## Known defects not yet fixed

- **`clio inbox list` truncates silently.** It defaults to `--limit 20` and prints
  "20 pending item(s)" when there are 22, unlike `recall`, which reports
  "Showing 1-5 of 50".
- **Duplicate memories.** The queue held three near-identical "deploy the binary"
  items and three near-identical namespace-merge items; deduplication is not
  catching them. There are now also two memories describing the same shell-quoting
  bug — one written deliberately, one distilled from the session that discussed it.
- **`max_links_per_memory` bounds outgoing links only.** Recall walks edges in both
  directions, so total degree is unbounded — observed at 23 against a cap of 5. The
  documentation now says so; whether that is the right design is open.

## The prompt

Copy the following into a new session.

---

Review the work from the 30 July 2026 session on this repository. Read
`docs/superpowers/plans/2026-07-30-review-handoff.md` first — it lists the commits,
where the risk is concentrated, and what was never verified.

Two jobs, in order.

**1. Bug hunt.** Go after correctness, not style. The areas most likely to be wrong,
with reasons given in the handoff, are: the dynamically built SQL `IN` clause with
hand-numbered placeholders in `suggest_links_excluding_kinds`; the unbounded loop in
`cmd_auto_link`; `drain_mcp` in `scripts/atlas-release.sh`, which signals processes
on a production host; the duplicated namespace-precedence logic between `cmd_capture`
and `cmd_distill`; and `scripts/clio-healthcheck.sh`, where a monitor that crashes
instead of reporting is worse than no monitor. For each finding, give the input or
state that triggers it and what goes wrong — not a description of the code.

Pay particular attention to cases nothing tests: `exclude_kinds` with two or more
entries, an empty `exclude_kinds`, a memory whose kind is null or unexpected, a
watermark that repeats, and a database where every memory is already at its cap.

**2. Code review.** Judge whether the changes are right, not merely working. Ask
whether `max_links_per_memory` should bound total degree rather than outgoing links;
whether `exclude_kinds` belongs in settings or is over-configuration; whether the
duplicated namespace logic should be unified in `clio-core`; and whether the
benchmark tools under `scripts/bench/` measure what they claim, given that their
ground truth is tag overlap generated by the same model pass as the content.

Then verify. `cargo test -p clio-core` should report 179 passing, `cargo clippy` and
`cargo fmt --check` clean, and `scripts/bench/link-invariants.sh` 8 of 8. Run them
rather than assuming. Do not change live configuration or the production database:
the handoff records the current live state, and every database mutation has a backup
listed there.

Be adversarial about the session's own conclusions. Several were wrong before being
corrected — a namespace bug that turned out to be a dry-run reporting artefact, a
threshold sweep invalidated by shell quoting that silently skipped its `DELETE`, and
a benchmark that reported six passes with zero fixtures. Assume more of that
survived.
