# clio-daemon

Always-on local daemon for lifecycle management and background processing.

## Responsibilities

- PID file singleton locking
- Unix domain socket control channel (status, stop, health)
- Inbox folder watcher — processes new files through capture pipeline or stores as notes
- Dual tracing: stderr + daily rolling log files
- Auto-link inference — periodically creates links between semantically similar memories
- Graceful SIGTERM/SIGINT shutdown with cleanup

Must NOT become the only way to use Clio, expose network listeners outside localhost, or implement storage semantics outside the core.
