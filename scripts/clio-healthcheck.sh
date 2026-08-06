#!/usr/bin/env bash
# Liveness signal for Clio's background work, alerting to Slack on failure.
#
# Clio's scheduled work fails silently. Auto-linking was dead for roughly a week in
# July 2026 and only surfaced because someone went looking: the cron wrote to a log
# that nothing read. This closes that gap.
#
# Silent when healthy — it alerts only on a state change, so a persistent fault does
# not repeat every run, and recovery is reported once.
#
# Usage:
#   scripts/clio-healthcheck.sh          # check, alert on change
#   scripts/clio-healthcheck.sh --test   # send a test alert and exit
#
# Environment:
#   CLIO_SLACK_WEBHOOK      required to alert; read from CLIO_ALERT_ENV_FILE if unset
#   CLIO_ALERT_ENV_FILE     default ~/.config/clio/alerting.env (mode 0600)
#   CLIO_DB_PATH            default ~/.local/share/clio/memory.db
#   CLIO_AUTOLINK_LOG       default <db dir>/auto-link.log
#   CLIO_AUTOLINK_MAX_AGE   seconds; default 7800 (2h 10m — one missed hourly run)
#   CLIO_STATE_FILE         default <db dir>/healthcheck-state
set -uo pipefail

CLIO_DB_PATH="${CLIO_DB_PATH:-$HOME/.local/share/clio/memory.db}"
DB_DIR="$(dirname "$CLIO_DB_PATH")"
CLIO_AUTOLINK_LOG="${CLIO_AUTOLINK_LOG:-$DB_DIR/auto-link.log}"
CLIO_AUTOLINK_MAX_AGE="${CLIO_AUTOLINK_MAX_AGE:-7800}"
CLIO_STATE_FILE="${CLIO_STATE_FILE:-$DB_DIR/healthcheck-state}"
CLIO_ALERT_ENV_FILE="${CLIO_ALERT_ENV_FILE:-$HOME/.config/clio/alerting.env}"
# The capture spool exists only on capture clients (the hook scripts write it, at a
# macOS path); hosts that merely hold the database — Atlas — have none, and section
# 4 self-skips there by design. CLIO_CAPTURE_SPOOL matches the hooks' own override.
CLIO_SPOOL="${CLIO_SPOOL:-${CLIO_CAPTURE_SPOOL:-$HOME/Library/Application Support/clio/capture-spool}}"

problems=()

if [[ -z "${CLIO_SLACK_WEBHOOK:-}" && -r "$CLIO_ALERT_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  . "$CLIO_ALERT_ENV_FILE"
fi
# The env file may assign without `export`, which sets a shell variable the
# python child in notify() cannot see. Export whatever we ended up with.
[[ -z "${CLIO_SLACK_WEBHOOK:-}" ]] || export CLIO_SLACK_WEBHOOK

notify() { # notify <text>
  if [[ -z "${CLIO_SLACK_WEBHOOK:-}" ]]; then
    printf 'no webhook configured; would have sent:\n%s\n' "$1" >&2
    return 1
  fi
  # --fail so a rejected webhook is an error rather than a silent success, which
  # would recreate exactly the blind spot this script exists to remove.
  printf '%s' "$1" | python3 -c '
import json, sys, urllib.request, os
text = sys.stdin.read()
req = urllib.request.Request(
    os.environ["CLIO_SLACK_WEBHOOK"],
    data=json.dumps({"text": text}).encode(),
    headers={"Content-Type": "application/json"},
)
with urllib.request.urlopen(req, timeout=15) as r:
    if r.status != 200:
        sys.exit("slack returned %s" % r.status)
'
}

if [[ "${1:-}" == "--test" ]]; then
  host="$(hostname -s)"
  if notify ":white_check_mark: Clio healthcheck test from ${host} — alerting works."; then
    echo "test alert sent"
  else
    echo "test alert FAILED" >&2
    exit 1
  fi
  exit 0
fi

# --- 1. Did the scheduled auto-link pass actually run? ---------------------------
if [[ -f "$CLIO_AUTOLINK_LOG" ]]; then
  now="$(date +%s)"
  mtime="$(date -r "$CLIO_AUTOLINK_LOG" +%s 2>/dev/null || stat -c %Y "$CLIO_AUTOLINK_LOG" 2>/dev/null || true)"
  if [[ -z "$mtime" ]]; then
    problems+=("cannot read the auto-link log's mtime (date -r and stat -c both failed)")
  else
    age=$((now - mtime))
    if [[ "$age" -gt "$CLIO_AUTOLINK_MAX_AGE" ]]; then
      problems+=("auto-link has not run for $((age / 60)) minutes (limit $((CLIO_AUTOLINK_MAX_AGE / 60)))")
    fi
  fi
  # Require the success signal rather than grepping for failure words: a run can
  # fail without printing any of them (a missing binary, a whole batch skipped for
  # want of embeddings), and error lines scroll out of any fixed tail window. Every
  # healthy run ends with its summary line, so its absence at the end of the log IS
  # the fault signal. Requires the cron entry to run plain `clio auto-link` (not
  # --json) with stderr redirected into the log — see docs/operations/deployment.md.
  if ! tail -1 "$CLIO_AUTOLINK_LOG" | grep -q "Auto-link complete"; then
    problems+=("auto-link's last run did not finish cleanly; inspect $CLIO_AUTOLINK_LOG locally")
  fi
else
  problems+=("auto-link log missing at $CLIO_AUTOLINK_LOG — has it ever run?")
fi

# --- 2. Is the schedule still installed? -----------------------------------------
# Ansible manages part of this crontab. If a playbook rewrites the file, the Clio
# entry disappears and linking stops with no other symptom.
if command -v crontab >/dev/null 2>&1; then
  if ! crontab -l 2>/dev/null | grep -q "clio auto-link"; then
    problems+=("the clio auto-link cron entry is MISSING from the crontab")
  fi
fi

# --- 3. Is the database sound? ---------------------------------------------------
if [[ -r "$CLIO_DB_PATH" ]]; then
  db_report="$(python3 - "$CLIO_DB_PATH" <<'PY'
import sqlite3, sys
try:
    c = sqlite3.connect("file:%s?mode=ro" % sys.argv[1], uri=True)
    integrity = c.execute("pragma integrity_check").fetchone()[0]
    if integrity != "ok":
        print("integrity_check: %s" % integrity)
    live = c.execute("select count(*) from memories where archived_at is null").fetchone()[0]
    links = c.execute("select count(*) from memory_links").fetchone()[0]
    if live == 0:
        print("no live memories")
    if links == 0:
        print("no links at all — auto-linking may have wiped the graph")
except Exception as exc:
    print("database unreadable: %s" % exc)
PY
)"
  [[ -z "$db_report" ]] || while IFS= read -r line; do problems+=("$line"); done <<<"$db_report"
else
  problems+=("database not readable at $CLIO_DB_PATH")
fi

# --- 4. Is the capture queue draining? (only where a spool exists) ---------------
if [[ -d "$CLIO_SPOOL" ]]; then
  dead="$(find "$CLIO_SPOOL/dead" -name '*.json' 2>/dev/null | wc -l | tr -d ' ')"
  pending="$(find "$CLIO_SPOOL/pending" -name '*.json' 2>/dev/null | wc -l | tr -d ' ')"
  [[ "${dead:-0}" -eq 0 ]] || problems+=("$dead capture job(s) dead-lettered — session knowledge is being lost")
  [[ "${pending:-0}" -lt 25 ]] || problems+=("$pending capture jobs pending — the queue is not draining")
fi

# --- Report, alerting only when the state changes --------------------------------
host="$(hostname -s)"
if [[ "${#problems[@]}" -eq 0 ]]; then
  if [[ -s "$CLIO_STATE_FILE" ]]; then
    # Clear the state only when the recovery message was delivered, so a message
    # lost to a transient webhook failure is retried on the next healthy run.
    if notify ":white_check_mark: Clio on ${host}: background work healthy again."; then
      : > "$CLIO_STATE_FILE"
    else
      echo "recovery notice FAILED to send; will retry next run" >&2
    fi
  fi
  echo "healthy"
  exit 0
fi

signature="$(printf '%s\n' "${problems[@]}" | sort | cksum | cut -d' ' -f1)"
previous="$(cat "$CLIO_STATE_FILE" 2>/dev/null || true)"

printf 'UNHEALTHY:\n'
printf '  - %s\n' "${problems[@]}"

if [[ "$signature" != "$previous" ]]; then
  message=":rotating_light: *Clio background work is unhealthy* on \`${host}\`"$'\n'
  for p in "${problems[@]}"; do message+="• ${p}"$'\n'; done
  message+="_Silent failure is the failure mode here — auto-linking was dead for a week in July before anyone noticed._"
  # Record the state only when the alert was actually delivered: an undelivered
  # alert recorded as sent would match every later run and never be heard.
  if notify "$message"; then
    printf '%s' "$signature" > "$CLIO_STATE_FILE"
  else
    echo "alert delivery FAILED; will retry next run" >&2
  fi
else
  echo "(already alerted for this state; not repeating)"
fi
exit 1
