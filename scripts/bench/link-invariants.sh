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

remember() { # remember <namespace> <title> <content> [kind]
  # --local keeps this off any configured shared route, so the test can never write
  # into live data even if settings say otherwise.
  if ! "$CLIO" --db-path "$DB" --local remember --content "$3" \
      --namespace "$1" --title "$2" --kind "${4:-note}" --json >"$WORK/last" 2>&1; then
    printf 'FIXTURE ERROR creating %s:\n' "$2"
    sed 's/^/    /' "$WORK/last"
    exit 1
  fi
}

total_degree_over() { # total_degree_over <cap> -> memories whose total auto-link degree exceeds cap
  q "select count(*) from memories m where (
       select count(*) from memory_links l
       where l.relationship = ?1
         and (l.from_memory_id = m.id or l.to_memory_id = m.id)
     ) > ?2" "$AUTO" "$1"
}

max_total_degree() {
  q "select coalesce(max((
       select count(*) from memory_links l
       where l.relationship = ?1
         and (l.from_memory_id = m.id or l.to_memory_id = m.id)
     )), 0) from memories m" "$AUTO"
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

# Cluster: paraphrases of one idea, which should link to each other. SEVEN members
# against a cap of five: an uncapped run would give every member total degree six,
# so the cap invariants below actually bind. A cluster smaller than the cap passes
# them with the cap logic deleted, which is exactly what this suite once did.
remember "project:alpha" "pool-1" "Database connection pooling was enabled to stop the API exhausting Postgres connections under load."
remember "project:alpha" "pool-2" "We turned on connection pooling for the database because the API was running Postgres out of available connections."
remember "project:alpha" "pool-3" "Postgres kept running out of connections, so pooling was introduced in front of the database for the API."
remember "project:alpha" "pool-4" "Connection pooling now fronts Postgres; without it the API exhausted the connection limit during traffic spikes."
remember "project:alpha" "pool-5" "To avoid exhausting Postgres connection limits from the API, database connection pooling was switched on."
remember "project:alpha" "pool-6" "The API was draining the Postgres connection limit, so database connection pooling was brought in."
remember "project:alpha" "pool-7" "Pooling database connections stopped the API running Postgres out of connections when traffic spiked."

# Control: unrelated subject, same namespace. Should attract nothing.
remember "project:alpha" "unrelated" "The office coffee machine needs descaling every fortnight or it starts leaking onto the counter."

# Control: near-duplicate in a DIFFERENT namespace. Must never link across.
remember "project:beta" "cross-ns" "Database connection pooling was enabled to stop the API exhausting Postgres connections under load."

# Control: near-duplicate that is archived. Must never be a link target.
remember "project:alpha" "archived-dupe" "Enabling database connection pooling prevented the API from exhausting Postgres connections."
ARCHIVED_ID="$(q "select id from memories where title = ?" "archived-dupe")"
"$CLIO" --db-path "$DB" --local archive "$ARCHIVED_ID" >/dev/null 2>&1

# Control: near-duplicate with an excluded kind. The settings exclude receipts, so
# this must link in neither direction — the fixture that proves exclude_kinds does
# anything at all.
remember "project:alpha" "receipt-dupe" "Session receipt: database connection pooling was enabled to stop the API exhausting Postgres connections." "receipt"

FIXTURES="$(q "select count(*) from memories")"
ARCHIVED="$(q "select count(*) from memories where archived_at is not null")"
EMBEDDED="$(q "select count(*) from memory_embeddings")"
RECEIPTS="$(q "select count(*) from memories where kind = 'receipt'")"
echo "Fixtures: $FIXTURES memories, $ARCHIVED archived, $RECEIPTS receipt, $EMBEDDED embedded"

# Guard: with no fixtures every "no unwanted link" assertion passes trivially and the
# suite reports success while proving nothing. Refuse to continue.
if [[ "$FIXTURES" -ne 11 || "$ARCHIVED" -ne 1 || "$RECEIPTS" -ne 1 || "$EMBEDDED" -eq 0 ]]; then
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

check "the excluded kind links in neither direction" 0 \
  "$(q "select count(*) from memory_links l join memories m
        on m.id in (l.from_memory_id, l.to_memory_id)
        where m.kind = 'receipt' and l.relationship = ?" "$AUTO")"

check "no memory exceeds the total-degree cap of 5" 0 "$(total_degree_over 5)"

# Without this, the cap assertions pass vacuously on any fixture too small or too
# loosely connected for the cap to matter — delete the cap logic and nothing fails.
check "the cap binds in this fixture (max total degree is exactly 5)" 5 "$(max_total_degree)"

check "every clustered memory is connected (either direction)" 7 \
  "$(q "select count(*) from memories m where m.title like ?
        and exists (select 1 from memory_links l where l.relationship = ?
                    and (l.from_memory_id = m.id or l.to_memory_id = m.id))" "pool-%" "$AUTO")"

check "a second run is idempotent" 0 \
  "$("$CLIO" --db-path "$DB" --local auto-link --json 2>/dev/null |
     python3 -c 'import json,sys; print(json.load(sys.stdin)["links_created"])')"

# The cap is cumulative across runs, not a fresh allowance per run. Reproduce the
# regression that once let capped memories keep gaining links on every timed pass:
# add a new similar memory and re-run — nothing may go over cap, and members
# already at the cap must not move.
remember "project:alpha" "pool-8" "Connection pooling for the database was the fix for the API exhausting Postgres connections."
"$CLIO" --db-path "$DB" --local auto-link >/dev/null 2>&1

check "a later run against new similar memories breaches no cap" 0 "$(total_degree_over 5)"
check "the cap still binds after the later run" 5 "$(max_total_degree)"

# At-cap candidates must not consume the newcomer's suggestion slots: pool-8 has
# under-cap neighbours (the cluster cannot saturate all seven members), so it must
# connect to at least one of them rather than end up isolated.
check "a newcomer to a saturated cluster still connects" yes \
  "$(if [[ "$(q "select count(*) from memory_links l join memories m
                 on m.id in (l.from_memory_id, l.to_memory_id)
                 where m.title = 'pool-8' and l.relationship = ?" "$AUTO")" -gt 0 ]]; then
       echo yes; else echo no; fi)"

echo
echo "Degree per clustered memory (outgoing / incoming / total):"
echo "  max_links_per_memory bounds TOTAL degree — both directions — because recall"
echo "  walks edges both ways, so total degree is what governs how much context a"
echo "  memory drags into a brief. A link is created only while both endpoints are"
echo "  below the cap."
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
