# Server and hooks review — 14 November 2026

A review of the Clio server side (MCP server, SSH bridge, daemon, background
jobs in `clio-core`) and the session hooks (`~/.claude/personal-skills/clio-hooks`).
Evidence came from the live hook log, the capture spool and its health log, a
live Codex test, and reading the code.

## The main finding

Session capture lost four days of work and nobody was told.

- From 1 to 5 November the OpenAI account had no credit. The capture queue
  treated that like a fault in each job: four tries in fifteen minutes, then
  the job was moved to the permanently-failed ("dead-letter") pile. 474 jobs
  went that way; with older ones, **535 jobs (about 5.5 million characters of
  session digests) are still in the pile**. The same thing happened in August
  (roughly 800 jobs).
- The warning for this was printed to the hook's error stream with a success
  exit code, which neither the agent nor the user ever sees.
- The hourly healthcheck that checks the pile runs on Atlas, where there is no
  spool, so it skipped the check.

The payloads are retained. Nothing is lost yet, but nothing has been replayed.

## Fixed in the hooks (`~/.claude` commit `a98b9f7`, 51 tests pass)

| Problem | Fix |
|---|---|
| An outage dead-lettered every queued job | Outage-class failures (Atlas unreachable, provider refusing the account, rate limit, server error) charge no attempts. The whole queue pauses with a backoff of up to an hour and drains when the backend answers. A failure is still charged to its job if another job succeeds in the same pass, so one bad job cannot hide as an "outage". |
| Dead or stuck jobs were announced where nobody could see | The warning goes to the hook's standard output, which the agent reads, once per session. |
| Codex's throwaway runs (next-message suggestions, memory consolidation) ran every hook | Codex sends `"transcript_path": null` for these (checked live on codex-cli 0.167.2). Every hook now skips them. About 17 a day each loaded a full brief, logged its memories as "shown" to a session nobody read — which counts towards the 20-session limit that silences an undated task — and left a pending job for a day. |
| The Codex stop hook searched for its transcript by filename | It uses the path Codex now supplies, and falls back to the search. |
| A requeued dead letter got one more try, not four | `retry` resets the attempt count and lifts any pause. |
| A waiting Codex discovery job made the queue look stuck | Discovery jobs no longer count towards pending age. |
| Session-start header said "GMT" all year | It uses the real time zone. |

## Fixed in the server (commit `69921db`, 397 tests pass)

| Problem | Where | Fix |
|---|---|---|
| Storing a memory with no title crashed when a multi-byte character (a dash, a curly quote, £) sat at byte 80 of the first line. Reproduced against the installed build. | `clio-core/src/title.rs` | Cut on a character boundary. |
| Old open follow-ups dropped out of resume once a namespace had 500 newer items of any status | `attention.rs` `eligible` | Fetch open and snoozed items by status. |
| Default recall raised the access count of memories it never returned, which feeds ranking | `repository.rs` `recall_scoped` | Track access once, for the returned page. |
| No timeout on any provider call | `capture.rs`, `title.rs`, `embeddings.rs` | 10 s to connect; 180 s total (60 s for embeddings). |
| With shared storage, `clio export --output file` reported success and wrote the file on Atlas | `clio-cli/src/main.rs` | A file path is refused; `--output -` streams locally. Same for import. |
| `memory_remember` said upsert needs `source` and `source_ref`, omitting `upsert: true`; following it produced a constraint error on the second call | `clio-mcp/src/main.rs` | Wording corrected. |

**Not deployed.** These are on `develop` only. The title crash is live on
Atlas and both Macs until the next release.

## Open, in rough order of value

Items marked *reported* came from a reviewer reading the code and were not
re-verified line by line.

1. **A resume that times out still marks its items as shown** (*reported*;
   `assembly.rs:873-894`). The brief records "surfaced" events before it is
   printed, so when the hook gives up after 8 s the items are still suppressed
   for that session. Needs a small design change: the hook acknowledges what
   it actually injected.
2. **`memory_action add` is implemented twice, in the MCP server and the
   CLI** (`clio-mcp/src/main.rs:2876-2936`, `clio-cli/src/main.rs:3014-3075`),
   against the rule that logic lives in `clio-core`. The MCP copy does not roll
   back if `COMMIT` fails, which would leave the shared connection inside a
   transaction until restart (*reported*).
3. **The Mac's capture spool has no monitor.** The in-session warning now
   covers it, but only while someone is working.
4. **AI titles are generated inside `remember`, under the MCP lock.** Latent:
   AI titles are switched off.
5. **A checkpoint fails whole if the model returns a date that is not strict
   RFC 3339** (*reported*; `checkpoint.rs:266-279`). No sign of it in practice:
   two validation dead letters in the whole health log, neither of this kind.
6. **`valid_until` is never validated**, and two code paths compare it
   differently (*reported*; `assembly.rs:947-951`, `repository.rs:1130`).
7. **The consolidated summary can be marked fresh while missing a memory**
   written during the model call (*reported*; `consolidate.rs:278`).
8. **Daemon inbox watcher** never scans at start-up, so a file that failed or
   arrived while it was down is never processed; on this Mac it writes to the
   local database while every reader uses Atlas (*reported*).
9. **Smaller server items** (*reported*): `memory_suggest_links` embeds while
   holding the lock; one line over 8 MiB ends a remote MCP session; the bridge
   drops a checkpoint's working folder and host; `memory_search`'s `rank` is
   documented as cosine similarity but is a composite; `memory_action` history
   defaults to 20 where the CLI defaults to 50; an invalid `sort_by` is
   ignored silently; `merge_memories` does not move open follow-ups; a task
   shown only under "Relevant knowledge" still counts towards the 20-session
   limit.
10. **Smaller hook items:** per-session state files are never pruned (7,058
    files, 15 MB); the stop hook's "not queued" warning also goes to the unseen
    error stream; the hook tests write to the real hook log; `clio migrate`
    and `clio repair` still take paths on the shared host.
11. **The repository is not `rustfmt`-clean**, so `cargo fmt --all` rewrites
    eight unrelated files.

## Checked and not a problem

- `resume --query` is fast (0.12 s end to end, measured) and uses full-text
  search only; it does not touch the embedding scan. The 8 s timeouts in the
  log cluster on 10 November and follow Atlas or the link being slow.
- Checkpoints are exact-once; retries cannot duplicate memories.
- Archived and expired records stay out of recall, search and resume.
- Access tracking never fails its parent call.
