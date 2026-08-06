#!/usr/bin/env bash
# Exercise dial-in.sh's failure boundary with a disposable database and a fake
# CLI whose auto-link succeeds but whose recall command fails.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

python3 - "$WORK/source.db" <<'PY'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
conn.execute("create table memories (id text primary key, namespace text, kind text, title text, archived_at text)")
conn.execute("create table memory_tags (memory_id text, tag text)")
conn.execute("create table memory_links (from_memory_id text, to_memory_id text, relationship text)")
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

printf '%s\n' '#!/usr/bin/env bash' \
  'db="$2"' \
  'if [[ " $* " == *" auto-link "* ]]; then' \
  '  python3 - "$db" <<'"'"'PY'"'"'' \
  'import sqlite3, sys' \
  'conn = sqlite3.connect(sys.argv[1])' \
  'conn.execute("insert into memory_links values ('"'"'memory-00'"'"', '"'"'memory-01'"'"', '"'"'auto:relates_to'"'"')")' \
  'conn.commit()' \
  'PY' \
  '  exit 0' \
  'fi' \
  'echo "synthetic recall failure" >&2' \
  'exit 42' > "$WORK/fake-clio"
chmod +x "$WORK/fake-clio"

OUT="$(CLIO="$WORK/fake-clio" bash "$HERE/dial-in.sh" "$WORK/source.db" 1 2>&1)"

failures="$(grep -c 'ABORT — recall evaluation failed:' <<<"$OUT" || true)"
if [[ "$failures" -ne 7 ]]; then
  printf 'FAIL: expected all 7 trials to report the recall failure; got %s\n%s\n' "$failures" "$OUT" >&2
  exit 1
fi
if grep -q 'prec=' <<<"$OUT"; then
  printf 'FAIL: a failed evaluation produced an apparent metrics row\n%s\n' "$OUT" >&2
  exit 1
fi

echo "PASS: failed recall evaluations abort every trial without printing metrics"
