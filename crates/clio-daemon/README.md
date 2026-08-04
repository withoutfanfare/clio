# clio-daemon

Always-on local daemon for lifecycle management and background processing.

## Responsibilities

- PID file singleton locking
- Unix domain socket control channel (status, stop, health)
- Inbox folder watcher — processes new files through capture pipeline or stores as notes
- Dual tracing: stderr + daily rolling log files
- Auto-link inference — periodically creates links between semantically similar memories
- Periodic local backup and integrity schedulers (disabled until configured)
- Graceful SIGTERM/SIGINT shutdown with cleanup

Inbox files are moved to `_processed/` only after capture/queueing or a durable
fallback note succeeds. A failed database write leaves the source file in place
for retry. Oversized and empty files are deliberate rejections and are moved.

Status reports only implemented routes: the control socket, configured watcher
and capture pipeline, enabled auto-linker, and each enabled maintenance
scheduler. `daemon.http_port` is retained as a reserved settings compatibility
field; the daemon does not start an HTTP listener.

Must NOT become the only way to use Clio, expose network listeners outside localhost, or implement storage semantics outside the core.
