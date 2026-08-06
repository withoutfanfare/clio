#!/usr/bin/env bash
# Exercise recall-eval.py against disposable fixtures and a fake brief command.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

python3 - "$WORK/source.db" <<'PY'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
conn.execute("create table memories (id text primary key, namespace text, kind text, title text, archived_at text)")
conn.execute("create table memory_tags (memory_id text, tag text)")
conn.execute("create table eval_state (calls integer not null)")
conn.execute("insert into eval_state values (0)")
for i in range(40):
    mid = "memory-%02d" % i
    conn.execute(
        "insert into memories values (?, 'project:test', 'fact', ?, null)",
        (mid, "A long enough benchmark title number %02d" % i),
    )
    conn.execute("insert into memory_tags values (?, 'shared-a')", (mid,))
    conn.execute("insert into memory_tags values (?, 'shared-b')", (mid,))
conn.commit()
PY
printf '{"scoring": {"decay_lambda": 0.0}}\n' > "$WORK/clio-settings.json"

printf '%s\n' '#!/usr/bin/env python3' \
  'import json, os, sqlite3, sys' \
  'if os.environ.get("FAKE_CLIO_MODE") == "fail":' \
  '    sys.exit("synthetic brief failure")' \
  'db = sys.argv[sys.argv.index("--db-path") + 1]' \
  'settings = os.path.join(os.path.dirname(db), "clio-settings.json")' \
  'if not os.path.exists(settings):' \
  '    sys.exit("snapshot is missing clio-settings.json")' \
  'conn = sqlite3.connect(db)' \
  'calls = conn.execute("select calls from eval_state").fetchone()[0]' \
  'conn.execute("update eval_state set calls = calls + 1")' \
  'conn.commit()' \
  'mid = "synthetic-a" if calls == 0 else "synthetic-b"' \
  'print(json.dumps({"sections": [{"items": [{"id": mid}]}]}))' > "$WORK/fake-clio"
chmod +x "$WORK/fake-clio"

set +e
LIVE_OUT="$(CLIO="$WORK/fake-clio" CLIO_DB_PATH="$WORK/source.db" \
  python3 "$HERE/recall-eval.py" "$WORK/source.db" 1 2>&1)"
LIVE_STATUS=$?
set -e
if [[ "$LIVE_STATUS" -eq 0 || "$LIVE_OUT" != *"Refusing to run against the live database"* ]]; then
  printf 'FAIL: CLIO_DB_PATH was not recognised as live\n%s\n' "$LIVE_OUT" >&2
  exit 1
fi

cp "$WORK/source.db" "$WORK/failure.db"
set +e
FAIL_OUT="$(env -u CLIO_DB_PATH CLIO="$WORK/fake-clio" FAKE_CLIO_MODE=fail \
  python3 "$HERE/recall-eval.py" "$WORK/failure.db" 1 2>&1)"
set -e
if [[ "$FAIL_OUT" != *"1 failed query pairs"* ]]; then
  printf 'FAIL: failed pairs were mislabelled\n%s\n' "$FAIL_OUT" >&2
  exit 1
fi

cp "$WORK/source.db" "$WORK/pair.db"
PAIR_OUT="$(env -u CLIO_DB_PATH CLIO="$WORK/fake-clio" \
  python3 "$HERE/recall-eval.py" "$WORK/pair.db" 1 2>&1)"
if [[ "$PAIR_OUT" != *"linked recall is contributing nothing"* ]]; then
  printf 'FAIL: paired arms did not start from identical database state\n%s\n' "$PAIR_OUT" >&2
  exit 1
fi
calls="$(python3 - "$WORK/pair.db" <<'PY'
import sqlite3, sys
print(sqlite3.connect(sys.argv[1]).execute("select calls from eval_state").fetchone()[0])
PY
)"
if [[ "$calls" -ne 0 ]]; then
  printf 'FAIL: evaluation mutated the source copy (%s calls recorded)\n' "$calls" >&2
  exit 1
fi

echo "PASS: live-path guard, pair labelling and paired snapshots"
