#!/usr/bin/env bash
# Start money·data in two separate Terminal windows:
#   • one for the Rust backend  (http://127.0.0.1:8080)
#   • one for the Vite frontend (http://localhost:5173)
# Each window runs its server in the foreground, so pressing Ctrl-C in a
# window stops just that server.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$ROOT/backend"
FRONTEND_DIR="$ROOT/frontend"

# Postgres connection. Override by exporting DATABASE_URL before running; the
# default targets a throwaway local container (md-pg) that we start below.
DATABASE_URL="${DATABASE_URL:-postgres://postgres:pw@127.0.0.1:5432/postgres}"

# Bring up a throwaway Postgres in Docker if one isn't already running.
ensure_pg() {
  command -v docker >/dev/null 2>&1 || {
    echo "⚠ docker not found — ensure DATABASE_URL points at a running Postgres" >&2
    return
  }
  if docker ps --format '{{.Names}}' | grep -qx md-pg; then
    echo "▸ Postgres (md-pg) already running"
    return
  fi
  if docker ps -a --format '{{.Names}}' | grep -qx md-pg; then
    echo "▸ Starting existing Postgres container (md-pg)…"
    docker start md-pg >/dev/null
  else
    echo "▸ Launching Postgres container (md-pg)…"
    docker run -d --name md-pg -p 5432:5432 -e POSTGRES_PASSWORD=pw postgres:16-alpine >/dev/null
  fi
  for _ in $(seq 1 30); do
    docker exec md-pg pg_isready -U postgres >/dev/null 2>&1 && break
    sleep 1
  done
}
ensure_pg

# Commands each Terminal window will run. `exec $SHELL` keeps the window open
# after the server stops (e.g. after Ctrl-C) so you can see any final output.
BACKEND_CMD="cd '$BACKEND_DIR' && export DATABASE_URL='$DATABASE_URL' && echo '▸ Rust backend → http://127.0.0.1:8080' && cargo run; exec \$SHELL"
FRONTEND_CMD="cd '$FRONTEND_DIR' && { [ -d node_modules ] || npm install; } && echo '▸ Vite frontend → http://localhost:5173' && npm run dev; exec \$SHELL"

# Open each command in its own new Terminal window.
open_terminal() {
  osascript - "$1" <<'APPLESCRIPT'
on run argv
  tell application "Terminal"
    activate
    do script (item 1 of argv)
  end tell
end run
APPLESCRIPT
}

echo "▸ Opening backend window…"
open_terminal "$BACKEND_CMD"

echo "▸ Opening frontend window…"
open_terminal "$FRONTEND_CMD"

echo ""
echo "  money·data is starting in two Terminal windows:"
echo "    backend  → http://127.0.0.1:8080"
echo "    frontend → http://localhost:5173   (open this)"
echo ""
echo "  Press Ctrl-C inside a window to stop that server."
