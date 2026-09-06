# RedPen review resolution — 6 September 2026

Scope: the two queued reviews matched to the primary Clio checkout on `develop`
at `f8a9146`. The user approved fixing the valid finding and archiving both
reports. Changes are on `codex/fix-dated-waiting-items`.

## Findings and implementation

| Review | Verdict | Resolution |
| --- | --- | --- |
| `job-9d131c6c4690f57ecd6e4e9e7dde12cc1efd216a7fe268c3d36c8bb099e176e5.md` — medium | Valid | Exempt waiting items with either `due_at` or `remind_at` from the resume age cap. Future dates are not yet eligible for Needs attention, so the old filter removed these items from the brief entirely. |
| `job-f55fde466b77b077b8b68c3729536373b6857781d44d3af3803f5eda785e9f20.md` — low | Skip: already fixed | Commit `900d826` recognises `Conflict:` within stringified exceptions. The existing regression throws a standard `Error` and confirms the conflict state. No UI change is needed. |

## Verification

- The new `resume_waiting_age_cap_exempts_dates` test failed before the fix:
  an old item with a future due date was missing with `max_age_days = 14`.
- `cargo test --locked -p clio-core --no-default-features --lib assembly::tests`:
  **23 passed, 0 failed**. The regression covers future due dates, future
  reminders, both dates, undated items and a disabled age cap using the real
  resume builder and an in-memory database.
- From `ui/`, `node --experimental-strip-types --test --test-name-pattern='save failure kind' src/composables/useAutoSave.test.mjs`:
  **1 passed, 0 failed**.
- `git diff --check`: passed. The diff is limited to the waiting filter, its
  explanatory comment, the regression test and this record.

Both reports are approved for archival under `implemented/` after committing
this change. Their resolution notes will identify the commit and checks.

This restores the date exemption already documented in
[settings](../reference/settings.md#attention). It does not change eligibility
timing, stored attention data or the deployed application.
