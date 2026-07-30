#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

CLIO_REPO_DIR="${CLIO_REPO_DIR:-$HOME/src/clio}"
CLIO_DB_PATH="${CLIO_DB_PATH:-$HOME/.local/share/clio/memory.db}"
CLIO_SETTINGS_PATH="${CLIO_SETTINGS_PATH:-$(dirname "$CLIO_DB_PATH")/clio-settings.json}"
CLIO_INSTALL_ROOT="${CLIO_INSTALL_ROOT:-$HOME/.local/lib/clio}"
CLIO_REMOTE_REF="${CLIO_REMOTE_REF:-origin/develop}"
CLIO_BIN_DIR="$HOME/.local/bin"
CLIO_ORT_DYLIB="${CLIO_ORT_DYLIB:-$CLIO_BIN_DIR/libonnxruntime.so}"
CLIO_TEMP_BACKUP=""
CLIO_TEMP_LINK_DIR=""
CLIO_TEMP_PROBE=""
CLIO_LAST_BACKUP=""
CLIO_EXPECTED_MIGRATIONS=""
CLIO_PENDING_MIGRATIONS=false
CLIO_MCP_GATED=false
CLIO_MIGRATION_STARTED=false
CLIO_RELEASE_ACTIVATED=false
CLIO_GATE_CANDIDATE="$CLIO_INSTALL_ROOT/gated-candidate"

usage() {
  cat <<'EOF'
Usage:
  scripts/atlas-release.sh check <full-sha>
  scripts/atlas-release.sh deploy <full-sha>
  scripts/atlas-release.sh status
  scripts/atlas-release.sh rollback <full-sha>
EOF
}

cleanup() {
  local exit_status=$? path
  set +e
  if [[ "$CLIO_MCP_GATED" == true ]]; then
    if [[ "$CLIO_MIGRATION_STARTED" == true && "$CLIO_RELEASE_ACTIVATED" != true ]]; then
      printf 'ERROR: Migration may have started but activation did not finish; new MCP sessions remain gated. Re-run the deployment to roll forward.\n' >&2
    else
      ungate_mcp || printf 'ERROR: Could not restore the stable MCP link.\n' >&2
    fi
  fi
  if [[ -n "$CLIO_TEMP_BACKUP" && -d "$CLIO_TEMP_BACKUP" ]]; then
    for path in memory.db memory.db-wal memory.db-shm memory.db-journal clio-settings.json; do
      [[ ! -e "$CLIO_TEMP_BACKUP/$path" ]] || unlink "$CLIO_TEMP_BACKUP/$path"
    done
    rmdir "$CLIO_TEMP_BACKUP" 2>/dev/null || true
  fi
  if [[ -n "$CLIO_TEMP_LINK_DIR" && -d "$CLIO_TEMP_LINK_DIR" ]]; then
    [[ ! -L "$CLIO_TEMP_LINK_DIR/clio" ]] || unlink "$CLIO_TEMP_LINK_DIR/clio"
    [[ ! -L "$CLIO_TEMP_LINK_DIR/clio-mcp" ]] || unlink "$CLIO_TEMP_LINK_DIR/clio-mcp"
    rmdir "$CLIO_TEMP_LINK_DIR" 2>/dev/null || true
  fi
  if [[ -n "$CLIO_TEMP_PROBE" && -d "$CLIO_TEMP_PROBE" ]]; then
    for path in memory.db memory.db-wal memory.db-shm memory.db-journal clio-settings.json; do
      [[ ! -e "$CLIO_TEMP_PROBE/$path" ]] || unlink "$CLIO_TEMP_PROBE/$path"
    done
    rmdir "$CLIO_TEMP_PROBE" 2>/dev/null || true
  fi
  return "$exit_status"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
require_tool() { command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"; }

normalise_sha() {
  [[ "${1:-}" =~ ^[[:xdigit:]]{40}$ ]] || fail "A full 40-character commit SHA is required."
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

require_linux() { [[ "$(uname -s)" == Linux ]] || fail "This script only runs on Linux."; }

check_checkout() {
  local sha="$1" head resolved remote
  [[ -d "$CLIO_REPO_DIR/.git" ]] || fail "Git checkout not found: $CLIO_REPO_DIR"
  [[ -z "$(git -C "$CLIO_REPO_DIR" status --porcelain --untracked-files=all)" ]] || fail "Checkout is not clean: $CLIO_REPO_DIR"
  resolved="$(git -C "$CLIO_REPO_DIR" rev-parse "$sha^{commit}" 2>/dev/null)" || fail "Commit is not available locally: $sha"
  [[ "$resolved" == "$sha" ]] || fail "Commit does not resolve exactly to $sha"
  remote="$(git -C "$CLIO_REPO_DIR" rev-parse "$CLIO_REMOTE_REF^{commit}" 2>/dev/null)" || fail "Approved remote ref is unavailable: $CLIO_REMOTE_REF"
  git -C "$CLIO_REPO_DIR" merge-base --is-ancestor "$sha" "$remote" || fail "$sha is not reachable from $CLIO_REMOTE_REF"
  head="$(git -C "$CLIO_REPO_DIR" rev-parse HEAD)"
  [[ "$head" == "$sha" ]] || fail "HEAD is $head, expected $sha"
}

preflight() {
  local sha="$1"
  require_linux
  for tool in git cargo python3 flock install mktemp pgrep sha256sum readlink; do require_tool "$tool"; done
  check_checkout "$sha"
  [[ -f "$CLIO_DB_PATH" ]] || fail "Database not found: $CLIO_DB_PATH"
  [[ -r "$CLIO_ORT_DYLIB" ]] || fail "ONNX Runtime library not found or unreadable: $CLIO_ORT_DYLIB"
  python3 -c 'import sqlite3' || fail "Python sqlite3 support is unavailable."
}

acquire_deploy_lock() {
  install -d -m 700 "$CLIO_INSTALL_ROOT"
  exec 9>"$CLIO_INSTALL_ROOT/deploy.lock"
  flock -n 9 || fail "Another Clio deployment is already running."
}

online_backup() {
  local sha="$1" backup_root stamp final
  backup_root="$CLIO_INSTALL_ROOT/backups"
  install -d -m 700 "$backup_root"
  CLIO_TEMP_BACKUP="$(mktemp -d "$backup_root/.backup.XXXXXX")"
  python3 - "$CLIO_DB_PATH" "$CLIO_TEMP_BACKUP/memory.db" <<'PY'
import sqlite3
import sys
from pathlib import Path

source_path = Path(sys.argv[1]).expanduser().resolve()
destination_path = Path(sys.argv[2])
source = sqlite3.connect(source_path.as_uri() + "?mode=ro", uri=True, timeout=30)
destination = sqlite3.connect(destination_path)
try:
    source.backup(destination)
    result = destination.execute("PRAGMA quick_check").fetchone()
    if result != ("ok",):
        raise SystemExit(f"backup quick_check failed: {result!r}")
finally:
    destination.close()
    source.close()
PY
  if [[ -f "$CLIO_SETTINGS_PATH" ]]; then
    install -m 600 "$CLIO_SETTINGS_PATH" "$CLIO_TEMP_BACKUP/clio-settings.json"
  else
    printf 'Settings file not present; database-only backup created.\n'
  fi
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  final="$backup_root/$stamp-$sha-$$"
  mv "$CLIO_TEMP_BACKUP" "$final"
  CLIO_TEMP_BACKUP=""
  CLIO_LAST_BACKUP="$final"
  printf 'Backup retained at %s\n' "$final"
}

install_release_binary() {
  local source="$1" destination="$2"
  if [[ -e "$destination" ]]; then
    cmp -s "$source" "$destination" || fail "Existing release file differs: $destination"
  else
    install -m 755 "$source" "$destination"
  fi
}

atomic_link() {
  local destination="$1" target="$2" parent name temporary
  parent="$(dirname "$destination")"
  name="$(basename "$destination")"
  temporary="$(mktemp -d "$parent/.clio-link.XXXXXX")"
  CLIO_TEMP_LINK_DIR="$temporary"
  ln -s "$target" "$temporary/$name"
  mv -Tf "$temporary/$name" "$destination"
  rmdir "$temporary"
  CLIO_TEMP_LINK_DIR=""
}

preserve_current_release() {
  local current="$CLIO_INSTALL_ROOT/current" legacy_dir source name
  if [[ -L "$current" ]]; then
    [[ -x "$(readlink -f "$current")/bin/clio" \
      && -x "$(readlink -f "$current")/bin/clio-mcp" \
      && -r "$(readlink -f "$current")/bin/libonnxruntime.so" ]] \
      || fail "The current release link is incomplete: $current"
    return
  fi
  [[ ! -e "$current" ]] || fail "$current exists but is not a symlink"

  if [[ ! -e "$CLIO_BIN_DIR/clio" && ! -e "$CLIO_BIN_DIR/clio-mcp" ]]; then
    return
  fi
  [[ -e "$CLIO_BIN_DIR/clio" && -e "$CLIO_BIN_DIR/clio-mcp" ]] || fail "Only one stable Clio binary exists; repair the installation before deploying"

  legacy_dir="$(mktemp -d "$CLIO_INSTALL_ROOT/releases/legacy-$(date -u +%Y%m%dT%H%M%SZ).XXXXXX")"
  install -d -m 700 "$legacy_dir/bin"
  for name in clio clio-mcp; do
    source="$(readlink -f "$CLIO_BIN_DIR/$name")"
    install -m 755 "$source" "$legacy_dir/bin/$name"
  done
  install -m 755 "$(readlink -f "$CLIO_ORT_DYLIB")" "$legacy_dir/bin/libonnxruntime.so"
  atomic_link "$current" "$legacy_dir"
  printf 'Legacy binaries retained at %s\n' "$legacy_dir"
}

ensure_stable_links() {
  local name expected
  for name in clio clio-mcp; do
    expected="$CLIO_INSTALL_ROOT/current/bin/$name"
    if [[ ! -L "$CLIO_BIN_DIR/$name" || "$(readlink "$CLIO_BIN_DIR/$name")" != "$expected" ]]; then
      atomic_link "$CLIO_BIN_DIR/$name" "$expected"
    fi
  done
}

gate_mcp() {
  local sha="$1" candidate_tmp="$CLIO_GATE_CANDIDATE.$$"
  printf '%s\n' "$sha" > "$candidate_tmp"
  chmod 600 "$candidate_tmp"
  mv -f "$candidate_tmp" "$CLIO_GATE_CANDIDATE"
  CLIO_MCP_GATED=true
  atomic_link "$CLIO_BIN_DIR/clio-mcp" /usr/bin/false
}

ungate_mcp() {
  atomic_link "$CLIO_BIN_DIR/clio-mcp" "$CLIO_INSTALL_ROOT/current/bin/clio-mcp"
  [[ ! -e "$CLIO_GATE_CANDIDATE" ]] || unlink "$CLIO_GATE_CANDIDATE"
  CLIO_MCP_GATED=false
}

mcp_is_gated() {
  [[ -L "$CLIO_BIN_DIR/clio-mcp" && "$(readlink "$CLIO_BIN_DIR/clio-mcp")" == /usr/bin/false ]]
}

# Ask lingering MCP servers to exit so clients reconnect against the release just
# activated. Swapping the `current` symlink does not affect a running process — it
# keeps the executable it already loaded — so without this a long-lived session
# serves superseded code indefinitely, and the mismatch is invisible from the
# client. Draining is a clean SIGTERM: MCP clients reconnect on demand.
#
# Set CLIO_KEEP_MCP_SESSIONS=1 to leave them alone, accepting that they run old code.
drain_mcp() {
  if [[ "${CLIO_KEEP_MCP_SESSIONS:-0}" == 1 ]]; then
    printf 'Existing MCP sessions left running (CLIO_KEEP_MCP_SESSIONS=1); they continue on superseded code.\n'
    return 0
  fi

  # Exact-name match, and signal only the PIDs captured here — never a pattern that
  # could match an unrelated process.
  local pids
  pids="$(pgrep -x clio-mcp 2>/dev/null || true)"
  if [[ -z "$pids" ]]; then
    printf 'No MCP sessions to drain.\n'
    return 0
  fi

  local count=0 pid
  for pid in $pids; do
    if kill -TERM "$pid" 2>/dev/null; then
      count=$((count + 1))
    fi
  done

  # Give them a moment to exit before reporting; do not escalate to SIGKILL, since a
  # server mid-write should be allowed to finish rather than risk a torn operation.
  local waited=0
  while [[ $waited -lt 10 ]] && pgrep -x clio-mcp >/dev/null 2>&1; do
    sleep 1
    waited=$((waited + 1))
  done

  local remaining
  remaining="$(pgrep -cx clio-mcp 2>/dev/null || true)"
  if [[ "${remaining:-0}" -gt 0 ]]; then
    printf 'Drained %s MCP session(s); %s still exiting — they will finish in their own time.\n' "$count" "$remaining"
  else
    printf 'Drained %s MCP session(s); clients will reconnect to the new release.\n' "$count"
  fi
}

detect_existing_gate() {
  local sha="$1" candidate=""
  if mcp_is_gated; then
    [[ -L "$CLIO_INSTALL_ROOT/current" ]] || fail "The MCP entry point is gated but no current release exists."
    [[ -f "$CLIO_GATE_CANDIDATE" ]] || fail "The MCP entry point is gated but its candidate SHA is missing; inspect the live migration state before continuing."
    candidate="$(<"$CLIO_GATE_CANDIDATE")"
    [[ "$candidate" == "$sha" ]] || fail "The MCP entry point is gated for $candidate, not $sha; resume the exact candidate or inspect the live migration state."
    CLIO_MCP_GATED=true
    CLIO_MIGRATION_STARTED=true
    printf 'Resuming the previously gated deployment for %s; the old MCP release will not be re-exposed.\n' "$sha"
  fi
}

acquire_migration_lock() {
  exec 8>"$CLIO_DB_PATH.maintenance.lock"
  flock -n 8
}

activate_release() {
  local release_dir="$1" current="$CLIO_INSTALL_ROOT/current"
  preserve_current_release
  if [[ ! -L "$current" ]]; then
    atomic_link "$current" "$release_dir"
  fi
  ensure_stable_links
  if [[ "$(readlink -f "$current")" != "$release_dir" ]]; then
    atomic_link "$current" "$release_dir"
  fi
}

migration_versions() {
  python3 - "$1" <<'PY'
import sqlite3
import sys
from pathlib import Path

path = Path(sys.argv[1]).expanduser().resolve()
connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
try:
    rows = connection.execute("SELECT version FROM schema_migrations ORDER BY version").fetchall()
    print("\n".join(row[0] for row in rows))
finally:
    connection.close()
PY
}

probe_migrations() {
  local candidate="$1" before after path
  CLIO_TEMP_PROBE="$(mktemp -d "$CLIO_INSTALL_ROOT/.migration-probe.XXXXXX")"
  cp "$CLIO_LAST_BACKUP/memory.db" "$CLIO_TEMP_PROBE/memory.db"
  if [[ -f "$CLIO_LAST_BACKUP/clio-settings.json" ]]; then
    cp "$CLIO_LAST_BACKUP/clio-settings.json" "$CLIO_TEMP_PROBE/clio-settings.json"
  fi
  before="$(migration_versions "$CLIO_TEMP_PROBE/memory.db")"
  "$candidate" --db-path "$CLIO_TEMP_PROBE/memory.db" --json stats >/dev/null
  "$candidate" --db-path "$CLIO_TEMP_PROBE/memory.db" --json recall \
    --query "deployment" --global --limit 1 >/dev/null
  "$candidate" --db-path "$CLIO_TEMP_PROBE/memory.db" --json search \
    "deployment smoke test" --global --limit 1 >/dev/null
  after="$(migration_versions "$CLIO_TEMP_PROBE/memory.db")"
  CLIO_EXPECTED_MIGRATIONS="$after"

  if [[ "$before" != "$after" ]]; then
    CLIO_PENDING_MIGRATIONS=true
    printf 'Migration probe found pending migrations.\n'
  fi

  for path in memory.db memory.db-wal memory.db-shm memory.db-journal clio-settings.json; do
    [[ ! -e "$CLIO_TEMP_PROBE/$path" ]] || unlink "$CLIO_TEMP_PROBE/$path"
  done
  rmdir "$CLIO_TEMP_PROBE"
  CLIO_TEMP_PROBE=""
}

deploy() {
  local sha="$1" release_dir running
  require_linux
  require_tool install
  require_tool flock
  acquire_deploy_lock
  preflight "$sha"
  detect_existing_gate "$sha"
  (cd "$CLIO_REPO_DIR" && cargo build --locked --release \
    --no-default-features --features local-embeddings-dynamic -p clio-cli --bin clio)
  (cd "$CLIO_REPO_DIR" && cargo build --locked --release \
    --no-default-features --features local-embeddings-dynamic -p clio-mcp --bin clio-mcp)
  check_checkout "$sha"
  release_dir="$CLIO_INSTALL_ROOT/releases/$sha"
  install -d -m 755 "$release_dir/bin" "$CLIO_BIN_DIR"
  install_release_binary "$CLIO_REPO_DIR/target/release/clio" "$release_dir/bin/clio"
  install_release_binary "$CLIO_REPO_DIR/target/release/clio-mcp" "$release_dir/bin/clio-mcp"
  install_release_binary "$(readlink -f "$CLIO_ORT_DYLIB")" "$release_dir/bin/libonnxruntime.so"

  # The backup must exist before this release opens, and therefore migrates, the live database.
  online_backup "$sha"
  probe_migrations "$release_dir/bin/clio"
  if [[ "$CLIO_PENDING_MIGRATIONS" == true ]]; then
    if [[ "$CLIO_MCP_GATED" != true ]]; then
      preserve_current_release
      if [[ -L "$CLIO_INSTALL_ROOT/current" ]]; then
        ensure_stable_links
        gate_mcp "$sha"
      fi
    fi
    running="$(pgrep -cx clio-mcp 2>/dev/null || true)"
    if ! acquire_migration_lock; then
      if [[ "${CLIO_ALLOW_LIVE_MIGRATION:-0}" != 1 ]]; then
        fail "An MCP session holds the database maintenance lock; disconnect it or set CLIO_ALLOW_LIVE_MIGRATION=1 after confirming backwards compatibility"
      fi
      printf 'Database maintenance lock is held; live compatibility override accepted.\n'
    elif [[ "$running" -gt 0 && "${CLIO_ALLOW_LIVE_MIGRATION:-0}" != 1 ]]; then
      fail "$running pre-lock MCP sessions are active and this release adds a migration; disconnect them or set CLIO_ALLOW_LIVE_MIGRATION=1 after confirming backwards compatibility"
    fi
    if [[ "$running" -gt 0 ]]; then
      printf 'New MCP sessions are gated; live compatibility override accepted for %s existing sessions.\n' "$running"
    else
      printf 'New MCP sessions are gated and no existing MCP sessions remain.\n'
    fi
  fi
  [[ "$CLIO_PENDING_MIGRATIONS" != true ]] || CLIO_MIGRATION_STARTED=true
  "$release_dir/bin/clio" --db-path "$CLIO_DB_PATH" --json stats >/dev/null
  [[ "$(migration_versions "$CLIO_DB_PATH")" == "$CLIO_EXPECTED_MIGRATIONS" ]] || fail "live migration state does not match the verified probe"

  if [[ "$CLIO_MCP_GATED" == true ]]; then
    atomic_link "$CLIO_INSTALL_ROOT/current" "$release_dir"
    CLIO_RELEASE_ACTIVATED=true
    ungate_mcp
  else
    activate_release "$release_dir"
    CLIO_RELEASE_ACTIVATED=true
  fi
  drain_mcp
  printf 'Deployed %s.\n' "$sha"
}

print_binary_status() {
  local name="$1" stable="$CLIO_BIN_DIR/$1" resolved hash
  if [[ ! -e "$stable" ]]; then printf '%s: missing\n' "$name"; return; fi
  resolved="$(readlink -f "$stable")"
  hash="$(sha256sum "$resolved" | awk '{print $1}')"
  printf '%s: %s\n  sha256: %s\n' "$name" "$resolved" "$hash"
}

status() {
  local current="" proc_exe target clean running=0 old_or_deleted=0 deleted=0
  require_linux
  require_tool readlink
  require_tool sha256sum
  print_binary_status clio
  print_binary_status clio-mcp
  [[ ! -e "$CLIO_BIN_DIR/clio-mcp" ]] || current="$(readlink -f "$CLIO_BIN_DIR/clio-mcp")"
  for proc_exe in /proc/[0-9]*/exe; do
    target="$(readlink "$proc_exe" 2>/dev/null)" || continue
    clean="${target% (deleted)}"
    [[ "$clean" == */clio-mcp ]] || continue
    running=$((running + 1))
    if [[ "$target" == *' (deleted)' ]]; then deleted=$((deleted + 1)); fi
    if [[ -z "$current" || "$clean" != "$current" || "$target" == *' (deleted)' ]]; then
      old_or_deleted=$((old_or_deleted + 1))
    fi
  done
  printf 'clio-mcp processes: %d running, %d old/deleted (%d deleted)\n' "$running" "$old_or_deleted" "$deleted"
}

rollback() {
  local sha="$1" release_dir
  require_linux
  require_tool flock
  require_tool install
  acquire_deploy_lock
  mcp_is_gated && fail "MCP is migration-gated; complete a roll-forward deployment before rolling back binaries."
  release_dir="$CLIO_INSTALL_ROOT/releases/$sha"
  [[ -x "$release_dir/bin/clio" && -x "$release_dir/bin/clio-mcp" \
    && -r "$release_dir/bin/libonnxruntime.so" ]] || fail "Release is incomplete or missing: $release_dir"
  install -d -m 755 "$CLIO_BIN_DIR" "$CLIO_INSTALL_ROOT/releases"
  activate_release "$release_dir"
  drain_mcp
  printf 'Rolled binary links back to %s. The database was not restored.\n' "$sha"
}

case "${1:-}" in
  -h|--help|help|'') usage ;;
  check) [[ $# -eq 2 ]] || fail "Usage: $0 check <full-sha>"; sha="$(normalise_sha "$2")"; preflight "$sha"; printf 'Atlas checkout is ready for %s.\n' "$sha" ;;
  deploy) [[ $# -eq 2 ]] || fail "Usage: $0 deploy <full-sha>"; sha="$(normalise_sha "$2")"; deploy "$sha" ;;
  status) [[ $# -eq 1 ]] || fail "Usage: $0 status"; status ;;
  rollback) [[ $# -eq 2 ]] || fail "Usage: $0 rollback <full-sha>"; sha="$(normalise_sha "$2")"; rollback "$sha" ;;
  *) fail "Unknown command: $1" ;;
esac
