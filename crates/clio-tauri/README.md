# clio-tauri

Desktop UI shell built with Tauri. Local mode consumes `clio-core` directly for
browse, edit, archive, and inspect workflows.

## Atlas remote mode

Set `CLIO_REMOTE_HOST` to route supported desktop operations through the
existing `clio remote-mcp` SSH bridge:

```sh
CLIO_REMOTE_HOST=atlas \
CLIO_REMOTE_DB_PATH=/home/ubuntu/.local/share/clio/memory.db \
CLIO_REMOTE_BINARY=/home/ubuntu/.local/bin/clio-mcp \
CLIO_REMOTE_COMMAND=/Users/dannyharding/.cargo/bin/clio \
./dev.sh
```

`CLIO_REMOTE_DB_PATH` and `CLIO_REMOTE_BINARY` are required in remote mode.
`CLIO_REMOTE_COMMAND` defaults to `clio`. Leave `CLIO_REMOTE_HOST` unset to use
the existing local database resolution (`CLIO_DB_PATH`, then the platform
default).

The app bar shows the active backend and connection state. A remote
configuration error never falls back to a local database.

Remote mode supports the normal memory, archive, link, namespace, statistics,
and semantic-search workflows exposed by MCP. Bulk operations, import/export,
database maintenance, deduplication, and namespace administration remain local
only; their UI controls are hidden. Remote mode requires a live SSH connection
and has no offline cache or synchronisation. Restart the app to reconnect after
the bridge process or SSH connection exits.
