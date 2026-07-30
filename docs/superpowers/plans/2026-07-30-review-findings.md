# Review findings — 30 July 2026 session

Independent review of the work recorded in `2026-07-30-review-handoff.md`
(commits `223816f..37e35d6` on `develop`). Every finding below was confirmed by
reading the code or running it; nothing is carried over from the handoff on trust.

## Verification (all four gates pass)

| Check | Expected | Observed |
|---|---|---|
| `cargo test -p clio-core` | 179 passing | **179 passed, 0 failed** in the lib binary (plus 67 integration, 9 multi-connection — 255 total) |
| `cargo clippy --workspace` | clean | **0 warnings** |
| `cargo fmt --check` | clean | **clean** |
| `scripts/bench/link-invariants.sh` | 8 of 8 | **8 passed, 0 failed** (run against a release binary built from HEAD, not the installed one) |

Nothing on the live machines or in the production database was touched.

## Blockers — the monitoring added to close a silent-failure gap can itself fail silently

### B1. A failed Slack alert is recorded as sent and never retried

`scripts/clio-healthcheck.sh:144-152`

```sh
notify "$message" || true
printf '%s' "$signature" > "$CLIO_STATE_FILE"
```

The state signature is written whether or not `notify` succeeded. If Slack is
unreachable for the fifteen seconds the first unhealthy check runs (or the
webhook rejects, or `python3` raises), the alert is lost — and every later run
sees the same signature, matches it against the state file, and prints
"(already alerted for this state; not repeating)". **A persistent fault is
reported zero times.** Lines 130-132 mirror the bug for the recovery message.

Fix is three lines: only write the state file when `notify` returns success.

### B2. The healthcheck reports "healthy" while auto-link is completely broken

`scripts/clio-healthcheck.sh:72-84` checks the log file's age and greps the last
five lines for `error|panic|failed`. Four independent ways through it:

1. **Freshness is log mtime, not success.** Any run that writes anything —
   including the shell's own `clio: command not found` — refreshes the mtime.
   ("Command not found" also matches none of the grep words.) The crontab line
   itself is not in the repo; for the log to receive anything at all the entry
   must redirect stderr (the CLI summary goes to stderr), which means mtime
   freshens on failures too.
2. **The failure line scrolls out of the window.** When batch embedding fails,
   the one line containing "failed" is followed by up to 50 per-memory warnings
   and then 50 "skipping — no embedding available" lines
   (`embeddings.rs:1094-1138`), none of which match the grep. `tail -5` sees
   only the tail.
3. **`clio auto-link` exits 0 having done nothing.** The no-embedding skip path
   (`embeddings.rs:1134-1141`) advances the watermark but does **not** count the
   memory as processed. A batch in which every memory lacks an embedding makes
   `memories_processed == 0`, which `cmd_auto_link` treats as "corpus finished"
   (`main.rs:3216`) — it stops early, skips everything after that batch, prints
   "Auto-link complete: 0 memory(ies)" and exits 0. (Contrast: the
   excluded-kind skip *does* count, so a batch of receipts doesn't halt the
   run. The asymmetry is the bug.)
4. **Link-write failures are invisible.** `repository::link` errors log at
   `debug!` (`embeddings.rs:1178`); the CLI default filter is `warn`
   (`main.rs:1121-1123`). A lock-contention storm looks like a clean run that
   created 0 links.

Fix: require the success signal instead of denylisting failure words (grep the
recent log for "Auto-link complete"), and make `cmd_auto_link` exit non-zero
when memories were skipped for want of an embedding.

### B3. `clio auto-link` ignores `auto_link.enabled` — the recorded Mac safety property does not exist

`main.rs:3198-3201` clones the config and reads only `threshold`; `enabled` is
never consulted. The daemon does check it (`clio-daemon/src/main.rs:161`); the
new CLI path doesn't. Two consequences:

- The handoff's live-state note "**Mac** — `auto_link.enabled` false, so it
  cannot link its own copy" is false. Anyone (or any script) running
  `clio auto-link` on the Mac writes `auto:relates_to` rows into the local
  snapshot, silently diverging it from Atlas.
- On Atlas, setting `enabled: false` does not stop the hourly cron. The only
  off switch is deleting the crontab entry — which the healthcheck then flags
  as a fault (line 90).

Fix: refuse to run when `!config.enabled`, with a `--force` escape hatch.

## Major — the test and benchmark evidence proves less than it reports

### M1. The invariants suite cannot detect a cap regression, and never exercises `exclude_kinds`

`scripts/bench/link-invariants.sh` builds a 5-memory cluster. Processing is
oldest-first and pairs already linked in either direction are excluded, so the
achievable outgoing degrees are 4/3/2/1/0 — **this run's own output shows
maximum outgoing degree 4 against a cap of 5**. The "max_links_per_memory is
respected" assertion can never bind; delete the entire cap block from
`embeddings.rs:1143-1156` and the suite still reports 8 of 8. The
"second run is idempotent" check doesn't cover it either — that 0 comes from
every pair being linked already, not from the cap. Reproducing the bug
`b26d511` fixed needs a new similar memory added *between* passes.

Separately, the fixture settings exclude `receipt` (line 59) but every fixture
is created `--kind note` (line 45). Commit `6ebc05f` is entirely unexercised by
the suite that exists to prove the design rules.

### M2. The cap unit test asserts the standard library, and the new SQL path has zero test callers

`embeddings.rs:1405-1412` — `remaining_budget_never_underflows_when_already_over_cap`
tests `cap.saturating_sub(17) == 0` on plain integers. It touches no Clio code:
replace `saturating_sub` with `-` in the real code and the test still passes.
And `suggest_links_excluding_kinds` has exactly two callers in the workspace,
both production code — so `exclude_kinds` with two or more entries (the case
the handoff feared for the placeholder numbering) is untested anywhere.

A real test would go through `auto_link_batch`: seed a memory with 4 existing
auto-links against a cap of 5, assert exactly one new link; seed 6, assert
zero and no underflow.

### M3. `dial-in.sh` asserts the DELETE but not the relink, on a possibly-stale copy

- Line 55: the auto-link run's exit code is discarded and nothing requires
  `links > 0` before scoring. The link count *is* printed, so a `links=0` run is
  visible to a careful reader — but nothing stops the trial producing
  plausible-looking recall numbers from an unlinked database. This is the same
  failure class the adjacent DELETE assertion exists to prevent, and unlike
  `link-invariants.sh:24`, this script never sets `FASTEMBED_CACHE_PATH`
  (grep: zero references), so on a host without the default model cache every
  trial silently links nothing.
- Line 24: `cp "$SRC"` copies a WAL-mode database without its `-wal` sidecar —
  the copy is the last checkpoint, not current state, and can be torn if the
  source is being written. Use `sqlite3 "$SRC" ".backup …"` or `VACUUM INTO`.

### M4. The recorded sweep does not support the configuration chosen from it

`scripts/bench/README.md:60-72` records **threshold 0.60 / cap 5 twice** with
different results: 59.6% / MRR 0.712 (threshold table) and 59.0% / 0.749 (cap
sweep sentence). Same configuration, so the gap — 0.6pp precision, 0.037 MRR —
is run-to-run noise (the link graph is not reproducible: `ORDER BY
updated_at ASC` with no tiebreak, and the two runs were likely on different
database copies). Every decision made from the table is inside that gap:

- 0.60 vs 0.65: 0.1pp precision, 0.032 MRR.
- Cap 3 vs 5 vs 8: 0.4pp total span.
- The metric the README itself calls decisive — added precision — rises
  monotonically (48.7% → 57.4% → 66.1%) and favours **0.65**, not 0.60.
- The script sweeps 0.70 (line 73); the README never records it.

**The two live Atlas changes (threshold 0.8 → 0.6, cap 3 → 5) are therefore
unsupported by the evidence cited for them.** The one robust conclusion is
negative: cap 8 buys 1,134 more links for nothing. Also note the cap sweep
relinks from empty each trial, measuring a *single-pass* cap, while production
enforces a *cumulative* cap across hourly runs — different semantics.

Before trusting any of it: re-run each configuration over ≥3 seeds, report
spread, add `, id` to the candidate ordering, and record 0.70.

### M5. `recall-eval.py` largely measures embedding–tag agreement, not retrieval

The query is the held-out memory's own title (`recall-eval.py:144`), sent to a
single FTS section. FTS quoting joins the title's words with implicit AND, so a
candidate must contain every word of M's title — usually only M does, which is
why the baseline returns 2.3 results. The seed set is then approximately `{M}`,
and `--include-links` expands M's own auto-link neighbours — created by cosine
similarity on M's content and graded by tags from the same model pass. The
"57.4% relevant vs 6.3% chance = 9.1×" figure mostly bypasses retrieval.

Mechanics compound it (`recall-eval.py:148-166`):

- Arms are unpaired: `if got:` admits each arm independently, so the
  with/without means can be computed over different query subsets.
- A non-zero exit or bad JSON silently returns `[]` — failures are
  indistinguishable from empty briefs, stderr is discarded (and `dial-in.sh`
  discards it again), and the reported query count is the number *selected*,
  not the number that produced results.
- The headline numbers mix macro-averages (per-query means) with a
  micro-average (added-items pool), which is why 2.3 @ 33.2% → 7.7 @ 59.6%
  doesn't reconcile with 57.4%.
- The right comparison for linked recall's contribution is against the
  without-links arm (33.2%), not the 6.3% random baseline.

Also: nothing stops it running against the live database (the docstring
contradicts itself in one sentence — briefs are *not* read-only: recall calls
`touch_accessed` unless suppressed, and `assembly.rs` doesn't suppress it), and
`subprocess.run` has no timeout. The README's own caveat (rank-only, tag-proxy)
is honest; the roadmap citations of "9.1× lift" and "8 invariants all passing"
overstate what these tools showed and should be softened.

### M6. `capture` and `distill` namespace precedence have already drifted

The handoff worried they *could* drift. They have:

- `capture` (`capture.rs:662-664` + `main.rs:2099-2101`): explicit
  `--namespace` → **cwd** → model suggestion. A model that answers `global` is
  overridden by the working directory.
- `distill` (`resolve_distill_namespace`, `capture.rs:613-627`): explicit →
  **model's `global` promotion** → cwd → model.

Same classification, different destination. The dry-run fix itself is correct —
each command's preview mirrors its own store path exactly — but
`docs/operations/capture-model-benchmark.md` proposes measuring "whether a model
over- or under-uses `global`", which for `capture` is now unobservable in
dry-run output and discarded at store time. Unify in one `clio-core` resolver
and decide deliberately whether `capture` honours the `global` promotion.

## Minor

- **`drain_mcp`'s stated rationale is false** (`atlas-release.sh:245-246`,
  repeated in the CHANGELOG): "a server mid-write should be allowed to finish"
  presumes a SIGTERM handler; `clio-mcp` has none (grep: no signal handling),
  so SIGTERM kills immediately. The database is safe (WAL rollback + the OS
  releases the flock), but an in-flight write is lost and the client sees a
  transport error — on every deploy and rollback. On the handoff's questions:
  `pgrep -x clio-mcp` is genuinely exact ("clio-mcp" is under the 15-char comm
  limit); no branch can abort a deploy (everything returns 0). Two loose ends:
  the match isn't scoped to the invoking user, and `kill`'s permission failure
  is swallowed, so someone else's session reports as "still exiting — they
  will finish in their own time" when it was never signalled. Use
  `pgrep -u "$(id -u)" -x clio-mcp`.
- **Healthcheck defaults are macOS-shaped on a Linux host.** `CLIO_SPOOL`
  defaults to `~/Library/Application Support/...` (line 30), so the dead-letter
  and backlog checks silently self-skip on Atlas — the one monitored machine.
  The webhook variable must be *exported* by `alerting.env` for the python
  child to see it (line 36 vs line 50); if it's a plain assignment the shell
  check passes and `notify` throws `KeyError` — which B1 then converts into a
  permanently suppressed alert. Nothing documents the export requirement.
  Line 80 also forwards up to 160 chars of raw log text to the webhook —
  no current path carries a secret, but it is an unbounded egress from a file
  the script doesn't control.
- **Key handling** (`settings.rs:437-466`): the shared-key warning interpolates
  only the variable *names* — it cannot leak the key (handoff question
  answered). But `non_empty` trims for the emptiness test and returns the value
  untrimmed, so a key with stray whitespace/newline (`KEY=$(cat file)`) goes to
  the provider verbatim and fails as an opaque 401. And `EnvKeySource` derives
  `Debug` while holding the key in both variants — nothing prints it today; a
  future `{:?}` would. Trim on return; hand-write a redacting `Debug`.
- **No lock around the hourly `clio auto-link`.** Runtime is unbounded
  (embedding backend calls), the cron has no `flock -n`, and a losing writer
  logs at `debug!` — overlap manifests as "0 links created" with a clean log,
  feeding B2. `atlas-release.sh` requires `flock` already; use it in the cron
  entry too.
- **Hot-loop costs** (`embeddings.rs`): `kind` is fetched with a per-memory
  query (line 1120) instead of being selected in the candidate query;
  `has_embedding_for_space` runs twice per memory (1060, 1135); the "affordable
  because capped memories are skipped" argument only covers memories that reach
  the cap on *outgoing* count — an inbound-heavy memory (observed: total degree
  23) can sit below the outgoing cap forever and pay a full namespace scan
  every hour.
- **`prepare` vs `prepare_cached`:** the dynamically built statement in
  `suggest_links_excluding_kinds` bypasses the statement cache on every call in
  the per-memory loop.

## Handoff corrections — three feared items are actually sound

1. **The `?5`/`?6` placeholder numbering is correct.** The base SQL's highest
   parameter is `?4` (`?1` repeats, counted once); kind placeholders start at
   `?5` and the bindings vector appends kinds after exactly four fixed values,
   so `?{5+i}` maps to `bindings[4+i]`. rusqlite binds slices positionally and
   errors on a count mismatch, so any future drift fails loudly at bind time
   rather than mis-binding silently. Empty `exclude_kinds` adds no clause and
   binds four values — also correct. (It still deserves the missing multi-entry
   test, per M2.)
2. **A null `kind` cannot occur** — the column is `TEXT NOT NULL DEFAULT 'note'`
   (`migrations.rs`), and a `Vec<String>` cannot inject SQL NULL, so the
   `NOT IN (NULL)` trap doesn't apply.
3. **Linked recall already honours archive visibility.**
   `append_linked_memories` refetches linked ids through
   `get_many_eligible(conn, …, q.include_archived, q.exclude_expired)`
   (`repository.rs:744-745`), so the 55 dangling links around archived
   memories are wasted rows, not a visibility leak. CLIO-OPS-007 is a missing
   test and a cleanup, and can drop in priority.

Also checked from the handoff's "cases nothing tests" list: a repeating
watermark is effectively impossible today — timestamps come from `now_utc()`
with sub-second precision and no code path writes `updated_at` via SQL
`datetime('now')`. The batch query's strict `>` plus `LIMIT` would silently and
permanently skip rows tied at a batch boundary if bulk second-precision writes
ever appear; adding `, id` to the ordering and carrying `(updated_at, id)` as
the watermark would close it. A corpus where every memory is at its cap
terminates correctly (one COUNT per memory per hour — cost, not correctness).
The `cmd_auto_link` loop itself terminates: each pass's watermark is strictly
greater than the last because the query demands `updated_at > watermark`.

## Judgement calls the review was asked to make

- **Should `max_links_per_memory` bound total degree?** Yes. The cap counts
  outgoing edges while pair-exclusion is bidirectional, so enforcement is
  order-dependent (oldest cluster member spends the whole quota — the invariant
  run's own degree table shows 4/3/2/1/0) and inbound accumulation is unbounded
  (observed 23 against a cap of 5). Total degree is what governs how much
  context recall pulls in. Cheapest correct move: count both directions in
  `count_auto_links` without changing the setting's meaning.
- **Does `exclude_kinds` belong in settings?** The SQL-level filtering is the
  right mechanism (excluded kinds can't consume `limit` slots), but a
  user-facing list is over-configuration for one measured problem with one
  known member, bought at the cost of a dynamic SQL path with no test and a
  per-memory query. A `kind_is_boilerplate()` predicate in `clio-core` would be
  simpler and impossible to misconfigure. If the setting stays, it needs the
  multi-entry test.
- **Unify the namespace logic in `clio-core`?** Yes — not because it might
  drift but because it already has (M6).
- **Do the bench tools measure what they claim?** Not yet. The invariants suite
  proves namespace/archive isolation but not the cap and not kind exclusion
  (M1); the recall eval's decisive figure is near-circular with unpaired arms
  (M5); the dial-in sweep's recorded output contradicts the settings chosen
  from it (M4). The README's caveat is honest; the roadmap's citations of these
  numbers are stronger than the evidence.

## What this means for the session's headline claims

- **The cost fix stands on its own evidence** (provider dashboard: 947 → 67
  requests, $8.93 → $1.43). Nothing reviewed here undermines it, and the
  request-volume mechanism (fewer calls) is visible in the code.
- **"Linked recall is the dominant driver of brief quality" is directionally
  supported but overstated** — the honest version is: links add ~5.7 memories
  per brief whose tag-agreement is well above chance, measured by a proxy that
  shares its origin with the thing measured, via a baseline that degenerates to
  a self-lookup.
- **The specific live settings (0.6 / cap 5) are not evidence-backed choices**
  — they are inside the measurement noise of the sweep that chose them (M4).
  They are not shown to be *wrong*, either; re-run with seeds and spread before
  tuning further.

## Recommended fix order

1. B1 — gate the state-file write on `notify` success (3 lines, closes the
   worst silent-failure loop).
2. B3 — honour `enabled` in `cmd_auto_link` (protects the Mac snapshot today).
3. B2 — success-signal check in the healthcheck + non-zero exit on skipped
   memories.
4. M2/M1 — real cap + multi-entry `exclude_kinds` tests (they gate everything
   else claimed about linking).
5. M6 — single namespace resolver in core.
6. M4/M5 — seeded re-runs and paired scoring before any further tuning; soften
   the roadmap citations.
7. Minors as convenient; none is urgent.
