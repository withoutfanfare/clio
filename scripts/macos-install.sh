#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

CLIO_REPO_DIR="${CLIO_REPO_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
CLIO_BIN_DIR="${CLIO_BIN_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"
CLIO_APP_PATH="${CLIO_APP_PATH:-/Applications/Clio.app}"
CLIO_PLIST_PATH="${CLIO_PLIST_PATH:-$HOME/Library/LaunchAgents/com.clio.daemon.plist}"
CLIO_REMOTE_REF="${CLIO_REMOTE_REF:-origin/develop}"
CLIO_BACKUP_ROOT="$HOME/Library/Application Support/Clio Deploy Backups"
CLIO_TEMP_FILE=""
CLIO_MOVED_APP=""
CLIO_DAEMON_ROLLBACK_PENDING=false
CLIO_DAEMON_BACKUP=""
CLIO_DAEMON_DESTINATION=""
CLIO_DAEMON_DOMAIN=""
CLIO_DAEMON_DATABASE=""

usage() {
  cat <<'EOF'
Usage:
  scripts/macos-install.sh check <full-sha>
  scripts/macos-install.sh install <full-sha> [--with-daemon] [--with-app]
EOF
}

cleanup() {
  local exit_status=$? interrupted_app
  set +e
  [[ -z "$CLIO_TEMP_FILE" || ! -e "$CLIO_TEMP_FILE" ]] || unlink "$CLIO_TEMP_FILE"
  if [[ "$CLIO_DAEMON_ROLLBACK_PENDING" == true ]]; then
    if restore_daemon "$CLIO_DAEMON_BACKUP" "$CLIO_DAEMON_DESTINATION" \
      "$CLIO_DAEMON_DOMAIN" "$CLIO_DAEMON_DATABASE"; then
      printf 'Restored the previous daemon after an interrupted installation.\n' >&2
    else
      printf 'ERROR: Could not restore the previous daemon from %s\n' "$CLIO_DAEMON_BACKUP" >&2
    fi
    CLIO_DAEMON_ROLLBACK_PENDING=false
  fi
  if [[ -n "$CLIO_MOVED_APP" && -d "$CLIO_MOVED_APP" ]]; then
    if [[ -e "$CLIO_APP_PATH" ]]; then
      interrupted_app="$(dirname "$CLIO_MOVED_APP")/Clio.interrupted.$$.app"
      mv "$CLIO_APP_PATH" "$interrupted_app" || printf 'ERROR: Could not preserve the interrupted app at %s\n' "$interrupted_app" >&2
    fi
    mv "$CLIO_MOVED_APP" "$CLIO_APP_PATH" || printf 'ERROR: Could not restore the previous Clio app from %s\n' "$CLIO_MOVED_APP" >&2
  fi
  return "$exit_status"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
require_tool() { command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"; }
require_macos() { [[ "$(uname -s)" == Darwin ]] || fail "This script only runs on macOS."; }

normalise_sha() {
  [[ "${1:-}" =~ ^[[:xdigit:]]{40}$ ]] || fail "A full 40-character commit SHA is required."
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

check_checkout() {
  local sha="$1" head resolved remote
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
  require_macos
  for tool in git cargo rustc python3 install mktemp; do require_tool "$tool"; done
  check_checkout "$sha"
  cargo metadata --manifest-path "$CLIO_REPO_DIR/Cargo.toml" --locked --no-deps --format-version 1 >/dev/null
}

backup_file() {
  local source="$1" name="$2" backup_dir="$3"
  CLIO_LAST_BACKUP=""
  [[ ! -e "$source" ]] || {
    install -d -m 700 "$backup_dir/bin"
    CLIO_LAST_BACKUP="$backup_dir/bin/$name.$(date -u +%Y%m%dT%H%M%SZ).$$"
    cp -p "$source" "$CLIO_LAST_BACKUP"
  }
}

atomic_install() {
  local source="$1" destination="$2" parent
  parent="$(dirname "$destination")"
  install -d -m 755 "$parent"
  CLIO_TEMP_FILE="$(mktemp "$parent/.$(basename "$destination").XXXXXX")"
  install -m 755 "$source" "$CLIO_TEMP_FILE"
  mv -f "$CLIO_TEMP_FILE" "$destination"
  CLIO_TEMP_FILE=""
}

plist_value() {
  python3 - "$CLIO_PLIST_PATH" "$1" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as handle:
    arguments = plistlib.load(handle).get("ProgramArguments", [])
if sys.argv[2] == "program":
    if arguments:
        print(arguments[0])
elif sys.argv[2] == "database":
    for index, value in enumerate(arguments):
        if value == "--db-path" and index + 1 < len(arguments):
            print(arguments[index + 1])
            break
        if value.startswith("--db-path="):
            print(value.split("=", 1)[1])
            break
PY
}

app_is_running() { pgrep -x Clio >/dev/null 2>&1 || pgrep -x clio-tauri >/dev/null 2>&1; }

daemon_ready() {
  local database="$1" attempt=0 status_json=""
  while [[ $attempt -lt 10 ]]; do
    status_json="$("$CLIO_BIN_DIR/clio" --db-path "$database" --json daemon status 2>/dev/null || true)"
    if printf '%s' "$status_json" | python3 -c 'import json,sys; value=json.load(sys.stdin); pid=value.get("pid"); raise SystemExit(0 if isinstance(pid, int) and pid > 0 else 1)' 2>/dev/null; then
      "$CLIO_BIN_DIR/clio" --db-path "$database" --json daemon doctor >/dev/null
      return
    fi
    attempt=$((attempt + 1))
    sleep 1
  done
  return 1
}

activate_daemon() {
  local source="$1" destination="$2" domain="$3" database="$4"
  atomic_install "$source" "$destination" || return 1
  codesign --verify --strict --verbose=2 "$destination" || return 1
  launchctl bootstrap "$domain" "$CLIO_PLIST_PATH" || return 1
  daemon_ready "$database"
}

restore_daemon() {
  local backup="$1" destination="$2" domain="$3" database="$4"
  launchctl bootout "$domain" "$CLIO_PLIST_PATH" >/dev/null 2>&1 || true
  [[ -n "$backup" && -f "$backup" ]] || return 1
  atomic_install "$backup" "$destination" || return 1
  codesign --verify --strict --verbose=2 "$destination" || return 1
  launchctl bootstrap "$domain" "$CLIO_PLIST_PATH" || return 1
  daemon_ready "$database"
}

install_daemon() {
  local backup_dir="$1" daemon_destination="$CLIO_BIN_DIR/clio-daemon" database="" domain candidate previous
  require_tool codesign
  domain="gui/$(id -u)"
  candidate="$CLIO_REPO_DIR/target/release/clio-daemon"
  codesign --verify --strict --verbose=2 "$candidate"
  if [[ -f "$CLIO_PLIST_PATH" ]]; then
    daemon_destination="$(plist_value program)"
    database="$(plist_value database)"
    [[ "$daemon_destination" == /* ]] || fail "The daemon path in $CLIO_PLIST_PATH is not absolute."
    [[ -n "$database" ]] || fail "The LaunchAgent does not configure --db-path."
  fi
  backup_file "$daemon_destination" clio-daemon "$backup_dir"
  previous="$CLIO_LAST_BACKUP"
  if [[ ! -f "$CLIO_PLIST_PATH" ]]; then
    atomic_install "$candidate" "$daemon_destination"
    codesign --verify --strict --verbose=2 "$daemon_destination"
    printf 'Installed clio-daemon, but no LaunchAgent is configured; it was not started.\n'
    return
  fi

  CLIO_DAEMON_BACKUP="$previous"
  CLIO_DAEMON_DESTINATION="$daemon_destination"
  CLIO_DAEMON_DOMAIN="$domain"
  CLIO_DAEMON_DATABASE="$database"
  CLIO_DAEMON_ROLLBACK_PENDING=true
  launchctl bootout "$domain" "$CLIO_PLIST_PATH" >/dev/null 2>&1 || true
  if ! activate_daemon "$candidate" "$daemon_destination" "$domain" "$database"; then
    if restore_daemon "$previous" "$daemon_destination" "$domain" "$database"; then
      CLIO_DAEMON_ROLLBACK_PENDING=false
      fail "The new daemon failed its health check; the previous daemon was restored."
    fi
    fail "The new daemon failed and automatic restoration also failed; rollback binary: $previous"
  fi
  CLIO_DAEMON_ROLLBACK_PENDING=false
  printf 'Daemon installed and healthy for %s.\n' "$database"
}

latest_dmg() {
  python3 - "$1" <<'PY'
import sys
from pathlib import Path

files = list(Path(sys.argv[1]).glob("*.dmg"))
if files:
    print(max(files, key=lambda item: item.stat().st_mtime))
PY
}

install_app() {
  local sha="$1" backup_dir="$2" target_dir built_app dmg app_backup failed_app dmg_destination
  app_is_running && fail "Clio is running; quit it before installing the desktop app."
  target_dir="$(cargo metadata --manifest-path "$CLIO_REPO_DIR/Cargo.toml" --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
  built_app="$target_dir/release/bundle/macos/Clio.app"
  [[ -d "$built_app" ]] || fail "Built application not found: $built_app"
  codesign --verify --deep --strict "$built_app"
  install -d -m 700 "$backup_dir"
  dmg="$(latest_dmg "$target_dir/release/bundle/dmg")"
  if [[ -n "$dmg" ]]; then
    dmg_destination="$backup_dir/$(basename "$dmg")"
    [[ ! -e "$dmg_destination" ]] || dmg_destination="$backup_dir/$(date -u +%Y%m%dT%H%M%SZ)-$(basename "$dmg")"
    cp -p "$dmg" "$dmg_destination"
  fi
  app_is_running && fail "Clio was opened during the build; quit it before installing."
  install -d -m 755 "$(dirname "$CLIO_APP_PATH")"
  if [[ -d "$CLIO_APP_PATH" ]]; then
    app_backup="$backup_dir/Clio.$(date -u +%Y%m%dT%H%M%SZ).$$.app"
    mv "$CLIO_APP_PATH" "$app_backup"
    CLIO_MOVED_APP="$app_backup"
  fi
  if ! ditto "$built_app" "$CLIO_APP_PATH" || ! codesign --verify --deep --strict "$CLIO_APP_PATH"; then
    if [[ -d "$CLIO_APP_PATH" ]]; then
      failed_app="$backup_dir/Clio.failed.$(date -u +%Y%m%dT%H%M%SZ).$$.app"
      mv "$CLIO_APP_PATH" "$failed_app"
    fi
    [[ -z "$CLIO_MOVED_APP" ]] || mv "$CLIO_MOVED_APP" "$CLIO_APP_PATH"
    CLIO_MOVED_APP=""
    fail "Desktop application installation failed; the previous app was restored."
  fi
  CLIO_MOVED_APP=""
  printf 'Desktop app installed at %s; build DMG retained in %s.\n' "$CLIO_APP_PATH" "$backup_dir"
}

check_command() {
  local sha="$1"
  preflight "$sha"
  if [[ -x "$CLIO_BIN_DIR/clio" ]]; then
    "$CLIO_BIN_DIR/clio" --version
    "$CLIO_BIN_DIR/clio" remote-mcp --help >/dev/null
  else
    printf 'No existing clio bridge is installed at %s.\n' "$CLIO_BIN_DIR/clio"
  fi
  printf 'macOS checkout is ready for native %s installation of %s.\n' "$(uname -m)" "$sha"
}

install_command() {
  local sha="$1" with_daemon="$2" with_app="$3" backup_dir sign_identity
  preflight "$sha"
  if [[ "$with_app" == true ]]; then
    require_tool codesign; require_tool ditto; require_tool npm; require_tool pgrep
    cargo tauri --version >/dev/null 2>&1 || fail "cargo-tauri is required for --with-app."
    app_is_running && fail "Clio is running; quit it before installing the desktop app."
  fi
  cargo build --manifest-path "$CLIO_REPO_DIR/Cargo.toml" --locked --release --no-default-features -p clio-cli --bin clio
  [[ "$with_daemon" != true ]] || cargo build --manifest-path "$CLIO_REPO_DIR/Cargo.toml" --locked --release -p clio-daemon --bin clio-daemon
  if [[ "$with_app" == true ]]; then
    npm --prefix "$CLIO_REPO_DIR/ui" ci
    sign_identity="${APPLE_SIGNING_IDENTITY:--}"
    (cd "$CLIO_REPO_DIR/crates/clio-tauri" && APPLE_SIGNING_IDENTITY="$sign_identity" cargo tauri build --ci --bundles app,dmg)
  fi
  check_checkout "$sha"
  backup_dir="$CLIO_BACKUP_ROOT/$sha"
  backup_file "$CLIO_BIN_DIR/clio" clio "$backup_dir"
  atomic_install "$CLIO_REPO_DIR/target/release/clio" "$CLIO_BIN_DIR/clio"
  "$CLIO_BIN_DIR/clio" --version
  "$CLIO_BIN_DIR/clio" remote-mcp --help >/dev/null
  [[ "$with_daemon" != true ]] || install_daemon "$backup_dir"
  [[ "$with_app" != true ]] || install_app "$sha" "$backup_dir"
  printf 'Installed Clio from %s.\n' "$sha"
}

case "${1:-}" in
  -h|--help|help|'') usage ;;
  check) [[ $# -eq 2 ]] || fail "Usage: $0 check <full-sha>"; sha="$(normalise_sha "$2")"; check_command "$sha" ;;
  install)
    [[ $# -ge 2 ]] || fail "Usage: $0 install <full-sha> [--with-daemon] [--with-app]"
    sha="$(normalise_sha "$2")"; shift 2; with_daemon=false; with_app=false
    while [[ $# -gt 0 ]]; do
      case "$1" in --with-daemon) with_daemon=true ;; --with-app) with_app=true ;; *) fail "Unknown option: $1" ;; esac
      shift
    done
    install_command "$sha" "$with_daemon" "$with_app"
    ;;
  *) fail "Unknown command: $1" ;;
esac
