#!/bin/bash
# Start Clio Tauri app (frontend + backend)
set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"

# Kill any leftover Vite on port 5173
lsof -ti:5173 | xargs kill 2>/dev/null || true

cd "$ROOT/ui" && npm run dev &
VITE_PID=$!

trap "kill $VITE_PID 2>/dev/null" EXIT

sleep 2

cd "$ROOT/crates/clio-tauri" && cargo tauri dev
