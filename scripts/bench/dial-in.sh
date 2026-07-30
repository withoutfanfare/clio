#!/usr/bin/env bash
# Tune auto-linking against retrieval quality rather than structural proxies.
#
# Relinks a copy of a database at each setting, then scores what briefs actually
# return. One variable moves at a time, because changing threshold and cap together
# leaves the result unattributable.
#
# Usage:
#   scripts/bench/dial-in.sh <source-db> [n-queries]
#
# The source database is never modified — each trial works on its own copy.
set -uo pipefail

BASE="$(cd "$(dirname "$0")" && pwd)"
SRC="${1:?usage: dial-in.sh <source-db> [n-queries]}"
N="${2:-40}"
CLIO="${CLIO:-$HOME/.cargo/bin/clio}"

[[ -r "$SRC" ]] || { echo "cannot read $SRC" >&2; exit 1; }

trial() { # trial <label> <threshold> <cap>
  local label="$1" threshold="$2" cap="$3" work links
  work="$(mktemp -d)"
  cp "$SRC" "$work/memory.db"

  cat > "$work/clio-settings.json" <<EOF
{"embeddings": {"provider": "local", "model": "all-MiniLM-L6-v2"},
 "auto_embed": true,
 "capture": {"enabled": false},
 "daemon": {"enabled": false, "auto_link": {"enabled": true,
   "threshold": $threshold, "interval_secs": 3600,
   "max_links_per_memory": $cap, "batch_size": 200,
   "exclude_kinds": ["receipt"]}}}
EOF

  # Clear inferred links, then ASSERT the clear happened. A silently skipped delete
  # makes every trial return the first trial's numbers, which looks plausible and
  # proves nothing — this has happened.
  if ! python3 - "$work/memory.db" <<'PY'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
conn.execute("delete from memory_links where relationship = ?", ("auto:relates_to",))
conn.commit()
left = conn.execute(
    "select count(*) from memory_links where relationship = ?", ("auto:relates_to",)
).fetchone()[0]
assert left == 0, "clear failed: %d links remained" % left
PY
  then
    echo "$label: ABORT — could not clear links"
    rm -rf "$work"
    return
  fi

  "$CLIO" --db-path "$work/memory.db" --local auto-link --json >/dev/null 2>&1
  links="$(python3 -c "
import sqlite3
print(sqlite3.connect('$work/memory.db').execute(
    'select count(*) from memory_links where relationship = ?',
    ('auto:relates_to',)).fetchone()[0])")"

  printf '%-22s links=%-6s ' "$label" "$links"
  CLIO="$CLIO" python3 "$BASE/recall-eval.py" "$work/memory.db" "$N" 2>/dev/null |
    awk '/^with links/       { precision = $4; mrr = $5 }
         /per brief\)/       { gsub("[()]", "", $(NF-2)); per = $(NF-2) }
         /chance baseline —/ { gsub("%", "", $1); added = $1; lift = $NF }
         END { printf "prec=%s MRR=%s added=%s/brief addPrec=%s%% lift=%s\n",
                      precision, mrr, per, added, lift }'
  rm -rf "$work"
}

echo "=== Threshold sweep at cap 5 ==="
for t in 0.55 0.60 0.65 0.70; do trial "thresh $t / cap 5" "$t" 5; done

echo
echo "=== Cap sweep at threshold 0.60 ==="
for c in 3 5 8; do trial "thresh 0.60 / cap $c" 0.60 "$c"; done
