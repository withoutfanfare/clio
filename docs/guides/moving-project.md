# Moving the Clio Project Directory

This guide covers relocating the Clio project to a different directory on your filesystem.

## What's Safe

The codebase itself is fully portable — all internal paths are relative:

- `build.sh` / `dev.sh` — resolve their own location via `$(dirname "$0")`
- `Cargo.toml` workspace members — relative paths
- `tauri.conf.json` — relative `../../ui/dist` reference
- SQLite database — lives at `~/Library/Application Support/clio/`, not in the project

## What Breaks

External configs embed absolute paths at setup/install time:

| Config | Location | Fix |
|--------|----------|-----|
| Daemon plist | `~/Library/LaunchAgents/com.clio.daemon.plist` | `clio daemon install` |
| Claude Code MCP | `~/.claude.json` | `clio setup claude-code` |
| Cursor MCP | `~/.cursor/mcp.json` | `clio setup cursor` |
| Windsurf MCP | `~/.windsurf/mcp.json` | `clio setup windsurf` |
| Gemini config | `.gemini/settings.json` (in project) | Delete, then `clio setup gemini` |
| Claude Code project memory | `~/.claude/projects/-<old-path>/memory/` | Copy to new path key (see below) |

## Steps

```bash
# 1. Stop the daemon
clio daemon stop

# 2. Move the project
mv ~/old/path/clio ~/new/path/clio

# 3. Rebuild (dependencies may need reinstalling if previously cleaned)
cd ~/new/path/clio
cd ui && npm install && cd ..
./build.sh

# 4. Reinstall the daemon (unload stale plist first)
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.clio.daemon.plist 2>/dev/null
clio daemon install

# 5. Regenerate MCP configs for each client you use
clio setup claude-code
clio setup cursor
# clio setup windsurf
# clio setup gemini

# 6. Clean stale in-project config
rm -f .gemini/settings.json

# 7. (Optional) Migrate Claude Code project memory
#    Claude Code keys project memory by filesystem path.
#    The key format is the absolute path with / replaced by -
OLD_KEY="-Old-Path-To-Project"
NEW_KEY="-New-Path-To-Project"
mkdir -p ~/.claude/projects/${NEW_KEY}/memory
cp ~/.claude/projects/${OLD_KEY}/memory/* ~/.claude/projects/${NEW_KEY}/memory/
```

## Verification

After the move, confirm everything works:

```bash
clio recall -n 3          # CLI can reach the database
clio daemon status        # Daemon is running
```

Then open a new Claude Code session from the new directory and verify MCP tools respond.

## Notes

- The `.fastembed_cache/` directory regenerates automatically on first embedding use.
- Installed binaries at `~/.cargo/bin/` are unaffected by the move — they don't reference the source directory.
- The database at `~/Library/Application Support/clio/` is completely independent of the project location.
