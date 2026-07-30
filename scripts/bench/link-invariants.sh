#!/usr/bin/env bash
# Prove auto-linking obeys its design rules, using controlled synthetic fixtures.
#
# Runs entirely in a throwaway database and never touches real data. Each fixture
# probes one invariant, so a failure names the rule that broke.
#
# Usage: scripts/bench/link-invariants.sh
#
# Environment:
#   CLIO                path to the clio binary (default ~/.cargo/bin/clio)
#   FASTEMBED_CACHE_PATH  where the local embedding model is cached
set -uo pipefail

CLIO="${CLIO:-$HOME/.cargo/bin/clio}"
WORK="$(mktemp -d)"
DB="$WORK/clio.db"
AUTO="auto:relates_to"
pass=0
fail=0

trap 'rm -rf "$WORK"' EXIT

# Default to the macOS model cache; override for other platforms.
export FASTEMBED_CACHE_PATH="${FASTEMBED_CACHE_PATH:-$HOME/Library/Application Support/clio/models}"

check() { # check <description> <expected> <actual>
  if [[ "$2" == "$3" ]]; then
    printf '  PASS  %s (%s)\n' "$1" "$3"
    pass=$((pass + 1))
  else
    printf '  FAIL  %s — expected %s, got %s\n' "$1" "$2" "$3"
    fail=$((fail + 1))
  fi
}

q() { python3 -c "
import sqlite3, sys
print(sqlite3.connect('$DB').execute(sys.argv[1], sys.argv[2:]).fetchone()[0])
" "$@"; }

remember() { # remember <namespace> <title> <content>
  # --local keeps this off any configured shared route, so the test can never write
  # into live data even if settings say otherwise.
  if ! "$CLIO" --db-path "$DB" --local remember --content "$3" \
      --namespace "$1" --title "$2" --kind note --json >"$WORK/last" 2>&1; then
    printf 'FIXTURE ERROR creating %s:\n' "$2"
    sed 's/^/    /' "$WORK/last"
    exit 1
  fi
}

# Production threshold and cap, so this tests the real configuration.
cat > "$WORK/clio-settings.json" <<'EOF'
{"embeddings": {"provider": "local", "model": "all-MiniLM-L6-v2"},
 "auto_embed": true,
 "capture": {"enabled": false},
 "daemon": {"enabled": false, "auto_link": {"enabled": true, "threshold": 0.6,
   "interval_secs": 3600, "max_links_per_memory": 5, "batch_size": 200,
   "exclude_kinds": ["receipt"]}}}
EOF

"$CLIO" --db-path "$DB" --local init >/dev/null 2>&1

# Cluster: paraphrases of one idea, which should link to each other.
remember "project:alpha" "pool-1" "Database connection pooling was enabled to stop the API exhausting Postgres connections under load."
remember "project:alpha" "pool-2" "We turned on connection pooling for the database because the API was running Postgres out of available connections."
remember "project:alpha" "pool-3" "Postgres kept running out of connections, so pooling was introduced in front of the database for the API."
remember "project:alpha" "pool-4" "Connection pooling now fronts Postgres; without it the API exhausted the connection limit during traffic spikes."
remember "project:alpha" "pool-5" "To avoid exhausting Postgres connection limits from the API, database connection pooling was switched on."

# Control: unrelated subject, same namespace. Should attract nothing.
remember "project:alpha" "unrelated" "The office coffee machine needs descaling every fortnight or it starts leaking onto the counter."

# Control: near-duplicate in a DIFFERENT namespace. Must never link across.
remember "project:beta" "cross-ns" "Database connection pooling was enabled to stop the API exhausting Postgres connections under load."

# Control: near-duplicate that is archived. Must never be a link target.
remember "project:alpha" "archived-dupe" "Enabling database connection pooling prevented the API from exhausting Postgres connections."
ARCHIVED_ID="$(q "select id from memories where title = ?" "archived-dupe")"
"$CLIO" --db-path "$DB" --local archive "$ARCHIVED_ID" >/dev/null 2>&1

FIXTURES="$(q "select count(*) from memories")"
ARCHIVED="$(q "select count(*) from memories where archived_at is not null")"
EMBEDDED="$(q "select count(*) from memory_embeddings")"
echo "Fixtures: $FIXTURES memories, $ARCHIVED archived, $EMBEDDED embedded"

# Guard: with no fixtures every "no unwanted link" assertion passes trivially and the
# suite reports success while proving nothing. Refuse to continue.
if [[ "$FIXTURES" -ne 8 || "$ARCHIVED" -ne 1 || "$EMBEDDED" -eq 0 ]]; then
  echo "ABORT: fixtures did not build, so the invariants would pass vacuously."
  exit 1
fi
echo

echo "Auto-link pass:"
"$CLIO" --db-path "$DB" --local auto-link 2>&1 | sed 's/^/  /'
echo

echo "Invariants:"

check "similar memories in one namespace get linked" "yes" \
  "$(if [[ "$(q "select count(*) from memory_links where relationship = ?" "$AUTO")" -gt 0 ]]; then echo yes; else echo no; fi)"

check "unrelated memory gets no links" 0 \
  "$(q "select count(*) from memory_links l join memories m on m.id = l.from_memory_id
        where m.title = ? and l.relationship = ?" "unrelated" "$AUTO")"

check "no links cross a namespace boundary" 0 \
  "$(q "select count(*) from memory_links l
        join memories a on a.id = l.from_memory_id
        join memories b on b.id = l.to_memory_id
        where l.relationship = ? and a.namespace <> b.namespace" "$AUTO")"

check "the cross-namespace duplicate is isolated" 0 \
  "$(q "select count(*) from memory_links l join memories m on m.id = l.from_memory_id
        where m.title = ? and l.relationship = ?" "cross-ns" "$AUTO")"

check "archived memories are never link targets" 0 \
  "$(q "select count(*) from memory_links l join memories b on b.id = l.to_memory_id
        where l.relationship = ? and b.archived_at is not null" "$AUTO")"

check "max_links_per_memory is respected" 0 \
  "$(q "select count(*) from (select from_memory_id, count(*) n from memory_links
        where relationship = ? group by from_memory_id having n > 5)" "$AUTO")"

# Degree must be counted in both directions. Memories are processed oldest-first, so
# the newest member of a cluster typically has zero OUTGOING links — everything
# already linked to it, and suggest_links excludes existing pairs. Recall traverses
# edges both ways, so an incoming-only memory is fully reachable.
check "every clustered memory is connected (either direction)" 5 \
  "$(q "select count(*) from memories m where m.title like ?
        and exists (select 1 from memory_links l where l.relationship = ?
                    and (l.from_memory_id = m.id or l.to_memory_id = m.id))" "pool-%" "$AUTO")"

check "a second run is idempotent" 0 \
  "$("$CLIO" --db-path "$DB" --local auto-link --json 2>/dev/null |
     python3 -c 'import json,sys; print(json.load(sys.stdin)["links_created"])')"

echo
echo "Degree per clustered memory (outgoing / incoming / total):"
echo "  The cap bounds OUTGOING links only. Because recall walks edges both ways,"
echo "  total degree is what governs how much context a recall pulls in, and it is"
echo "  not bounded by max_links_per_memory."
python3 - "$DB" "$AUTO" <<'PY'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
auto = sys.argv[2]
for title, in conn.execute(
    "select title from memories where title like 'pool-%' order by title"
):
    mid, = conn.execute("select id from memories where title = ?", (title,)).fetchone()
    out, = conn.execute(
        "select count(*) from memory_links where from_memory_id = ? and relationship = ?",
        (mid, auto)).fetchone()
    inc, = conn.execute(
        "select count(*) from memory_links where to_memory_id = ? and relationship = ?",
        (mid, auto)).fetchone()
    print("    %-8s %d / %d / %d" % (title, out, inc, out + inc))
PY

echo
echo "RESULT: $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
