#!/bin/bash
# Build and install Clio binaries, then restart the daemon.
#
# Usage:
#   ./build.sh          # build all crates and restart daemon
#   ./build.sh cli      # build only clio CLI
#   ./build.sh mcp      # build only clio-mcp
#   ./build.sh daemon   # build only clio-daemon and restart it
#   ./build.sh tauri    # launch the desktop app in dev mode (hot reload)
set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

TARGETS="${1:-all}"

build_cli() {
  echo "Building clio CLI..."
  cargo install --path crates/clio-cli --bin clio
}

build_mcp() {
  echo "Building clio-mcp..."
  cargo install --path crates/clio-mcp --bin clio-mcp
}

build_daemon() {
  echo "Building clio-daemon..."
  cargo install --path crates/clio-daemon --bin clio-daemon
}

restart_daemon() {
  local PLIST="$HOME/Library/LaunchAgents/com.clio.daemon.plist"
  local DOMAIN
  DOMAIN="gui/$(id -u)"

  if [ ! -f "$PLIST" ]; then
    echo "clio-daemon LaunchAgent is not configured; built binary only."
    return 0
  fi

  echo "Restarting clio-daemon..."
  launchctl bootout "$DOMAIN" "$PLIST" 2>/dev/null || true
  launchctl bootstrap "$DOMAIN" "$PLIST"
  sleep 2

  # Verify it came back
  PID=$(launchctl print "$DOMAIN/com.clio.daemon" 2>/dev/null | awk '/pid =/ {print $3; exit}')
  if [ -n "$PID" ]; then
    echo "clio-daemon running (PID $PID)"
  else
    echo "WARNING: clio-daemon may not have started. Check logs:"
    echo "  tail -20 ~/Library/Logs/clio/clio-daemon.stderr.log"
    return 1
  fi
}

case "$TARGETS" in
  all)
    build_cli
    build_mcp
    build_daemon
    restart_daemon
    echo "All crates built and daemon restarted."
    ;;
  cli)
    build_cli
    ;;
  mcp)
    build_mcp
    ;;
  daemon)
    build_daemon
    restart_daemon
    ;;
  restart)
    restart_daemon
    ;;
  tauri)
    echo "Launching desktop app in dev mode (hot reload)..."
    exec "$ROOT/dev.sh"
    ;;
  *)
    echo "Unknown target: $TARGETS"
    echo "Usage: ./build.sh [all|cli|mcp|daemon|restart|tauri]"
    exit 1
    ;;
esac
