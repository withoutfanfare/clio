#!/usr/bin/env bash
# Self-test for clio-healthcheck.sh, using a throwaway environment: a fake
# database, a fake crontab on PATH, and a local webhook that can be told to
# accept or refuse. Exists because the healthcheck's own failure mode is the one
# it polices — a monitor that breaks silently — so its alert-delivery state
# machine needs a check that fails if the logic regresses.
#
# Usage: scripts/clio-healthcheck-selftest.sh
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/clio-healthcheck.sh"
WORK="$(mktemp -d)"
pass=0
fail=0

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" 2>/dev/null
    wait "$SERVER_PID" 2>/dev/null
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

ok() { # ok <description> <condition-exit-status>
  if [[ "$2" -eq 0 ]]; then
    printf '  PASS  %s\n' "$1"; pass=$((pass + 1))
  else
    printf '  FAIL  %s\n' "$1"; fail=$((fail + 1))
  fi
}

# --- Fixtures -------------------------------------------------------------------

# A minimal database with the two tables the check queries, one live memory and
# one link, so the database section reports nothing.
python3 - "$WORK/memory.db" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
c.execute("create table memories (id text primary key, archived_at text)")
c.execute("create table memory_links (id integer primary key)")
c.execute("insert into memories values ('m1', null)")
c.execute("insert into memory_links values (1)")
c.commit()
PY

# A fake crontab so the schedule check passes off-Atlas.
mkdir -p "$WORK/bin"
printf '#!/bin/sh\necho "17 * * * * clio auto-link"\n' > "$WORK/bin/crontab"
chmod +x "$WORK/bin/crontab"

# A webhook that accepts unless a refuse-file exists. 200 on accept, 500 on refuse.
# Request bodies are retained in the throwaway directory so alert redaction is
# exercised at the actual HTTP boundary.
python3 - "$WORK/refuse" "$WORK/requests" "$WORK/port" <<'PY' &
import http.server, sys
refuse_marker = sys.argv[1]
requests_path = sys.argv[2]
port_path = sys.argv[3]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        import os
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with open(requests_path, "ab") as requests:
            requests.write(body + b"\n")
        code = 500 if os.path.exists(refuse_marker) else 200
        self.send_response(code)
        self.end_headers()
    def log_message(self, *a):
        pass
server = http.server.HTTPServer(("127.0.0.1", 0), H)
with open(port_path, "w") as port_file:
    print(server.server_port, file=port_file, flush=True)
server.serve_forever()
PY
SERVER_PID=$!
for _ in {1..50}; do
  [[ -s "$WORK/port" ]] && break
  sleep 0.1
done
PORT="$(cat "$WORK/port")"

healthy_log() {
  printf 'Auto-link complete: 3 memory(ies) over 1 pass(es), 2 link(s) created (threshold 0.6, cap 5 per memory).\n' \
    > "$WORK/auto-link.log"
}

run_check() { # run_check -> captures stdout+stderr in $OUT, exit status in $STATUS
  OUT="$(PATH="$WORK/bin:$PATH" \
    CLIO_DB_PATH="$WORK/memory.db" \
    CLIO_AUTOLINK_LOG="$WORK/auto-link.log" \
    CLIO_STATE_FILE="$WORK/state" \
    CLIO_ALERT_ENV_FILE="$WORK/absent.env" \
    CLIO_SLACK_WEBHOOK="http://127.0.0.1:$PORT/hook" \
    CLIO_SPOOL="$WORK/no-spool" \
    bash "$CHECK" 2>&1)"
  STATUS=$?
}

echo "Scenarios:"

# --- 1. Healthy run: exits 0, writes no state -----------------------------------
healthy_log
run_check
ok "healthy log and database exit 0" "$STATUS"
[[ ! -s "$WORK/state" ]]; ok "healthy run leaves no alert state" $?

# --- 2. Fault with the webhook DOWN: state must NOT be recorded -----------------
rm -f "$WORK/auto-link.log"   # fault: log missing
touch "$WORK/refuse"          # webhook refuses
run_check
[[ "$STATUS" -ne 0 ]]; ok "fault exits non-zero" $?
grep -q "alert delivery FAILED" <<<"$OUT"; ok "failed delivery is reported" $?
[[ ! -s "$WORK/state" ]]; ok "undelivered alert is NOT recorded as sent" $?

# --- 3. Same fault again, webhook BACK UP: alert must still be sent -------------
rm -f "$WORK/refuse"
run_check
if grep -q "already alerted" <<<"$OUT"; then false; else true; fi
ok "alert retries after a failed delivery" $?
[[ -s "$WORK/state" ]]; ok "delivered alert records its state" $?

# --- 4. Same fault, third run: now deduplicated ---------------------------------
run_check
grep -q "already alerted" <<<"$OUT"; ok "repeated fault does not re-alert" $?

# --- 5. Recovery with webhook DOWN: state must survive for a retry --------------
healthy_log
touch "$WORK/refuse"
run_check
ok "recovery run exits 0 even when the notice fails" "$STATUS"
[[ -s "$WORK/state" ]]; ok "failed recovery notice keeps state for retry" $?

# --- 6. Recovery with webhook UP: state clears ----------------------------------
rm -f "$WORK/refuse"
run_check
ok "recovery run exits 0" "$STATUS"
[[ ! -s "$WORK/state" ]]; ok "delivered recovery notice clears state" $?

# --- 7. A run that died mid-way is caught without leaking its log to Slack -------
printf 'auto-link: provider returned private diagnostic detail\n' > "$WORK/auto-link.log"
run_check
grep -q "did not finish cleanly" <<<"$OUT"; ok "missing summary line is a fault" $?
if grep -q "private diagnostic detail" "$WORK/requests"; then false; else true; fi
ok "auto-link log content is not sent to Slack" $?
grep -Fq "$WORK/auto-link.log" "$WORK/requests"
ok "the alert points the operator to the local log" $?

echo
echo "RESULT: $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
